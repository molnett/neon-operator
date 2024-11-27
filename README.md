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

- Rust toolchain (latest stable)
- Kubernetes cluster (1.28+)
- kubectl
- [kubebuilder](https://book.kubebuilder.io/quick-start.html)

### Neon Repository Setup

This operator requires the main Neon repository to be checked out adjacent to this repository for builds to work:

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

```bash
cargo build
```

## Installation

TBD - Installation instructions will be added as the project matures.

## Usage

TBD - Usage instructions and examples will be added as the project matures.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

TBD
