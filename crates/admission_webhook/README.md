# NeonPageserver Admission Webhook

This admission webhook validates NeonPageserver resources to ensure:

1. **ID Uniqueness**: The `id` field must be unique per cluster across all namespaces
2. **Immutability**: The `id`, `cluster`, and `storage_config` fields cannot be changed after creation

## Architecture

The webhook runs as a ValidatingAdmissionWebhook that intercepts CREATE and UPDATE operations on NeonPageserver resources. It validates the resources before they are persisted to etcd.

## Validation Rules

### CREATE Operations
- Validates that the `id` is unique within the specified `cluster` across all namespaces
- If another NeonPageserver exists with the same `id` and `cluster`, the request is denied

### UPDATE Operations  
- Validates that `id`, `cluster`, and `storage_config` fields are immutable
- Any attempt to change these fields will result in a denied request

## Certificate Management

The webhook uses cert-manager for automatic TLS certificate management:

- `Issuer`: Creates a self-signed certificate authority
- `Certificate`: Generates TLS certificates for the webhook service
- `ValidatingWebhookConfiguration`: Automatically injected with the CA bundle via cert-manager

## Deployment

### Prerequisites
- cert-manager must be installed in the cluster
- RBAC permissions to list NeonPageserver resources across all namespaces

### Install
```bash
# Build and install the admission webhook
just install-admission-webhook
```

### Uninstall  
```bash
# Remove the admission webhook
just uninstall-admission-webhook
```

## Configuration

The webhook deployment includes:

- **Service Account**: `neon-admission-webhook` 
- **ClusterRole**: Permissions to list NeonPageserver resources
- **Service**: Exposes webhook on port 443 (HTTPS) and 8080 (health)
- **Deployment**: Runs the webhook container with TLS certificates mounted

## Health Checks

The webhook exposes a health endpoint at `/health` on port 8080 for liveness and readiness probes.

## Logging

Configure logging level via the `RUST_LOG` environment variable:
```bash
RUST_LOG=info,admission_webhook=debug
```

## Development

Build the webhook locally:
```bash
cargo build -p admission_webhook
```

Build Docker image:
```bash
just build-admission-webhook
```
