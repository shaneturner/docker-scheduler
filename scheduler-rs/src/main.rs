mod config;
mod docker_api; // <<< ADD THIS
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

    // --- Test discovery (temporary) ---
    tracing::info!("Running initial discovery...");
    let discovered_tasks = docker_api::discover_scheduled_tasks(&docker_client, &config).await?;
    tracing::info!("Discovered tasks: {:?}", discovered_tasks);
    // --- End Test ---


    tracing::info!("Rust Scheduler starting setup...");

    // --- Placeholder ---

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
    // Ok(())
}
