import logging
import os
import shlex  # For safer command splitting
import subprocess
import time
from typing import Any, Dict, List, Optional, Set

import docker
from apscheduler.jobstores.base import JobLookupError
from apscheduler.schedulers.background import BackgroundScheduler
from apscheduler.triggers.cron import CronTrigger
from docker.models.containers import Container
from dotenv import load_dotenv
from pytz import timezone as pytz_timezone
from pytz import UnknownTimeZoneError

# --- Environment Loading ---
# Load .env file if present (useful for local development)
load_dotenv()

# --- Configuration Constants ---
# Read from environment variables with defaults
LOG_LEVEL: str = os.environ.get("LOG_LEVEL", "INFO").upper()
SCHEDULER_TIMEZONE: str = os.environ.get("SCHEDULER_TIMEZONE", "UTC")
SCHEDULE_INTERVAL_MINUTES: int = int(
    os.environ.get("SCHEDULE_INTERVAL_MINUTES", "1")
)
COMMAND_TIMEOUT_SECONDS: int = int(
    os.environ.get("COMMAND_TIMEOUT_SECONDS", "300") # 5 minutes default
)
DOCKER_SOCKET_PATH: str = "/var/run/docker.sock" # Standard path

# Docker Compose environment variables
COMPOSE_PROJECT_NAME: str = os.environ.get("COMPOSE_PROJECT_NAME", "")
DOCKER_CONTEXT: str = os.environ.get("DOCKER_CONTEXT", "default")

# Label configuration
LABEL_PREFIX: str = "custom.scheduler."
LABEL_ENABLE: str = f"{LABEL_PREFIX}enable"
LABEL_ACTION: str = f"{LABEL_PREFIX}action"
LABEL_COMMAND: str = f"{LABEL_PREFIX}command"
LABEL_CRON: str = f"{LABEL_PREFIX}cron"

# Standard Docker Compose label for service name
COMPOSE_SERVICE_LABEL: str = "com.docker.compose.service"

# Internal constants
JOB_ID_PREFIX: str = "compose_job_"

# --- Logging Setup ---
logging.basicConfig(
    level=getattr(logging, LOG_LEVEL, logging.INFO),
    format="%(asctime)s - %(levelname)s - [%(name)s] %(message)s",
)
log = logging.getLogger("scheduler")

# --- Docker Client Setup ---
try:
    docker_client: docker.DockerClient = docker.from_env(
        # Explicitly using version avoids potential negotiation issues
        version="auto"
    )
    # Verify connection by pinging the Docker daemon
    if not docker_client.ping():
         raise ConnectionError("Docker daemon ping failed.")
    log.info("Successfully connected to Docker daemon.")
except docker.errors.DockerException as e:
    log.error(f"Failed to connect to Docker daemon at '{DOCKER_SOCKET_PATH}': {e}")
    log.error("Ensure the Docker socket is mounted correctly and the daemon is running.")
    exit(1)
except ConnectionError as e:
     log.error(f"Failed to ping Docker daemon: {e}")
     exit(1)
except Exception as e:
    # Catch unexpected errors during client initialization
    log.error(f"An unexpected error occurred initializing Docker client: {e}")
    exit(1)


# --- APScheduler Setup ---
try:
    tz = pytz_timezone(SCHEDULER_TIMEZONE)
    scheduler = BackgroundScheduler(timezone=tz)
    log.info(f"APScheduler initialized with timezone: {SCHEDULER_TIMEZONE}")
except UnknownTimeZoneError:
    log.error(f"Invalid SCHEDULER_TIMEZONE specified: '{SCHEDULER_TIMEZONE}'. Defaulting to UTC.")
    tz = pytz_timezone("UTC")
    scheduler = BackgroundScheduler(timezone=tz)
except Exception as e:
    log.error(f"Failed to initialize APScheduler: {e}")
    exit(1)


# --- Core Functions ---

def run_compose_command(action: str, service_name: str, command_str: str) -> None:
    """
    Constructs and runs the 'docker compose' command using subprocess.

    Args:
        action: The compose action ('exec', 'run', 'run-rm').
        service_name: The target service name from docker-compose.yml.
        command_str: The command string to execute.
    """
    # Include context and project name in base command
    base_cmd: List[str] = ["docker"]
    
    # Add Docker context if specified
    if DOCKER_CONTEXT:
        base_cmd.extend(["--context", DOCKER_CONTEXT])
    
    base_cmd.append("compose")
    
    # Add project name if specified
    if COMPOSE_PROJECT_NAME:
        base_cmd.extend(["-p", COMPOSE_PROJECT_NAME])
    
    # Use shlex.split for robust parsing of the command string
    try:
        command_parts: List[str] = shlex.split(command_str)
    except ValueError as e:
        log.error(f"Error splitting command for service '{service_name}': '{command_str}'. Error: {e}")
        return

    cmd: List[str]
    if action == "exec":
        # 'exec' typically doesn't need --rm, it runs in an existing container
        cmd = base_cmd + ["exec", service_name] + command_parts
    elif action == "run":
        # 'run' creates a new container
        cmd = base_cmd + ["run", service_name] + command_parts
    elif action == "run-rm":
        # 'run --rm' creates a new container and removes it afterwards
        cmd = base_cmd + ["run", "--rm", service_name] + command_parts
    else:
        log.error(f"Unknown action '{action}' requested for service '{service_name}'")
        return

    log.info(f"Executing: {' '.join(cmd)}")
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            check=False, # Don't raise exception on non-zero exit code
            timeout=COMMAND_TIMEOUT_SECONDS
        )
        if result.returncode == 0:
            log.info(f"Command succeeded for '{service_name}'. Output:\n{result.stdout.strip()}")
            # Optionally log stderr even on success if it contains info
            if result.stderr:
                 log.info(f"Stderr from '{service_name}':\n{result.stderr.strip()}")
        else:
            log.error(f"Command failed for '{service_name}'. RC: {result.returncode}")
            if result.stdout:
                 log.error(f"Stdout:\n{result.stdout.strip()}")
            if result.stderr:
                 log.error(f"Stderr:\n{result.stderr.strip()}")

    except FileNotFoundError:
         log.error(f"Error: 'docker compose' command not found in PATH inside the scheduler container.")
    except subprocess.TimeoutExpired:
         log.error(f"Command timed out after {COMMAND_TIMEOUT_SECONDS}s for service '{service_name}': {' '.join(cmd)}")
    except Exception as e:
        # Catch other potential subprocess errors
        log.error(f"An unexpected error occurred running command for service '{service_name}': {e}")


def parse_labels_and_get_job_config(container: Container) -> Optional[Dict[str, Any]]:
    """
    Parses scheduler labels from a container and validates them.

    Args:
        container: The Docker container object.

    Returns:
        A dictionary containing validated job configuration (action, command, cron, service_name)
        or None if labels are missing, invalid, or disabled.
    """
    labels: Dict[str, str] = container.labels

    if labels.get(LABEL_ENABLE) != "true":
        return None # Scheduling not enabled for this container

    service_name: Optional[str] = labels.get(COMPOSE_SERVICE_LABEL)
    action: Optional[str] = labels.get(LABEL_ACTION)
    command: Optional[str] = labels.get(LABEL_COMMAND)
    cron_schedule: Optional[str] = labels.get(LABEL_CRON)

    container_id_short: str = container.short_id
    log_prefix: str = f"Container {container_id_short}"
    if service_name:
         log_prefix += f" (Service: {service_name})"


    # Validate essential labels
    if not all([service_name, action, command, cron_schedule]):
        log.debug(f"{log_prefix}: Missing one or more essential labels ({COMPOSE_SERVICE_LABEL}, {LABEL_ACTION}, {LABEL_COMMAND}, {LABEL_CRON}). Skipping.")
        return None
    if action not in ["exec", "run", "run-rm"]:
         log.warning(f"{log_prefix}: Invalid action '{action}'. Must be 'exec', 'run', or 'run-rm'. Skipping.")
         return None

    # Validate cron schedule format (basic check using APScheduler)
    try:
        CronTrigger.from_crontab(cron_schedule)
    except ValueError as e:
        log.warning(f"{log_prefix}: Invalid cron string '{cron_schedule}'. Error: {e}. Skipping.")
        return None

    return {
        "service_name": service_name,
        "action": action,
        "command": command,
        "cron_schedule": cron_schedule,
        "job_id": f"{JOB_ID_PREFIX}{service_name}" # Simple ID based on service name
    }


def discover_and_update_schedules() -> None:
    """
    Discovers Compose services via container labels and updates APScheduler jobs.
    Adds new jobs, modifies changed ones, and removes jobs for containers
    that no longer have valid scheduling labels.
    """
    log.info("Starting schedule discovery and update...")
    active_job_ids: Set[str] = set()
    processed_services: Set[str] = set() # Track services processed to use latest definition found

    try:
        # List all containers (including stopped ones) to read labels
        containers: List[Container] = docker_client.containers.list(all=True)
        log.debug(f"Found {len(containers)} total containers.")
    except docker.errors.APIError as e:
        log.error(f"Error listing containers from Docker API: {e}")
        return # Skip update cycle if Docker API fails

    # --- Process containers and configure jobs ---
    for container in containers:
        job_config = parse_labels_and_get_job_config(container)

        if not job_config:
            continue # Skip if labels invalid or disabled

        service_name: str = job_config["service_name"]
        job_id: str = job_config["job_id"]

        # If multiple containers exist for a service (e.g., during rolling update),
        # ensure we only schedule one job based on the most recent container found?
        # Or assume labels are consistent. For simplicity, let's schedule based on
        # the first valid labeled container found for a service in this loop.
        # If a service scales, this assumes all instances have identical schedule labels.
        if service_name in processed_services:
             log.debug(f"Service '{service_name}' already processed in this cycle. Skipping duplicate container {container.short_id}.")
             continue
        processed_services.add(service_name)
        active_job_ids.add(job_id) # Mark this job ID as active for this cycle

        # --- Get details for APScheduler ---
        action: str = job_config["action"]
        command: str = job_config["command"]
        cron_schedule: str = job_config["cron_schedule"]
        job_args: List[str] = [action, service_name, command]

        try:
            trigger = CronTrigger.from_crontab(cron_schedule, timezone=scheduler.timezone)
        except ValueError:
            # This should have been caught by parse_labels_and_get_job_config, but double-check
            log.error(f"Internal Error: Invalid cron string '{cron_schedule}' for job '{job_id}' passed validation earlier. Skipping.")
            continue

        # --- Add or Modify Job in APScheduler ---
        existing_job = scheduler.get_job(job_id)

        if existing_job:
            # Compare current config with existing job's config
            # Note: Comparing triggers directly can be complex. Compare cron string.
            current_cron_expr: str = getattr(existing_job.trigger, 'expression', '') # Handle potential differences in trigger types
            current_args: List[Any] = list(existing_job.args)

            if current_cron_expr != cron_schedule or current_args != job_args:
                log.info(f"Modifying job '{job_id}' for service '{service_name}'. Schedule or command changed.")
                try:
                    scheduler.modify_job(job_id, trigger=trigger, args=job_args)
                except JobLookupError:
                     log.warning(f"Job '{job_id}' disappeared before modification. Will be re-added if still valid.")
                except Exception as e:
                     log.error(f"Failed to modify job '{job_id}': {e}")
            else:
                log.debug(f"Job '{job_id}' for service '{service_name}' is up-to-date.")
        else:
            # Add new job
            log.info(f"Adding new job '{job_id}' for service '{service_name}' with schedule: '{cron_schedule}'")
            try:
                scheduler.add_job(
                    run_compose_command,
                    trigger=trigger,
                    args=job_args,
                    id=job_id,
                    name=f"Compose task for {service_name}",
                    replace_existing=True, # Replace if job with same ID exists but wasn't fetched (edge case)
                    misfire_grace_time=60 # Allow job to run if it missed schedule by up to 60s
                )
            except Exception as e:
                log.error(f"Failed to add job '{job_id}': {e}")

    # --- Remove Stale Jobs ---
    # Get all jobs managed by this scheduler instance
    all_scheduler_job_ids: Set[str] = {
        job.id for job in scheduler.get_jobs() if job.id.startswith(JOB_ID_PREFIX)
    }
    stale_job_ids: Set[str] = all_scheduler_job_ids - active_job_ids

    for job_id in stale_job_ids:
        log.info(f"Removing stale job '{job_id}' (service/container likely removed or labels changed).")
        try:
            scheduler.remove_job(job_id)
        except JobLookupError:
            log.debug(f"Job '{job_id}' already removed.") # Already gone, ignore
        except Exception as e:
            log.error(f"Failed to remove stale job '{job_id}': {e}")

    log.info(f"Scheduling update complete. Active jobs managed: {len(active_job_ids)}. Total jobs in scheduler: {len(scheduler.get_jobs())}")


# --- Main Execution ---
def main() -> None:
    """Main function to start the scheduler and discovery loop."""
    log.info("Starting Compose Scheduler...")
    
    # Log Docker Compose configuration
    if COMPOSE_PROJECT_NAME:
        log.info(f"Using Docker Compose project name: {COMPOSE_PROJECT_NAME}")
    if DOCKER_CONTEXT:
        log.info(f"Using Docker context: {DOCKER_CONTEXT}")

    # Add the discovery task itself to run periodically
    try:
        scheduler.add_job(
            discover_and_update_schedules,
            trigger='interval',
            minutes=SCHEDULE_INTERVAL_MINUTES,
            id='discover_jobs_task',
            name='Discover Docker Compose Schedules',
            replace_existing=True
        )
        log.info(f"Scheduled discovery task to run every {SCHEDULE_INTERVAL_MINUTES} minute(s).")
    except Exception as e:
         log.error(f"Fatal: Could not schedule the discovery task: {e}")
         exit(1)

    # Run initial discovery immediately before starting the loop
    log.info("Running initial discovery...")
    discover_and_update_schedules()

    # Start the scheduler's background thread
    try:
        scheduler.start()
        log.info("Scheduler background thread started.")
    except Exception as e:
         log.error(f"Fatal: Could not start the scheduler: {e}")
         exit(1)


    log.info("Scheduler running. Press Ctrl+C to exit.")

    # Keep the main thread alive, handle shutdown gracefully
    try:
        while True:
            time.sleep(60) # Check less frequently, APScheduler runs in background
    except (KeyboardInterrupt, SystemExit):
        log.info("Shutdown signal received.")
    finally:
        log.info("Shutting down scheduler...")
        try:
             # Give running jobs a chance to finish? APScheduler shutdown handles this.
            scheduler.shutdown()
            log.info("Scheduler shut down gracefully.")
        except Exception as e:
             log.error(f"Error during scheduler shutdown: {e}")

if __name__ == "__main__":
    main()
