mod commands;
mod config;
mod docker_api;
mod errors;
mod scheduler;

use errors::SchedulerError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), SchedulerError> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env()) // Optional: Keeps RUST_LOG functionality
        .with_target(false) // <-- This line removes the module path (target)
        // .with_timer(fmt::time::UtcTime::rfc_3339()) // Optional: Customize timestamp if needed
        .init(); // Initialize the subscriber

    info!("Loading configuration...");
    let config = Arc::new(config::load_config().map_err(SchedulerError::Config)?);
    info!("Configuration loaded successfully: {:?}", config);

    info!("Connecting to Docker...");
    let docker_client = Arc::new(docker_api::connect_to_docker().await?);
    info!("Docker connection successful.");

    info!("Initializing scheduler...");
    // <-- wrap scheduler in a Mutex for interior mutability
    let scheduler_instance = Arc::new(Mutex::new(scheduler::init_scheduler(&config).await?));
    info!("Scheduler initialized.");

    let job_map = Arc::new(Mutex::new(HashMap::<String, Uuid>::new()));

    // --- Start the scheduler ---
    info!("Starting scheduler...");
    {
        let sched = scheduler_instance.lock().await;
        sched.start().await.map_err(|e| {
            error!("Failed to start scheduler: {}", e);
            SchedulerError::Other(format!("Failed to start scheduler: {}", e))
        })?;
    }
    info!("Scheduler started successfully.");

    // --- Initial discovery ---
    info!("Running initial discovery and scheduling...");
    {
        let sched_clone = Arc::clone(&scheduler_instance);
        let docker_clone = Arc::clone(&docker_client);
        let cfg_clone = Arc::clone(&config);
        let map_clone = Arc::clone(&job_map);

        tokio::spawn(async move {
            let mut map = map_clone.lock().await;
            scheduler::discover_and_update_schedules(
                sched_clone,
                docker_clone,
                cfg_clone,
                &mut *map,
            )
            .await;
        })
        .await
        .map_err(|e| {
            SchedulerError::Other(format!("Initial discovery task panicked: {:?}", e))
        })?;
    }
    info!("Initial discovery complete.");

    // --- Periodic discovery ---
    let interval = config.schedule_interval;
    info!("Setting up periodic discovery task (interval: {:?})...", interval);
    {
        let sched_clone = Arc::clone(&scheduler_instance);
        let docker_clone = Arc::clone(&docker_client);
        let cfg_clone = Arc::clone(&config);
        let map_clone = Arc::clone(&job_map);

        tokio::spawn(async move {
            loop {
                sleep(interval).await;
                info!("Running periodic discovery and update...");
                let mut map = map_clone.lock().await;
                scheduler::discover_and_update_schedules(
                    sched_clone.clone(),
                    docker_clone.clone(),
                    cfg_clone.clone(),
                    &mut *map,
                )
                .await;
            }
        });
    }

    info!("Scheduler running. Press Ctrl+C to exit.");

    // --- Wait for Ctrl+C ---
    if let Err(e) = signal::ctrl_c().await {
        error!("Failed to listen for shutdown signal: {}", e);
    } else {
        info!("Shutdown signal received.");
    }

    // --- Graceful shutdown ---
    info!("Shutting down scheduler...");
    {
        let mut sched = scheduler_instance.lock().await;
        if let Err(e) = sched.shutdown().await {
            error!("Error shutting down scheduler: {}", e);
        }
    }
    info!("Scheduler shut down successfully.");

    Ok(())
}
