mod commands;
mod config;
mod docker_api;
mod errors;
mod scheduler;

use errors::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
// Remove the unused Duration import
// use std::time::Duration;
use tokio::signal;
use tokio::time::sleep;
use tracing::{error, info};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing (logging)
    tracing_subscriber::fmt::init();

    info!("Loading configuration...");
    let config = Arc::new(config::load_config().map_err(errors::SchedulerError::Config)?);
    info!("Configuration loaded successfully: {:?}", config);

    info!("Connecting to Docker...");
    let docker_client = Arc::new(docker_api::connect_to_docker().await?);
    info!("Docker connection successful.");

    info!("Initializing scheduler...");
    let scheduler_instance = Arc::new(scheduler::init_scheduler(&config).await?);
    info!("Scheduler initialized.");

    // Initialize job tracking map
    let job_map = Arc::new(Mutex::new(HashMap::<String, Uuid>::new()));

    info!("Starting scheduler...");
    if let Err(e) = scheduler_instance.start().await {
        error!("Failed to start scheduler: {}", e);
        return Err(errors::SchedulerError::Other(format!("Failed to start scheduler: {}", e)));
    }
    info!("Scheduler started successfully.");

    // Run initial discovery
    info!("Running initial discovery and scheduling...");
    let scheduler_ref = Arc::clone(&scheduler_instance);
    let docker_ref = Arc::clone(&docker_client);
    let config_ref = Arc::clone(&config);
    let job_map_ref = Arc::clone(&job_map);
    
    // Run initial discovery - fixed to avoid MutexGuard Send issues
    tokio::spawn(async move {
        // Scope to ensure the mutex guard is dropped before await
        {
            let mut job_map_guard = job_map_ref.lock().unwrap();
            scheduler::discover_and_update_schedules(
                Arc::clone(&scheduler_ref),
                Arc::clone(&docker_ref),
                Arc::clone(&config_ref),
                &mut *job_map_guard,
            )
            .await;
        } // MutexGuard is dropped here
    })
    .await
    .unwrap();
    
    info!("Initial discovery complete.");

    // Set up periodic discovery task
    let scheduler_ref = Arc::clone(&scheduler_instance);
    let docker_ref = Arc::clone(&docker_client);
    let config_ref = Arc::clone(&config);
    let job_map_ref = Arc::clone(&job_map);
    
    let discovery_interval = config.schedule_interval;
    info!("Setting up periodic discovery task (interval: {:?})...", discovery_interval);
    
    tokio::spawn(async move {
        loop {
            sleep(discovery_interval).await;
            info!("Running periodic discovery and update...");
            
            // Use a block scope to ensure MutexGuard is dropped before sleep.await
            {
                let mut job_map_guard = match job_map_ref.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("Failed to acquire job map lock: {}", e);
                        continue;
                    }
                };
                
                scheduler::discover_and_update_schedules(
                    Arc::clone(&scheduler_ref),
                    Arc::clone(&docker_ref),
                    Arc::clone(&config_ref),
                    &mut *job_map_guard,
                )
                .await;
            } // MutexGuard is dropped here
        }
    });

    info!("Scheduler running. Press Ctrl+C to exit.");
    
    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Shutdown signal received.");
        }
        Err(e) => {
            error!("Failed to listen for shutdown signal: {}", e);
        }
    }

    // Shutdown the scheduler - fixed to handle Arc correctly
    info!("Shutting down scheduler...");
    // Get a clone of the Arc to avoid borrow issues
    let scheduler_for_shutdown = Arc::clone(&scheduler_instance);
    if let Err(e) = scheduler_for_shutdown.shutdown().await {
        error!("Error shutting down scheduler: {}", e);
    }
    info!("Scheduler shut down successfully.");

    Ok(())
}
