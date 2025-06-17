FROM debian:stable-slim
ARG TARGETARCH=aarch64-unknown-linux-gnu
COPY --chown=nonroot:nonroot ./target/${TARGETARCH}/release/operator /app/
EXPOSE 8080
ENTRYPOINT ["/app/operator"]
