use crate::commands;
use crate::config::Config;
use crate::docker_api::{self, JobConfig};
use crate::errors::{Result, SchedulerError};

use bollard::Docker;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
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
    scheduler: Arc<JobScheduler>,
    docker_client: Arc<Docker>,
    config: Arc<Config>,
    job_map: &mut HashMap<String, Uuid>,
) {
    info!("Starting schedule discovery and update cycle...");
    // Discover tasks from Docker
    let discovered_tasks = match docker_api::discover_scheduled_tasks(&docker_client, &config).await
    {
        Ok(tasks) => tasks,
        Err(e) => { error!("Failed to discover tasks from Docker: {}", e); return; }
    };
    let discovered_map: HashMap<String, JobConfig> = discovered_tasks
        .into_iter()
        .map(|task| (task.job_id.clone(), task))
        .collect();
    let active_job_ids_this_cycle: HashSet<String> = discovered_map.keys().cloned().collect();

    // Get the current job count
    match scheduler.as_ref().jobs_count().await {
        Ok(count) => debug!("Current job count before update: {}", count),
        Err(e) => warn!("Failed to get job count: {}", e),
    }

    // Process all discovered tasks
    for (job_id_str, task_config) in &discovered_map {
        // Since we can't check if the job exists directly with newer API versions,
        // we'll maintain our job map and just remove/readd if needed
        if job_map.contains_key(job_id_str) {
            // Check if the cron schedule has changed
            let existing_uuid = job_map[job_id_str];
            warn!("Job '{}' exists with UUID {}. Removing and recreating to ensure updated schedule.", 
                job_id_str, existing_uuid);
            
            // Always remove and re-add to ensure latest schedule is used
            if let Err(e) = scheduler.as_ref().remove(&existing_uuid).await {
                error!("Failed to remove job '{}' ({}) for recreation: {}", job_id_str, existing_uuid, e);
                continue;
            }
            
            // Remove from map before re-adding
            job_map.remove(job_id_str);
            info!("Removed job '{}' ({}) from scheduler and map for recreation.", job_id_str, existing_uuid);
            
            // Add the job again
            match add_task_job(scheduler.as_ref(), task_config, Arc::clone(&docker_client), Arc::clone(&config)).await {
                Ok(new_uuid) => { 
                    job_map.insert(job_id_str.clone(), new_uuid);
                    info!("Re-added job '{}' with new UUID {}", job_id_str, new_uuid);
                }
                Err(e) => { 
                    error!("Failed to re-add job '{}' after modification: {}", job_id_str, e);
                }
            }
        } else {
            // Job doesn't exist yet, add it
            info!("Adding newly discovered job '{}' with schedule '{}'", job_id_str, task_config.cron_schedule);
            match add_task_job(scheduler.as_ref(), task_config, Arc::clone(&docker_client), Arc::clone(&config)).await {
                Ok(new_uuid) => { 
                    job_map.insert(job_id_str.clone(), new_uuid);
                    info!("Added new job '{}' with UUID {}", job_id_str, new_uuid);
                }
                Err(e) => { 
                    error!("Failed to add new job '{}': {}", job_id_str, e);
                }
            }
        }
    }

    // Check for stale jobs to remove
    info!("Checking for stale jobs to remove...");
    let mut stale_job_ids_to_remove = Vec::new();
    for (job_id_str, uuid) in job_map.iter() {
        if !active_job_ids_this_cycle.contains(job_id_str) {
            stale_job_ids_to_remove.push((job_id_str.clone(), *uuid));
        }
    }
    let mut removal_errors = false;
    for (job_id_str, uuid_to_remove) in stale_job_ids_to_remove {
        warn!("Removing stale job '{}' ({})", job_id_str, uuid_to_remove);
        if let Err(e) = scheduler.as_ref().remove(&uuid_to_remove).await {
            error!("Failed to remove stale job '{}' ({}) from scheduler: {}", job_id_str, uuid_to_remove, e);
            removal_errors = true;
        } else {
            job_map.remove(&job_id_str);
            info!("Successfully removed stale job '{}' ({})", job_id_str, uuid_to_remove);
        }
    }

    // Get final job count
    let current_scheduler_job_count = match scheduler.as_ref().jobs_count().await {
        Ok(count) => count,
        Err(e) => { 
            warn!("Could not get job count from scheduler: {}", e); 
            job_map.len() 
        }
    };
    
    info!(
        "Schedule update cycle complete. Jobs tracked in map: {}. Jobs in scheduler: {}.{}",
        job_map.len(), current_scheduler_job_count, if removal_errors { " (Errors occurred during removal)" } else { "" }
    );
}

// Helper function to add a job for a specific task config
async fn add_task_job(
    scheduler: &JobScheduler,
    task_config: &JobConfig,
    _docker_client: Arc<Docker>,
    config: Arc<Config>,
) -> Result<Uuid> {
    let action = task_config.action.clone();
    let service_name = task_config.service_name.clone();
    let command_str = task_config.command.clone();
    let job_id_str = task_config.job_id.clone();
    let config_clone = Arc::clone(&config);

    // Corrected Closure Signature: Use JobScheduler for the second argument
    let job_closure = move |job_uuid: Uuid, _scheduler_context: JobScheduler| { // <<< Corrected type
        let inner_action = action.clone();
        let inner_service_name = service_name.clone();
        let inner_command_str = command_str.clone();
        let inner_job_id_str = job_id_str.clone();
        let inner_config_clone = Arc::clone(&config_clone);

        // Explicitly cast the pinned boxed future to a trait object
        let fut = Box::pin(async move {
            info!(job_uuid = %job_uuid, job_id = %inner_job_id_str, service = %inner_service_name, action = %inner_action, "Executing scheduled job");
            match commands::run_compose_command(
                &inner_config_clone, &inner_action, &inner_service_name, &inner_command_str
            ).await {
                Ok(_) => info!(job_uuid = %job_uuid, job_id = %inner_job_id_str, service = %inner_service_name, "Scheduled job completed successfully."),
                Err(e) => error!(job_uuid = %job_uuid, job_id = %inner_job_id_str, service = %inner_service_name, "Scheduled job failed: {}", e),
            }
       }) as Pin<Box<dyn Future<Output = ()> + Send>>; // <<< CAST HERE
       fut // Return the cast future
    };

    let job = Job::new_async_tz(
        task_config.cron_schedule.as_str(),
        config.scheduler_timezone,
        job_closure,
    )
    .map_err(|e| SchedulerError::CronParse {
        cron_str: task_config.cron_schedule.clone(),
        source: Box::new(e),
    })?;

    let added_job_uuid = scheduler.add(job).await
        .map_err(|e| SchedulerError::JobAdd {
            job_id: task_config.job_id.clone(),
            source: Box::new(e),
        })?;

    info!("Successfully added job '{}' to scheduler with UUID: {}", task_config.job_id, added_job_uuid);
    Ok(added_job_uuid)
}
