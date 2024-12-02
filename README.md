# Neon Kubernetes Operator

A Kubernetes operator for managing [Neon](https://neon.tech) database clusters. This project is currently in early development and is not ready for production use.

## Project Status

This is an early-stage project that provides basic functionality for running Neon clusters in Kubernetes. While functional for testing and development, it is not yet production-ready.

### What Works

- Basic Neon cluster deployment
- Custom Resource Definitions (CRDs) for Neon clusters
- Basic reconciliation of cluster state

### What's Coming / Not Implemented

- High availability configurations
- Advanced monitoring and metrics
- Production-grade security features
- Automatic backup and restore
- Rolling updates
- Multi-tenant support

## Prerequisites

### Required Dependencies

- Rust toolchain (1.70 or later)
- Kubernetes cluster (1.28+)
- kubectl
- [kubebuilder](https://book.kubebuilder.io/quick-start.html)
- [just](https://github.com/casey/just) command runner

### Neon Repository Setup

This operator currently requires the main Neon repository to be checked out adjacent to this repository for builds to work. We plan to remove this dependency in future versions.

1. Clone the Neon repository next to this operator repository:

   ```bash
   git clone https://github.com/neondatabase/neon.git
   ```

2. Follow the setup instructions in the [Neon repository](https://github.com/neondatabase/neon) to install dependencies
3. Build Neon once using make:

   ```bash
   cd neon
   make
   ```

Your directory structure should look like:

```
parent-directory/
├── neon/             # Main Neon repository
└── neon-operator/    # This repository
```

## Building

To build the operator:

```bash
just build-base
```

## Local Development

To run the operator locally against your Kubernetes cluster:

```bash
just run
```

## Usage

### Installing the Operator

1. Apply the CRDs to your cluster:

   ```bash
   kubectl apply -f yaml/crds/
   ```

2. Deploy the operator:

   ```bash
   kubectl apply -f yaml/operator/
   ```

### Creating a Neon Cluster

Example manifest (basic configuration):

```yaml
apiVersion: neon.tech/v1alpha1
kind: NeonCluster
metadata:
  name: example-cluster
spec:
  # Configuration details TBD
```

Detailed configuration options and examples will be added as the project matures.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. Before contributing:

1. Open an issue to discuss your proposed changes
2. Ensure all tests pass locally
3. Add tests for new functionality
4. Update documentation as needed

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.
