# Docker Controller Container

This project demonstrates how to create a Docker container that can control other containers within a docker-compose configuration. It can execute commands inside other containers, start temporary single-run containers with the `--rm` flag, and stop/start other services.

## Features

- Execute commands in other containers
- Start and stop other services
- Run temporary containers with `--rm` flag
- Access and control the Docker daemon from within a container

## Project Structure

```
.
├── Dockerfile        # Docker image definition for controller
├── docker-compose.yml # Docker Compose configuration
└── scripts/          # Directory for optional utility scripts
```

## Configuration Files

### Dockerfile
```dockerfile
FROM alpine:latest

# Install Docker CLI
RUN apk add --no-cache \
    docker-cli \
    docker-compose \
    bash \
    curl \
    jq 

# Create a directory for scripts
WORKDIR /app

# Add your management scripts
COPY scripts/ /app/scripts/
RUN chmod +x /app/scripts/*.sh

COPY docker-compose.yml /app/

CMD ["/bin/bash"]
```

### docker-compose.yml
```yaml
services:
  controller:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
    environment:
      COMPOSE_PROJECT_NAME: $COMPOSE_PROJECT_NAME
      # COMPOSE_FILE: $COMPOSE_FILE
      DOCKER_CONTEXT: default
    depends_on:
      - web
      - db
      - redis
    networks:
      - app-network
    # You can use either command if you want to run a specific script at startup
    # command: ["bash", "/app/scripts/manage-containers.sh"]
    # Or just keep the container running and connect to it for manual execution
    tty: true
    stdin_open: true

  web:
    image: nginx:alpine
    ports:
      - "8080:80"
    networks:
      - app-network

  db:
    image: postgres:13
    environment:
      POSTGRES_PASSWORD: example
      POSTGRES_USER: postgres
      POSTGRES_DB: app_db
    networks:
      - app-network

  redis:
    image: redis:alpine
    networks:
      - app-network

networks:
  app-network:
    driver: bridge
```

## Setup Instructions

1. Create the directory structure:
   ```bash
   mkdir -p docker-controller/scripts
   cd docker-controller
   ```

2. Create the Dockerfile and docker-compose.yml files with the content provided above

3. Start the environment:
   ```bash
   docker compose up -d
   ```

## Usage

### Connect to the controller container:

```bash
docker compose exec controller bash
```

### Controlling other containers (from inside the controller):

To execute commands inside other containers:
```bash
# Using docker command
docker exec web ls -la

# Using docker compose command
docker compose exec web ls -la
```

To start/stop services:
```bash
# Using docker command
docker start redis
docker stop redis

# Using docker compose command
docker compose start redis
docker compose stop redis
```

To run a temporary container:
```bash
# Run a temporary container and remove it after execution
docker run --rm --network app-network alpine echo "Hello"
```

To check status of services:
```bash
# View all containers
docker ps

# View compose services
docker compose ps
```

### Direct commands (from host):

```bash
# Execute command in another container via the controller
docker compose exec controller docker exec web ls -la

# Start a container via the controller
docker compose exec controller docker start redis

# Stop a container via the controller
docker compose exec controller docker stop redis

# Run a temporary container via the controller
docker compose exec controller docker run --rm --network app-network alpine echo "Hello"
```

## How It Works

The controller container is given access to the Docker socket (`/var/run/docker.sock`), which allows it to communicate with the Docker daemon on the host. This enables the controller to manage other containers as if the commands were run directly on the host.

The environment variable `COMPOSE_PROJECT_NAME` is passed from the host to the controller container, which allows Docker Compose commands inside the container to correctly identify the project context.

### Identifying Containers

When working with docker-compose, you can reference containers by their service names. From inside the controller container, you can interact with other containers using:

```bash
# Execute command in web container
docker exec web ls -la

# Check logs of database container
docker logs db
```

For temporary containers with the `--rm` flag, you would use standard docker run commands:

```bash
docker run --rm --network app-network alpine echo "Hello from temp container"
```

### Security Considerations

Giving a container access to the Docker socket effectively gives it root-equivalent permissions on the host. This container should be considered trusted and secure. In production environments, consider implementing additional security measures or using alternative orchestration tools like Kubernetes.

## Networking

All containers are connected to the same Docker network (`app-network`), which allows them to communicate with each other using their service names as hostnames.

## Troubleshooting

### Docker Compose Not Detecting Project

If Docker Compose inside the controller container doesn't detect that the project has been started:

1. **Check Environment Variables**:
   ```bash
   # Inside the controller container
   echo $COMPOSE_PROJECT_NAME
   ```
   
2. **Try Explicit Project Specification**:
   ```bash
   # If you know your project name
   docker compose -p your_project_name ps
   ```

3. **List All Running Containers**:
   ```bash
   docker ps
   ```
   
4. **Verify Docker Compose Version**:
   ```bash
   docker compose version
   ```

### Container Access Issues

If you're unable to control other containers:

1. **Check Docker Socket Mounting**:
   ```bash
   ls -la /var/run/docker.sock
   ```

2. **Verify Network Connectivity**:
   ```bash
   docker network inspect app-network
   ```

3. **Try Using Container IDs Instead of Names**:
   ```bash
   docker ps --format "{{.ID}} {{.Names}}"
   # Then use the ID instead of the name
   docker exec <container_id> ls
   ```

4. **Check if Container Names Are Being Properly Resolved**:
   ```bash
   # From inside the controller container
   ping web
   ping db
   ```
