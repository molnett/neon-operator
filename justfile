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

# compile for musl (for docker image)
compile features="":
  cargo build --release --features={{features}} --target=x86_64-unknown-linux-gnu -p operator

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
