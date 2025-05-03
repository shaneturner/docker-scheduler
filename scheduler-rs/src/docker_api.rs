use crate::config::Config;
use crate::errors::{Result, SchedulerError}; // Use our custom Result and Error
use bollard::container::{ListContainersOptions, LogsOptions, LogOutput};
use bollard::models::ContainerSummary;
use bollard::Docker;
use std::collections::HashMap;
use std::default::Default;
use tracing::{debug, info, instrument, warn}; // Import tracing macros
use futures_util::TryStreamExt;

// --- Constants for Labels ---
const LABEL_PREFIX: &str = "custom.scheduler.";
const LABEL_ENABLE: &str = "custom.scheduler.enable";
const LABEL_ACTION: &str = "custom.scheduler.action";
const LABEL_COMMAND: &str = "custom.scheduler.command";
const LABEL_CRON: &str = "custom.scheduler.cron";
// Standard Docker Compose label
const COMPOSE_SERVICE_LABEL: &str = "com.docker.compose.service";
// Docker Compose project label (used for filtering)
const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";

// Structure to hold the parsed job configuration from labels
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobConfig {
    pub service_name: String,
    pub container_id: String, // Keep track of the specific container ID found
    pub action: String,
    pub command: String,
    pub cron_schedule: String,
    pub job_id: String, // Derived unique ID for the scheduler job
}

// Connect to the Docker daemon via the default socket
// The 'instrument' macro automatically adds spans to traces
#[instrument]
pub async fn connect_to_docker() -> Result<Docker> {
    info!("Connecting to Docker daemon...");
    // Assumes Docker socket is mounted at the default path
    // Or DOCKER_HOST env var is set
    let docker = Docker::connect_with_local_defaults()
        .map_err(|e| SchedulerError::Docker(bollard::errors::Error::from(e)))?;

    // Verify connection by pinging
    docker.ping().await?;
    info!("Successfully connected to Docker daemon.");
    Ok(docker)
}

// List containers (optionally filtered by compose project) and parse labels
#[instrument(skip(docker, config))]
pub async fn discover_scheduled_tasks(
    docker: &Docker,
    config: &Config,
) -> Result<Vec<JobConfig>> {
    debug!("Starting container discovery for scheduled tasks...");
    let mut filters = HashMap::new();

    // --- MODIFICATION START ---
    // Add filter for compose project name if provided in config
    if let Some(project_name) = &config.compose_project_name {
        // Filter by the label key AND value
        let label_filter = format!("{}={}", COMPOSE_PROJECT_LABEL, project_name);
        debug!("Filtering containers using label: {}", label_filter);
        // Use "label" as the filter KEY, and "key=value" as the filter VALUE
        filters.insert("label".to_string(), vec![label_filter]);
    } else {
        debug!("No Compose project filter specified, scanning all containers.");
        // Optional: Filter for only containers that *have* the enable label?
        // This might reduce the number of containers processed later.
        // filters.insert("label".to_string(), vec![LABEL_ENABLE.to_string()]);
    }


    let options = ListContainersOptions {
        all: true, // Include stopped containers to read labels consistently
        filters,
        ..Default::default()
    };

    let containers = docker.list_containers(Some(options)).await?;
    debug!("Found {} containers matching filters.", containers.len());

    let mut discovered_jobs = Vec::new();
    let mut processed_services = std::collections::HashSet::new(); // Track processed services

    for container in containers {
        let container_id = container.id.clone().unwrap_or_else(|| "unknown".to_string());
        let container_id_short = container_id.chars().take(12).collect::<String>();

        match parse_labels(&container, &container_id_short) {
            Ok(Some(job_config)) => {
                // Avoid scheduling duplicate jobs if multiple containers exist for the same service
                // (e.g., during updates or scaling). Use the first valid one found.
                if processed_services.insert(job_config.service_name.clone()) {
                    debug!("Found valid schedule for service '{}' from container {}", job_config.service_name, container_id_short);
                    discovered_jobs.push(job_config);
                } else {
                     debug!("Service '{}' already processed, skipping container {}", job_config.service_name, container_id_short);
                }
            }
            Ok(None) => {
                // Labels missing or scheduling disabled, ignore quietly or debug log
                debug!("Container {} does not have valid scheduling labels or is disabled.", container_id_short);
            }
            Err(e) => {
                // Log label parsing errors but continue scanning other containers
                warn!("{}", e); // Log the warning from LabelParse error
            }
        }
    }

    info!("Discovery complete. Found {} unique services with valid schedules.", discovered_jobs.len());
    Ok(discovered_jobs)
}

// Parses labels from a single container summary
fn parse_labels(container: &ContainerSummary, container_id_short: &str) -> Result<Option<JobConfig>> {
    let labels = match &container.labels {
        Some(l) => l,
        None => return Ok(None), // No labels, definitely no schedule
    };

    // Check if scheduling is explicitly enabled
    if labels.get(LABEL_ENABLE).map(|v| v.as_str()) != Some("true") {
        return Ok(None);
    }

    // Extract required labels
    let service_name = labels.get(COMPOSE_SERVICE_LABEL);
    let action = labels.get(LABEL_ACTION);
    let command = labels.get(LABEL_COMMAND);
    let cron_schedule = labels.get(LABEL_CRON);

    // Validate essential labels are present
    match (service_name, action, command, cron_schedule) {
        (Some(sn), Some(a), Some(cmd), Some(cron)) => {
            // Validate action type
            if !["exec", "run", "run-rm"].contains(&a.as_str()) {
                return Err(SchedulerError::LabelParse {
                    container_id: container_id_short.to_string(),
                    message: format!("Invalid action '{}'. Must be 'exec', 'run', or 'run-rm'.", a),
                });
            }

             // Basic cron validation (more thorough validation happens in scheduler)
            if cron.split_whitespace().count() < 5 {
                 return Err(SchedulerError::LabelParse {
                     container_id: container_id_short.to_string(),
                     message: format!("Invalid cron string '{}'. Too few fields.", cron),
                 });
            }

            let job_id = format!("compose_job_{}", sn); // Consistent job ID based on service name

            Ok(Some(JobConfig {
                service_name: sn.clone(),
                container_id: container.id.clone().unwrap_or_default(),
                action: a.clone(),
                command: cmd.clone(),
                cron_schedule: cron.clone(),
                job_id,
            }))
        }
        _ => {
            // One or more essential labels are missing, even though enabled="true"
            // Log this as potentially misconfigured
            debug!(
                "Container {} has '{}' set to true, but is missing one or more required labels ({}*, {}*, {}*, {}*).",
                container_id_short, LABEL_ENABLE, COMPOSE_SERVICE_LABEL, LABEL_ACTION, LABEL_COMMAND, LABEL_CRON
            );
            Ok(None)
        }
    }
}

// Example function to get logs (might be useful for debugging)
#[allow(dead_code)]
#[instrument(skip(docker))]
pub async fn get_container_logs(docker: &Docker, container_id: &str, tail: usize) -> Result<String> {
    info!("Fetching logs for container {}...", container_id);
    let options = LogsOptions {
        stdout: true,
        stderr: true,
        tail: tail.to_string(), // Show last 'tail' lines
        ..Default::default()
    };

    let logs_stream = docker.logs(container_id, Some(options));

    use futures_util::TryStreamExt; // Make sure this line is present and uncommented
    let log_chunks: Vec<LogOutput> = logs_stream.try_collect().await?; // <<< Use LogOutput directly

    let logs = log_chunks.iter().map(|chunk| chunk.to_string()).collect();

    Ok(logs)
}
