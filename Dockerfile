# Build stage
FROM rust:1.83-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev

# Create a new empty project
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy source files to cache dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs

# Build dependencies only (this will be cached)
RUN cargo build --release && \
    rm -rf src

# Copy actual source code
COPY src ./src

# Build the application
# Touch main.rs and lib.rs to force rebuild of our code only
RUN touch src/main.rs src/lib.rs && \
    cargo build --release

# Runtime stage - distroless
FROM gcr.io/distroless/static-debian12:nonroot

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/alle /app/alle

# Expose WebSocket port
EXPOSE 8080

# Default environment variables
ENV POSTGRES_URL="postgresql://postgres:postgres@postgres:5432/postgres" \
    WS_ADDR="0.0.0.0:8080" \
    LOG_LEVEL="alle=info,info"

# Distroless runs as nonroot user (65532) by default
USER nonroot:nonroot

# Run the binary
ENTRYPOINT ["/app/alle"]
CMD ["serve"]
