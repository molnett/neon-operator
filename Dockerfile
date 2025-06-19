FROM gcr.io/distroless/cc-debian12:nonroot
ARG TARGETARCH=aarch64-unknown-linux-gnu
COPY --chown=nonroot:nonroot ./target/${TARGETARCH}/release/operator /app/operator
EXPOSE 8080
ENTRYPOINT ["/app/operator"]
