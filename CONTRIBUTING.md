# Contributing to Neon Kubernetes Operator
Thank you for your interest in contributing to the Neon Kubernetes Operator! This document provides guidelines and best practices for contributing to the project.

## General Contributing Guidelines

### Before Contributing

1. Open an issue to discuss your proposed changes
2. Ensure all tests pass locally: `just test-unit && just test-e2e`
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

#### 1. External API Operations
Always handle external API calls with proper error handling rather than using `.unwrap()`.

#### 2. User Input Validation
User-provided specifications should be validated and return appropriate error messages when invalid.

#### 3. Network Operations
Network calls can fail in various ways and should include timeout handling and retry logic where appropriate.

#### 4. File and I/O Operations
File operations can fail due to permissions, disk space, or other system-level issues.

#### 5. Resource Metadata Access
Kubernetes resource metadata might be missing or malformed and should be validated before use.

### Controller-Specific Patterns

#### Error Propagation in Reconcile Functions
Controller reconcile functions should return `Result<Action<()>, Error>` and handle all external operations with proper error propagation.

#### Status Updates
Status update failures should be handled gracefully and not interrupt the main reconciliation loop.

### Common Error Types

The project provides these error types in `util/errors.rs`:

- `StdError::KubeError` - For Kubernetes API errors
- `StdError::InvalidArgument` - For validation errors
- `StdError::MetadataMissing` - For missing required metadata
- `StdError::HttpError` - For external HTTP request failures
- `StdError::DecodingError` - For parsing/deserialization failures
