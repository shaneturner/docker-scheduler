mod commands;
mod config;
mod docker_api;
mod errors;
mod scheduler;

use errors::Result;
use std::sync::Arc;

use tracing::info;


#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("Loading configuration...");
    let config = Arc::new(config::load_config().map_err(errors::SchedulerError::Config)?);
    info!("Configuration loaded successfully: {:?}", config);

    info!("Connecting to Docker...");
    // Prefix unused variable with _
    let _docker_client = Arc::new(docker_api::connect_to_docker().await?);
    info!("Docker connection successful.");

    info!("Initializing scheduler...");
    // Prefix unused variable with _
    let _scheduler_instance = Arc::new(scheduler::init_scheduler(&config).await?);
    info!("Scheduler initialized."); // Added confirmation

    // --- Placeholder for scheduler setup & main loop ---
    // The errors below about discover_and_update_schedules arguments are still expected

    info!("Rust Scheduler starting setup...");

    // TODO: Initialize job_map (probably using Mutex or RwLock)
    // TODO: Add periodic discovery job (passing the job_map)
    // TODO: Run initial discovery and schedule tasks (passing the job_map)
    // TODO: Start scheduler & await shutdown signal

    info!("Scheduler setup not yet implemented. Exiting after initial checks.");

    Ok(())
}
