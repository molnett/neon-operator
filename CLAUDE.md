# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Kubernetes operator for managing Neon database clusters, written in Rust. The operator uses the kube-rs library and follows the Kubernetes controller pattern with custom resource definitions (CRDs).

## Architecture

The project is structured as a Rust workspace with three main crates:

- **`crates/operator`**: Main binary that runs the operator with HTTP server for health/metrics endpoints
- **`crates/neon_cluster`**: Core library containing controllers and utilities
- **`crates/crdgen`**: Utility for generating Kubernetes CRD YAML from Rust structs

### Controllers

The operator runs three main controllers concurrently:
- **Cluster Controller**: Manages NeonCluster resources and overall cluster state
- **Project Controller**: Handles Neon project lifecycle 
- **Branch Controller**: Manages database branches within projects

Controllers are located in `crates/neon_cluster/src/controllers/` and each has its own state management.

## Development Commands

### Building and Running
```bash
# Build Docker image for operator
just build-base

# Run operator locally against your cluster
just run

# Run with OpenTelemetry tracing
just run-telemetry
```

### Testing
```bash
# Run unit tests
just test-unit

# Run integration tests (requires CRDs installed)
just test-integration

# Run telemetry tests
just test-telemetry
```

### CRD Management
```bash
# Generate CRDs from Rust code
just generate

# Install CRDs into cluster
just install-crd
```

### Code Quality
```bash
# Format code with nightly rustfmt
just fmt
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

Environment variables:
- `RUST_LOG` - Controls logging levels (e.g., `info,kube=debug,controller=debug`)
- `OPENTELEMETRY_ENDPOINT_URL` - OpenTelemetry collector endpoint for tracing

## Error Handling

Follow error handling guidelines in CONTRIBUTING.md. Prefer proper error propagation over unwrap in controller logic. Key principles:

- Use `.unwrap()` only in tests, impossible cases (with comments), or programming bug detection
- Handle Kubernetes API errors, CRD validation, and external requests with proper error types
- Controllers should return `Result<Action<()>, Error>` and use error types from `util/errors.rs`
- Always validate user-provided CRD specifications rather than unwrapping optional fields