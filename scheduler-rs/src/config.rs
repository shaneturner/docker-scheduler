use anyhow::{Context, Result}; // Using anyhow for simple error propagation here
use std::env;
use std::time::Duration;
use chrono_tz::Tz;

// Define a struct to hold the application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub scheduler_timezone: Tz,
    pub schedule_interval: Duration,
    pub command_timeout: Duration,
    pub compose_project_name: Option<String>,
    pub docker_context: Option<String>,
    // RUST_LOG is typically handled by tracing_subscriber::fmt::init() or EnvFilter
}

// Function to load configuration from environment variables
pub fn load_config() -> Result<Config> {
    // Load .env file if present. Ignore errors if it doesn't exist.
    dotenvy::dotenv().ok();

    // --- Load individual variables ---

    // Timezone
    let tz_str = env::var("SCHEDULER_TIMEZONE").unwrap_or_else(|_| "UTC".to_string());
    let scheduler_timezone: Tz = tz_str.parse().map_err(|e| {
        anyhow::anyhow!("Invalid SCHEDULER_TIMEZONE '{}': {}", tz_str, e)
    })?;

    // Schedule Interval
    let interval_minutes = env::var("SCHEDULE_INTERVAL_MINUTES")
        .unwrap_or_else(|_| "1".to_string())
        .parse::<u64>()
        .context("Failed to parse SCHEDULE_INTERVAL_MINUTES as an integer")?;
    let schedule_interval = Duration::from_secs(interval_minutes * 60);

    // Command Timeout
    let timeout_seconds = env::var("COMMAND_TIMEOUT_SECONDS")
        .unwrap_or_else(|_| "600".to_string())
        .parse::<u64>()
        .context("Failed to parse COMMAND_TIMEOUT_SECONDS as an integer")?;
    let command_timeout = Duration::from_secs(timeout_seconds);

    // Read the variable, get Option<String>, then filter: if Some(s) where s is empty, becomes None.
    let compose_project_name = env::var("COMPOSE_PROJECT_NAME").ok().filter(|s| !s.is_empty());
    let docker_context = env::var("DOCKER_CONTEXT").ok().filter(|s| !s.is_empty());


    // --- Construct Config struct ---
    Ok(Config {
        scheduler_timezone,
        schedule_interval,
        command_timeout,
        compose_project_name,
        docker_context,
    })
}

// Helper function to get string env var with default
#[allow(dead_code)] // Keep potentially useful helper
fn get_env_var(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}
