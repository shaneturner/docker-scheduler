use crate::config::Config;
use crate::errors::{Result, SchedulerError}; // Use our custom Result and Error
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command; // Use tokio's async Command
use tracing::{debug, error, info, instrument, warn}; // Import tracing macros

// Function to construct and run the 'docker compose' command
#[instrument(skip(config, command_str), fields(service = %service_name, action = %action))]
pub async fn run_compose_command(
    config: &Config, // Pass config for project name, context, timeout
    action: &str,
    service_name: &str,
    command_str: &str,
) -> Result<()> { // Returns Ok(()) on success, or SchedulerError on failure
    let mut base_cmd_args: Vec<String> = vec!["docker".to_string()];

    // Add Docker context if specified
    if let Some(context) = &config.docker_context {
        base_cmd_args.push("--context".to_string());
        base_cmd_args.push(context.clone());
        debug!("Using Docker context: {}", context);
    }

    base_cmd_args.push("compose".to_string());

    // Add project name if specified
    if let Some(project) = &config.compose_project_name {
        base_cmd_args.push("-p".to_string());
        base_cmd_args.push(project.clone());
        debug!("Using Compose project name: {}", project);
    }

    // Parse the command string using shlex for safety
    let command_parts = match shlex::split(command_str) {
        Some(parts) => parts,
        None => {
            return Err(SchedulerError::CommandParse {
                command_str: command_str.to_string(),
                details: "Failed to split command string using shlex.".to_string(),
            });
        }
    };
    if command_parts.is_empty() && (action == "exec" || action == "run" || action == "run-rm") {
         // Allow empty command for 'up', 'down', etc. if we supported them,
         // but for exec/run, an empty command is likely an error.
          return Err(SchedulerError::CommandParse {
                command_str: command_str.to_string(),
                details: "Command string is empty or resulted in no arguments.".to_string(),
            });
    }


    // Build the final command vector based on action
    let mut final_cmd_args = base_cmd_args;
    match action {
        "exec" => {
            final_cmd_args.push("exec".to_string());
            // Add any necessary flags for exec? E.g., -T for no TTY? Assume defaults for now.
            final_cmd_args.push(service_name.to_string());
            final_cmd_args.extend(command_parts);
        }
        "run" => {
            final_cmd_args.push("run".to_string());
            // Add flags for run? --no-deps? Assume defaults for now.
            final_cmd_args.push(service_name.to_string());
            final_cmd_args.extend(command_parts);
        }
        "run-rm" => {
            final_cmd_args.push("run".to_string());
            final_cmd_args.push("--rm".to_string()); // Add the --rm flag
            final_cmd_args.push(service_name.to_string());
            final_cmd_args.extend(command_parts);
        }
        _ => {
            // This should ideally be caught during label parsing, but double-check
            error!("Unsupported action type '{}' requested for service '{}'", action, service_name);
            // Return an error instead of just logging
             return Err(SchedulerError::Other(format!("Unsupported action '{}'", action)));
        }
    }

    info!("Executing command: {}", final_cmd_args.join(" "));

    // Create the command using tokio::process::Command
    let mut command = Command::new(&final_cmd_args[0]); // "docker"
    command.args(&final_cmd_args[1..]); // Add the rest of the arguments
    command.stdout(Stdio::piped()); // Capture stdout
    command.stderr(Stdio::piped()); // Capture stderr
    // Prevent the child process from inheriting parent's stdin
    command.stdin(Stdio::null());

    // Execute with timeout
    let timeout_duration = config.command_timeout;
    match tokio::time::timeout(timeout_duration, command.output()).await {
        // Timeout occurred
        Err(_) => {
            error!(
                "Command timed out after {:?} for service '{}'",
                timeout_duration, service_name
            );
            Err(SchedulerError::CommandTimeout {
                service_name: service_name.to_string(),
                duration: timeout_duration,
            })
        }
        // Command finished, check result
        Ok(output_result) => {
            match output_result {
                // Process spawn or IO error
                Err(e) => {
                    error!(
                        "Failed to execute command for service '{}': {}",
                        service_name, e
                    );
                    Err(SchedulerError::CommandExec {
                        service_name: service_name.to_string(),
                        source: e,
                    })
                }
                // Command executed, check status code
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                    if output.status.success() {
                        info!(
                            "Command succeeded for service '{}'. Exit code: {}",
                            service_name,
                            output.status.code().unwrap_or(-1) // Should always have code if success() is true
                        );
                        // Log stdout/stderr only if they contain content
                        if !stdout.trim().is_empty() {
                            debug!("Stdout:\n{}", stdout.trim());
                        }
                        if !stderr.trim().is_empty() {
                            // Use warn for stderr even on success, as it might contain useful info/warnings
                            warn!("Stderr:\n{}", stderr.trim());
                        }
                        Ok(()) // Indicate success
                    } else {
                        error!(
                            "Command failed for service '{}'. Exit code: {}",
                            service_name,
                            output.status.code().unwrap_or(-1) // May not have code in all failure cases? Use -1 as sentinel
                        );
                         if !stdout.trim().is_empty() {
                            error!("Stdout:\n{}", stdout.trim());
                        }
                        if !stderr.trim().is_empty() {
                             error!("Stderr:\n{}", stderr.trim());
                        }
                        Err(SchedulerError::CommandFailed {
                            service_name: service_name.to_string(),
                            code: output.status.code().unwrap_or(-1),
                            stderr, // Include stderr in the error type
                        })
                    }
                }
            }
        }
    }
}
