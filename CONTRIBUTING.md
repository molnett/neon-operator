# Contributing to Neon Kubernetes Operator

Thank you for your interest in contributing to the Neon Kubernetes Operator! This document provides guidelines and best practices for contributing to the project.

## Error Handling Guidelines

This project follows specific error handling guidelines to ensure robust operation in Kubernetes environments while maintaining code clarity.

### When `.unwrap()` Is Acceptable ✅

#### 1. Test Code
Use `.unwrap()` freely in test code for cleaner, more readable tests:

```rust
#[test]
fn test_cluster_config() {
    let config = parse_cluster_config("test-input").unwrap();
    assert_eq!(config.name, "test-cluster");
}

#[test]
fn test_neon_cluster_creation() {
    let cluster = create_test_cluster().unwrap();
    assert!(cluster.metadata.name.is_some());
}
```

#### 2. Impossible Cases with Clear Comments
When you can prove a case is impossible, document it clearly:

```rust
// This unwrap is safe because we just checked the value exists above
let namespace = cluster.metadata.namespace.as_ref().unwrap();

// Parser guarantees this field exists for valid configs
let endpoint = config.database_url.unwrap(); // SAFETY: validated by parser
```

#### 3. Programming Bug Detection
Use `.unwrap()` to detect programming errors that should crash the program:

```rust
// If this fails, there's a bug in our cluster initialization logic
let cluster_id = CLUSTER_REGISTRY.get(&name).unwrap();
```

#### 4. Resource Allocation in Non-Critical Paths
For allocations that should never fail in practice:

```rust
let mut buffer = Vec::with_capacity(1024);
buffer.try_reserve(additional_capacity).unwrap(); // OOM should crash
```

### When to Avoid `.unwrap()` ❌

#### 1. Kubernetes API Interactions
Always handle Kubernetes API errors properly:

```rust
// ❌ Bad - can crash the controller
let pods = client.list::<Pod>(&params).await.unwrap();

// ✅ Good - proper error handling
let pods = client.list::<Pod>(&params).await
    .map_err(|e| StdError::KubeError(e))?;
```

#### 2. CRD Spec Processing
User-provided specifications can be invalid:

```rust
// ❌ Bad - user input can be malformed
let database_name = cluster.spec.database.name.unwrap();

// ✅ Good - validate user input
let database_name = cluster.spec.database.name
    .ok_or_else(|| StdError::InvalidArgument("database.name is required".to_string()))?;
```

#### 3. External HTTP Requests
Network operations can fail in many ways:

```rust
// ❌ Bad - network calls can fail
let response = http_client.get(url).await.unwrap();

// ✅ Good - handle network errors
let response = http_client.get(url).await
    .map_err(|e| StdError::HttpError(format!("Failed to fetch {}: {}", url, e)))?;
```

#### 4. File I/O Operations
File operations can fail due to permissions, disk space, etc.:

```rust
// ❌ Bad - file operations can fail
let config = fs::read_to_string(path).unwrap();

// ✅ Good - handle I/O errors
let config = fs::read_to_string(path)
    .map_err(|e| StdError::DecodingError(format!("Failed to read config: {}", e)))?;
```

#### 5. Metadata Access in Controllers
Kubernetes metadata can be missing or malformed:

```rust
// ❌ Bad - metadata might not exist
let namespace = cluster.metadata.namespace.unwrap();

// ✅ Good - handle missing metadata
let namespace = cluster.metadata.namespace
    .ok_or_else(|| StdError::MetadataMissing("namespace is required".to_string()))?;
```

### Controller-Specific Patterns

#### Error Propagation in Reconcile Functions
Controller reconcile functions should return `Result<Action<()>, Error>`:

```rust
async fn reconcile_cluster(cluster: Arc<NeonCluster>, ctx: Arc<Context>) -> Result<Action<()>, Error> {
    let name = cluster.metadata.name
        .ok_or_else(|| StdError::MetadataMissing("cluster name is required".to_string()))?;
    
    let namespace = cluster.metadata.namespace
        .ok_or_else(|| StdError::MetadataMissing("cluster namespace is required".to_string()))?;
    
    // Use proper error handling for all Kubernetes operations
    let existing_pods = ctx.client
        .list::<Pod>(&ListParams::default().labels(&format!("cluster={}", name)))
        .await
        .map_err(StdError::KubeError)?;
    
    // Return appropriate requeue behavior
    Ok(Action::requeue(Duration::from_secs(30)))
}
```

#### Status Updates
Always handle status update failures gracefully:

```rust
// ✅ Good - handle status update errors
if let Err(e) = update_cluster_status(&client, &cluster, status).await {
    warn!("Failed to update cluster status: {}", e);
    // Continue processing - status updates shouldn't break reconciliation
}
```

### Common Error Types

The project provides these error types in `util/errors.rs`:

- `StdError::KubeError` - For Kubernetes API errors
- `StdError::InvalidArgument` - For validation errors
- `StdError::MetadataMissing` - For missing required metadata
- `StdError::HttpError` - For external HTTP request failures
- `StdError::DecodingError` - For parsing/deserialization failures

### Migration Strategy

When converting existing `.unwrap()` calls:

1. **Identify the error type** - What can actually go wrong?
2. **Choose appropriate error handling** - Should it be logged, returned, or crash?
3. **Add context** - Include relevant information in error messages
4. **Test failure cases** - Ensure error paths work correctly

### Automated Enforcement

The project uses Clippy to warn about `.unwrap()` usage:

```toml
[workspace.lints.clippy]
unwrap_used = { level = "warn", priority = 1 }
```

This helps catch problematic unwrap usage during development while allowing them in test code where appropriate.

## General Contributing Guidelines

### Before Contributing

1. Open an issue to discuss your proposed changes
2. Ensure all tests pass locally: `just test-unit && just test-integration`
3. Run linting and formatting: `just fmt`
4. Add tests for new functionality
5. Update documentation as needed

### Code Style

- Follow existing code patterns and conventions
- Use the project's error types from `util/errors.rs`
- Add comprehensive error handling for all external interactions
- Write clear, descriptive error messages
- Include unit tests for error conditions

### Testing

- Add unit tests for new functionality
- Include error case testing
- Run the full test suite before submitting PRs
- Test against a real Kubernetes cluster when possible

## Getting Help

- Open an issue for questions about contributing
- Check existing issues and PRs for similar work
- Review the project's architecture documentation in `CLAUDE.md`