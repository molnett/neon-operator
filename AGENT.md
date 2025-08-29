# AGENT.md

This file provides guidance when working with code in this repository.

## Project Overview

This is a Kubernetes operator for managing Neon database clusters, written in Go. The operator uses the controller-runtime library and follows the Kubernetes controller pattern with custom resource definitions (CRDs).

## Architecture

The project follows standard Go project structure:

- **`cmd/controller/`**: Main controller binary that runs the operator with HTTP server for health/metrics endpoints
- **`cmd/controlplane/`**: Control plane binary for managing storage components
- **`internal/controller/`**: Core controllers and reconciliation logic
- **`internal/controlplane/`**: Control plane utilities and services
- **`api/`**: API definitions and CRD structs

### Controllers

The operator runs multiple controllers concurrently:
- **Cluster Controller**: Manages NeonCluster resources and overall cluster state
- **Project Controller**: Handles Neon project lifecycle
- **Branch Controller**: Manages database branches within projects

Controllers are located in `internal/controller/` and each implements the controller-runtime reconciler pattern.

## Development Commands

### Building and Running
```bash
# Build Docker image for operator
make docker-build

# Run operator locally against your cluster
make run
```

### Testing
```bash
# Run unit tests
make test

# Run integration tests (requires CRDs installed)
make test-e2e
```

### CRD Management
```bash
# Generate CRDs from Go code
make generate

# Install CRDs into cluster
make install
```

### Code Quality
```bash
# Format code with Go fmt
make fmt
```

## External Dependencies

**Important**: This operator currently requires the main Neon repository to be cloned adjacent to this repository for builds to work. The Neon repo must be built at least once with `make`.

Directory structure should be:
```
parent-directory/
├── neon/             # Main Neon repository
└── neon-operator/    # This repository
```

## Runtime Configuration

The operator exposes HTTP endpoints on port 8080:
- `/health` - Health check endpoint
- `/metrics` - Prometheus metrics
- `/` - Diagnostics information
