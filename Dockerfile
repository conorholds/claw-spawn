# Build stage
FROM rust:1.75 as builder

WORKDIR /app

# Copy Cargo files for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY migrations ./migrations
COPY scripts ./scripts

# Build release binary
RUN cargo build --release --bin claw-spawn-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    curl \
    libssl3 \
    postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash cedros

# Copy binary from builder
COPY --from=builder /app/target/release/claw-spawn-server /usr/local/bin/
COPY --from=builder /app/migrations /app/migrations

# Set ownership
RUN chown -R cedros:cedros /app

# Switch to non-root user
USER cedros

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run migrations and start server
CMD ["sh", "-c", "cd /app && claw-spawn-server"]
