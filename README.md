# Neon Kubernetes Operator

A Kubernetes operator for managing [Neon](https://neon.com) database clusters. This operator implements Neon's core architecture with separated compute and storage, enabling you to run Neon's serverless Postgres platform on Kubernetes.

## Project Status

This operator is functional for development and testing environments. It implements Neon's core architecture components and provides basic cluster management capabilities.

### What's Implemented

- **Neon Architectural Components**: Pageservers, Safekeepers, Storage Broker, and Storage Controller
- **Notify Hooks**: Full support for notify-attach hooks, which reconfigured Compute to communicate with a different pageserver
- **Basic Branching**: Create new database branches within projects
- **Persistent Storage**: Configurable storage for pageservers and safekeepers
- **E2E Testing**: End-to-end test suite for validating operator functionality

## Prerequisites

### Required Dependencies

- Rust toolchain (1.70 or later)
- Kubernetes cluster (1.28+)
- kubectl configured for your cluster
- [just](https://github.com/casey/just) command runner
- [Tilt](https://tilt.dev/) for local development (optional)
- Docker for building images

### Production requirements

- NVMe-supported PVCs for Pageservers and Safekeepers
- Postgres instance for Storage Controller

## Development

A single-purpose Kind cluster is recommended for local development.

### Local Development with Tilt

For rapid iteration during development:

```bash
# Start Tilt (rebuilds and redeploys on changes)
tilt up

# View Tilt UI
tilt up --web
```

### Manual Development

```bash
# Install CRDs
just install-crd
```

## Testing

### Unit Tests
```bash
just test-unit
```

### Integration Tests
```bash
# Requires CRDs installed
just test-integration
```

### End-to-End Tests
```bash
# Run full E2E test suite (builds image and tests cluster lifecycle)
just test-e2e

# Run E2E tests with existing image (faster iteration)
just test-e2e-fast

# Cleanup any leftover test clusters
just cleanup-e2e
```

## Building

```bash
# Build for current architecture
just build-base

# Build for x86_64
just build-base-x86
```

## Usage

### Installing the Operator

1. Generate and apply CRDs:
```bash
just install-crd
```

2. Deploy the operator:
```bash
kubectl apply -f yaml/operator/
```

### Creating a Neon Cluster

```yaml
apiVersion: oltp.molnett.org/v1alpha1
kind: NeonCluster
metadata:
  name: my-neon-cluster
spec:
  storage:
    pageserver:
      storageClass: "fast-ssd"
      size: "10Gi"
    safekeeper:
      storageClass: "fast-ssd"
      size: "5Gi"
```

### Creating a Project

```yaml
kind: NeonProject
apiVersion: oltp.molnett.org/v1
metadata:
  name: molnett-project
spec:
  cluster_name: basic-cluster
  id: neon-project
  name: neon-project
  pg_version: "PG17"
```

### Creating a Branch

```yaml
kind: NeonBranch
apiVersion: oltp.molnett.org/v1
metadata:
  name: neon-main
spec:
  name: main
  pg_version: "PG17"
  default_branch: true
  project_id: neon-project
```

## Monitoring

The operator exposes HTTP endpoints on port 8080:
- `/health` - Health check endpoint
- `/metrics` - Prometheus metrics (basic)
- `/` - Diagnostics information

## Contributing

Contributions welcome! Please read the [CONTRIBUTING.md](CONTRIBUTING.md) file for details on how to contribute.

## Architecture

This operator implements Neon's separation of compute and storage:

- **Pageservers**: Handles reads from cache and Object Storage
- **Safekeepers**: Provide consensus and WAL durability
- **Storage Broker**: Coordinates storage operations
- **Storage Controller**: Manages storage cluster state
- **Compute Nodes**: PostgreSQL instances that connect to storage

Each component runs as Kubernetes workloads with persistent storage and service discovery.

## License

Apache License 2.0 - see [LICENSE](LICENSE) file for details.
