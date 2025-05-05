use crate::commands;
use crate::config::Config;
use crate::docker_api::{self, JobConfig};
use crate::errors::{Result, SchedulerError};
use bollard::Docker;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;
use std::future::Future;
use std::pin::Pin;

#[instrument(skip(_config))]
pub async fn init_scheduler(_config: &Config) -> Result<JobScheduler> {
    info!("Initializing job scheduler...");
    let scheduler = JobScheduler::new()
        .await
        .map_err(|e| SchedulerError::Other(format!("Failed to create scheduler: {}", e)))?;
    info!("Job scheduler initialized.");
    Ok(scheduler)
}

#[instrument(skip(scheduler, docker_client, config, job_map))]
pub async fn discover_and_update_schedules(
    scheduler: Arc<Mutex<JobScheduler>>,
    docker_client: Arc<Docker>,
    config: Arc<Config>,
    job_map: &mut HashMap<String, Uuid>,
) {
    info!("Starting schedule discovery and update cycle...");
    debug!("Current jobs tracked in map before update: {}", job_map.len());

    let discovered_tasks = match docker_api::discover_scheduled_tasks(&docker_client, &config).await {
        Ok(tasks) => tasks,
        Err(e) => {
            error!("Failed to discover tasks from Docker: {}", e);
            return;
        }
    };
    let discovered_map: HashMap<String, JobConfig> = discovered_tasks
        .into_iter()
        .map(|task| (task.job_id.clone(), task))
        .collect();
    let active_job_ids_this_cycle: HashSet<String> = discovered_map.keys().cloned().collect();

    // Add or recreate jobs
    for (job_id_str, task_config) in &discovered_map {
        if let Some(existing_uuid) = job_map.get(job_id_str).copied() { // Keep UUID for removal logic
            info!(
                // Modify message: Remove UUID placeholder
                "Job '{}' exists. Removing and recreating to ensure updated schedule.",
                job_id_str
            );
            {
                let sched = scheduler.lock().await;
                if let Err(e) = sched.remove(&existing_uuid).await { // Need UUID here
                    error!(
                        // Modify message: Remove UUID placeholder
                        "Failed to remove job '{}' for recreation: {}",
                        job_id_str, e
                    );
                    continue;
                }
            }
            job_map.remove(job_id_str); // Remove from map using job_id_str
            info!(
                // Modify message: Remove UUID placeholder
                "Removed job '{}' from scheduler and map for recreation.",
                job_id_str
            );

            match add_task_job(
                Arc::clone(&scheduler),
                task_config,
                Arc::clone(&config),
            )
            .await
            {
                Ok(new_uuid) => {
                    job_map.insert(job_id_str.clone(), new_uuid); // Need new_uuid for map
                    info!(
                        // Modify message: Remove UUID placeholder
                        "Re-added job '{}'.",
                        job_id_str
                    );
                }
                Err(e) => {
                    error!("Failed to re-add job '{}' after modification: {}", job_id_str, e);
                }
            }
        } else {
            info!(
                "Adding newly discovered job '{}' with schedule '{}'",
                job_id_str, task_config.cron_schedule
            );
            match add_task_job(
                Arc::clone(&scheduler),
                task_config,
                Arc::clone(&config),
            )
            .await
            {
                Ok(new_uuid) => {
                    job_map.insert(job_id_str.clone(), new_uuid); // Need new_uuid for map
                    info!(
                        // Modify message: Remove UUID placeholder
                        "Added new job '{}'.",
                        job_id_str
                    );
                }
                Err(e) => {
                    error!("Failed to add new job '{}': {}", job_id_str, e);
                }
            }
        }
    }

    // Remove stale jobs
    info!("Checking for stale jobs to remove...");
    let mut stale = Vec::new();
    for (job_id_str, uuid) in job_map.iter() { // Need UUID for removal logic
        if !active_job_ids_this_cycle.contains(job_id_str) {
            stale.push((job_id_str.clone(), *uuid));
        }
    }
    let mut had_errors = false;
    for (job_id_str, uuid_to_remove) in stale { // Need UUID for removal logic
        warn!(
            // Modify message: Remove UUID placeholder
            "Removing stale job '{}'", job_id_str
        );
        {
            let sched = scheduler.lock().await;
            if let Err(e) = sched.remove(&uuid_to_remove).await { // Need UUID here
                error!(
                    // Modify message: Remove UUID placeholder
                    "Failed to remove stale job '{}' from scheduler: {}",
                    job_id_str, e
                );
                had_errors = true;
                continue;
            }
        }
        job_map.remove(&job_id_str); // Remove from map using job_id_str
        info!(
            // Modify message: Remove UUID placeholder
            "Successfully removed stale job '{}'", job_id_str
        );
    }

    info!(
        "Schedule update cycle complete. Jobs tracked in map: {}.{}",
        job_map.len(),
        if had_errors { " (Errors during removal)" } else { "" }
    );
}

async fn add_task_job(
    scheduler: Arc<Mutex<JobScheduler>>,
    task_config: &JobConfig,
    config: Arc<Config>,
) -> Result<Uuid> { // Still need to return Uuid for the map
    let action = task_config.action.clone();
    let service_name = task_config.service_name.clone();
    let command_str = task_config.command.clone();
    let job_id_str = task_config.job_id.clone();
    let config_clone = Arc::clone(&config);

    let standard_cron = task_config.cron_schedule.as_str();
    let full_cron = format!("0 {}", standard_cron);

    debug!("Converting cron from '{}' to '{}'", standard_cron, full_cron);

    let job_closure = move |_job_uuid: Uuid, _ctx: JobScheduler| { // job_uuid is passed by the scheduler, keep it in the closure signature
        let inner_action = action.clone();
        let inner_service_name = service_name.clone();
        let inner_command_str = command_str.clone();
        let inner_job_id_str = job_id_str.clone();
        let inner_config = Arc::clone(&config_clone);

        Box::pin(async move {
            info!(
                // Modify message: Remove job_uuid field
                // job_uuid = %job_uuid,
                job_id = %inner_job_id_str,
                service = %inner_service_name,
                action = %inner_action,
                "Executing scheduled job"
            );
            match commands::run_compose_command(
                &inner_config,
                &inner_action,
                &inner_service_name,
                &inner_command_str,
            )
            .await
            {
                Ok(_) => info!(
                    // Modify message: Remove job_uuid field
                    // job_uuid = %job_uuid,
                    job_id = %inner_job_id_str,
                    service = %inner_service_name,
                    "Scheduled job completed successfully."
                ),
                Err(e) => error!(
                    // Modify message: Remove job_uuid field
                    // job_uuid = %job_uuid,
                    job_id = %inner_job_id_str,
                    service = %inner_service_name,
                    "Scheduled job failed: {}",
                    e
                ),
            }
        }) as Pin<Box<dyn Future<Output = ()> + Send>>
    };

    let job = Job::new_async_tz(
        full_cron.as_str(),
        config.scheduler_timezone,
        job_closure,
    )
    .map_err(|e| SchedulerError::CronParse {
        cron_str: task_config.cron_schedule.clone(),
        source: Box::new(e),
    })?;

    let added_uuid = { // Need the Uuid to return
        let lock = scheduler.lock().await;
        lock.add(job)
            .await
            .map_err(|e| SchedulerError::JobAdd {
                job_id: task_config.job_id.clone(),
                source: Box::new(e),
            })?
    };

    info!(
        // Modify message: Remove UUID placeholder
        "Successfully added job '{}' to scheduler.",
        task_config.job_id
    );
    Ok(added_uuid) // Still return the Uuid
}
