# Contributing to Neon Kubernetes Operator
Thank you for your interest in contributing to the Neon Kubernetes Operator! This document provides guidelines and best practices for contributing to the project.

## General Contributing Guidelines

### Before Contributing

1. Open an issue to discuss your proposed changes
2. Ensure all tests pass locally: `make test && make test-e2e`
3. Run linting and formatting: `make fmt && make lint`
4. Add tests for new functionality
5. Update documentation as needed

### Code Style

- Follow existing code patterns and conventions
- Use Go standard error handling patterns
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
- Review the project's architecture documentation in `AGENT.md`

## Error Handling Guidelines

This project follows Go standard error handling patterns to ensure robust operation in Kubernetes environments while maintaining code clarity.

### Go Error Handling Best Practices

#### 1. Always Check Errors
Handle errors explicitly using Go's standard error handling pattern:

```go
func TestClusterConfig(t *testing.T) {
    config, err := parseClusterConfig("test-input")
    if err != nil {
        t.Fatalf("failed to parse config: %v", err)
    }
    assert.Equal(t, "test-cluster", config.Name)
}
```

#### 2. Wrap Errors for Context
Use `fmt.Errorf` with `%w` to wrap errors and provide context:

```go
if err := validateClusterSpec(spec); err != nil {
    return fmt.Errorf("invalid cluster specification: %w", err)
}
```

#### 3. Early Returns
Return errors early to avoid deep nesting:

```go
func processCluster(cluster *v1alpha1.NeonCluster) error {
    if cluster.Metadata.Name == "" {
        return fmt.Errorf("cluster name cannot be empty")
    }
    
    if cluster.Metadata.Namespace == "" {
        return fmt.Errorf("cluster namespace cannot be empty")
    }
    
    // Continue processing...
    return nil
}
```

### When to Use Panic

#### 1. Programming Errors
Use `panic()` only for unrecoverable programming errors:

```go
// If this fails, there's a bug in our initialization logic
if clusterRegistry == nil {
    panic("cluster registry was not initialized")
}
```

#### 2. Test Failures
In test code, you can use helper functions that panic on error for cleaner tests:

```go
func createTestCluster(t *testing.T) *v1alpha1.NeonCluster {
    cluster, err := newTestCluster()
    if err != nil {
        t.Fatalf("failed to create test cluster: %v", err)
    }
    return cluster
}
```

### What to Avoid

#### 1. Ignoring Errors
Never ignore errors without explicit reasoning:

```go
// Bad
result, _ := riskyOperation()

// Good
result, err := riskyOperation()
if err != nil {
    log.Printf("operation failed, using default: %v", err)
    result = defaultValue
}
```

#### 2. Generic Error Messages
Always provide context in error messages:

```go
// Bad
return errors.New("validation failed")

// Good
return fmt.Errorf("cluster validation failed: missing required field 'spec.storage'")
```

### Controller-Specific Patterns

#### Error Propagation in Reconcile Functions
Controller reconcile functions should return `(ctrl.Result, error)` and handle all external operations with proper error propagation.

#### Status Updates
Status update failures should be handled gracefully and not interrupt the main reconciliation loop.
