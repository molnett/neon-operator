[private]
default:
  @just --list --unsorted

# install crd into the cluster
install-crd: generate
  kubectl apply -f yaml/crd.yaml

generate:
  cargo run -p crdgen > yaml/crd.yaml

# run with opentelemetry
run-telemetry:
  OPENTELEMETRY_ENDPOINT_URL=http://127.0.0.1:55680 RUST_LOG=info,kube=trace,controller=debug cargo run -o operator --features=telemetry

# run without opentelemetry
run:
  RUST_LOG=info,kube=debug,controller=debug cargo run -p operator

# format with nightly rustfmt
fmt:
  cargo +nightly fmt

# run unit tests
test-unit:
  cargo test
# run integration tests
test-integration: install-crd
  cargo test -- --ignored
# run telemetry tests
test-telemetry:
  OPENTELEMETRY_ENDPOINT_URL=http://127.0.0.1:55680 cargo test --lib --all-features -- get_trace_id_returns_valid_traces --ignored

# compile for linux arm64 (for docker image) using zigbuild cross-compilation
compile features="":
  cargo zigbuild --release --features={{features}} -p operator

[private]
_build features="":
  just compile {{features}}
  docker build -t molnett/neon-operator:local .

# docker build base
build-base: (_build "")
# docker build with telemetry
build-otel: (_build "telemetry")


# local helper for test-telemetry and run-telemetry
# forward grpc otel port from svc/promstack-tempo in monitoring
forward-tempo:
  kubectl port-forward -n monitoring svc/promstack-tempo 55680:4317

# Build operator image for E2E testing
build-e2e-image:
  just build-base

# Run E2E tests (requires operator image)
test-e2e: build-e2e-image
  cargo test --package e2e_tests --test basic_e2e -- --test-threads=1 --nocapture

# Run E2E tests with existing image (faster iteration)
test-e2e-fast:
  cargo test --package e2e_tests --test basic_e2e -- --test-threads=1 --nocapture

# Run E2E tests with verbose output
test-e2e-verbose: build-e2e-image
  RUST_LOG=debug cargo test --package e2e_tests --test basic_e2e -- --test-threads=1 --nocapture

# Clean up any leftover e2e test clusters
cleanup-e2e:
  ./cleanup-e2e-clusters.sh

# Clean up using cargo test (alternative method)
cleanup-e2e-rust:
  cargo test --package e2e_tests --test basic_e2e cleanup_test_clusters -- --ignored --nocapture
