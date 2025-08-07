# Neon Kubernetes Operator

A Kubernetes operator for managing self-hosted [Neon](https://neon.com) Postgres database clusters. This operator enables you to manage all necessary compoents of Neon's control plane on Kubernetes (in cloud and on-premises).

*This product isn't affiliated with or endorsed by Neon in any way.*

## Project Status

This operator is functional for development and testing environments. It implements Neon's core architecture components and provides basic cluster management capabilities. We are currently working on Day 1 and Day 2 operations, which means performance is not yet optimized.

### Limitations vs Hosted Neon

This self-hosted operator currently has several limitations compared to the fully managed Neon service:

- **No Compute Auto-scaling**: Compute instances run persistently and do not scale to zero
- **Manual Tenant Sharding**: Tenant sharding must be configured manually or triggered by specific conditions
- **Performance Optimization**: Day 2 operations and performance tuning are still in development
- **Feature Completeness**: Some advanced features available in hosted Neon are not yet implemented

### What's Implemented

- **Neon Architectural Components**: Pageservers, Safekeepers, Storage Broker, and Storage Controller
- **Notify Hooks**: Full support for notify-attach hooks which reconfigures Compute to communicate with a different pageserver
- **Basic Branching**: Create new database branches within projects
- **Persistent Storage**: Configurable storage for pageservers and safekeepers
- **E2E Testing**: End-to-end test suite for validating operator functionality


### Architecture

This operator implements Neon's separation of compute and storage:

- **Pageservers**: Handles reads from cache and Object Storage
- **Safekeepers**: Provide consensus and WAL durability
- **Storage Broker**: Coordinates storage operations
- **Storage Controller**: Manages storage cluster state
- **Compute Nodes**: PostgreSQL instances that connect to storage

Each component runs as Kubernetes workloads with persistent storage and service discovery.

### What's to come

#### Functional refactors
Safekeepers and Pageservers are built to be horizontally scalable. Moving them to a dedicated CRD rather than just a Pod/StatefulSet allows us to not have a dedicated database to manage their state.
- [] Moving Pageservers from Pod to a dedicated CRD #30
- [] Moving Safekeepers from a Statefulset to a dedicated CRD

#### Day 2 operations
- [] Automatically draining Pageservers on retirement or malfunction #21
- [] Cleaning up tenants and timelines when objects are deleted #10

#### Performance
- [] PGBouncer support

## Compatibility

This operator is tested with:
- **Neon Components**: Release 9129 (latest compute always supported)
- **Kubernetes**: 1.28+
- **Storage**: S3-compatible object storage required

## Prerequisites

### Required Dependencies

- Rust toolchain (1.70 or later)
- Kubernetes cluster (1.28+)
- kubectl configured for your cluster
- [just](https://github.com/casey/just) command runner
- [Tilt](https://tilt.dev/) for local development (optional)
- Docker for building images

### Storage Requirements

- **Object Storage**: S3-compatible storage (e.g., AWS S3, Rook/Ceph, MinIO)
- **Persistent Volumes**: NVMe-supported PVCs recommended for optimal performance
  - Works with standard storage but performance will be significantly reduced
  - Requires 512 byte sector size support
- **Database**: PostgreSQL instance for Storage Controller (can use any provider/CNPG)

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

### Deployment Flow

The correct order for creating resources is:

1. **Install Operator**: Deploy the operator and CRDs
2. **Create Cluster**: Deploy a NeonCluster resource and wait for all components to become available
3. **Create Project**: Once the cluster is ready, create NeonProject resources
4. **Create Branches**: Create NeonBranch resources within projects

**Important**: The entire cluster must be available before projects and branches can be created. Monitor cluster status before proceeding with dependent resources.

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

## License

Apache License 2.0 - see [LICENSE](LICENSE) file for details.
