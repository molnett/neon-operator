FROM debian:stable-slim
COPY --chown=nonroot:nonroot ./target/aarch64-unknown-linux-gnu/release/operator /app/
EXPOSE 8080
ENTRYPOINT ["/app/operator"]
