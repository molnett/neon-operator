FROM gcr.io/distroless/cc-debian12:nonroot
ARG TARGETARCH=aarch64-unknown-linux-gnu
COPY --chown=nonroot:nonroot ./target/${TARGETARCH}/release/operator /app/operator
EXPOSE 8080
ENTRYPOINT ["/app/operator"]

LABEL org.opencontainers.image.title="Neon Operator"
LABEL org.opencontainers.image.description="Kubernetes operator for managing Neon database clusters"
LABEL org.opencontainers.image.source="https://github.com/molnett/neon-operator"
LABEL org.opencontainers.image.vendor="Molnett"
