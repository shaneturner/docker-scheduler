mod config;
mod docker_api;
mod commands;
mod errors;

use errors::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    tracing::info!("Loading configuration...");
    let config = config::load_config().map_err(errors::SchedulerError::Config)?;
    tracing::info!("Configuration loaded successfully: {:?}", config);

    tracing::info!("Connecting to Docker...");
    let docker_client = docker_api::connect_to_docker().await?; // <<< ADD THIS
    tracing::info!("Docker connection successful.");
    
    tracing::info!("Rust Scheduler starting setup...");

    // --- Placeholder for scheduler setup & main loop ---
    // TODO: Initialize scheduler
    // TODO: Add periodic discovery job
    // TODO: Run initial discovery and schedule tasks
    // TODO: Start scheduler & await shutdown signal

    tracing::warn!("Scheduler setup not yet implemented. Exiting after initial checks."); // Add a temporary warning
    // Keep running for now, but eventually replace with scheduler logic
    // loop {
    //     tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    // }

    Ok(()) // Exit cleanly after setup phase for now
    
}
