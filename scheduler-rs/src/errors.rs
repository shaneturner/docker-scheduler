use std::time::Duration;
use thiserror::Error;

// Define a top-level error enum for the application
#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("Configuration error: {0}")]
    Config(#[from] anyhow::Error), // Can wrap general config errors from anyhow

    #[error("Docker API error: {0}")]
    Docker(#[from] bollard::errors::Error), // Errors from the bollard Docker client

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error), // Standard IO errors

    #[error("Cron parsing error for '{cron_str}': {source}")]
    CronParse {
        cron_str: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>, // << CORRECTED ::
    },

    #[error("Command execution failed for service '{service_name}': {source}")]
    CommandExec {
        service_name: String,
        #[source]
        source: std::io::Error, // Subprocess errors are typically IO errors
    },

    #[error("Command timed out for service '{service_name}' after {duration:?}")]
    CommandTimeout {
        service_name: String,
        duration: Duration,
    },

    #[error("Command failed for service '{service_name}' with exit code {code}. Stderr:\n{stderr}")]
    CommandFailed {
        service_name: String,
        code: i32,
        stderr: String,
    },

    #[error("Failed to parse command '{command_str}': {details}")]
    CommandParse {
        command_str: String,
        details: String,
    },

    #[error("Failed to add job '{job_id}' to scheduler: {source}")]
    JobAdd {
        job_id: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>, // << CORRECTED ::
    },

    #[error("Failed to modify job '{job_id}': {source}")]
    JobModify {
        job_id: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>, // << CORRECTED ::
    },

    #[error("Failed to remove job '{job_id}': {source}")]
    JobRemove {
        job_id: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>, // << CORRECTED ::
    },

    #[error("Invalid timezone identifier: {0}")]
    InvalidTimezone(String),

    #[error("Label parsing error for container {container_id}: {message}")]
    LabelParse {
        container_id: String,
        message: String,
    },

    // Catch-all for other unexpected errors if needed
    #[error("An unexpected error occurred: {0}")]
    Other(String),
}

// Define a Result type alias using our custom error
pub type Result<T> = std::result::Result<T, SchedulerError>;
