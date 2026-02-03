# Claw Spawn - Makefile for local development
# Usage: make (builds and starts everything)
#        make help (shows all available commands)

.PHONY: all help setup check-deps db setup-env migrate build run test clean docker-build docker-run

# Default target - runs everything needed to start
all: check-deps setup-env db migrate build run

# Show all available commands
help:
	@echo "Claw Spawn - Available Commands"
	@echo "========================================"
	@echo ""
	@echo "  make          - Full setup and start (default)"
	@echo "  make setup    - Initial environment setup"
	@echo "  make db       - Create database if not exists"
	@echo "  make migrate  - Run database migrations"
	@echo "  make build    - Build release binary"
	@echo "  make run      - Start the server"
	@echo "  make dev      - Quick dev mode (checks, migrates, runs)"
	@echo "  make test     - Run all tests"
	@echo "  make clean    - Clean build artifacts"
	@echo "  make docker-build - Build Docker image"
	@echo "  make docker-run   - Run with Docker Compose"
	@echo ""
	@echo "Environment Variables Required:"
	@echo "  CEDROS_DATABASE_URL      - PostgreSQL connection string"
	@echo "  CEDROS_DIGITALOCEAN_TOKEN  - DigitalOcean API token"
	@echo "  CEDROS_ENCRYPTION_KEY      - 32-byte base64 encoded key"
	@echo ""

# Check all required dependencies
check-deps:
	@echo "🔍 Checking dependencies..."
	@which cargo > /dev/null || (echo "❌ Rust/Cargo not found. Install from https://rustup.rs/" && exit 1)
	@echo "✅ Cargo found"
	@which psql > /dev/null || (echo "❌ PostgreSQL client (psql) not found" && exit 1)
	@echo "✅ PostgreSQL client found"
	@cargo sqlx --version > /dev/null 2>&1 || (echo "⚠️  sqlx-cli not found. Run: cargo install sqlx-cli" && exit 1)
	@echo "✅ sqlx-cli found"
	@echo "✅ All dependencies satisfied!"
	@echo ""

# Initial setup - create .env file if it doesn't exist
setup: check-deps setup-env
	@echo "✅ Setup complete!"
	@echo ""
	@echo "Next steps:"
	@echo "  1. Edit .env with your actual credentials"
	@echo "  2. Run: make db"
	@echo "  3. Run: make migrate"
	@echo "  4. Run: make run"

# Create .env file from example if it doesn't exist
setup-env:
	@if [ ! -f .env ]; then \
		echo "📝 Creating .env file..."; \
		echo "CEDROS_DATABASE_URL=postgres://postgres:postgres@localhost:5432/claw_spawn" > .env; \
		echo "CEDROS_DIGITALOCEAN_TOKEN=your_digitalocean_api_token_here" >> .env; \
		echo "CEDROS_ENCRYPTION_KEY=$$(openssl rand -base64 32)" >> .env; \
		echo "CEDROS_SERVER_HOST=0.0.0.0" >> .env; \
		echo "CEDROS_SERVER_PORT=8080" >> .env; \
		echo "CEDROS_OPENCLAW_IMAGE=ubuntu-22-04-x64" >> .env; \
		echo "✅ Created .env with default values"; \
		echo "⚠️  IMPORTANT: Edit .env and add your DigitalOcean token!"; \
	else \
		echo "✅ .env file already exists"; \
	fi

# Create database if it doesn't exist
db:
	@echo "🗄️  Setting up database..."
	@if [ -z "$$CEDROS_DATABASE_URL" ]; then \
		echo "❌ CEDROS_DATABASE_URL not set. Run: make setup-env"; \
		exit 1; \
	fi
	@DB_NAME=$$(echo "$$CEDROS_DATABASE_URL" | sed -n 's/.*\/\([^?]*\).*/\1/p'); \
	DB_HOST=$$(echo "$$CEDROS_DATABASE_URL" | sed -n 's/.*@\([^:]*\):.*/\1/p'); \
	DB_USER=$$(echo "$$CEDROS_DATABASE_URL" | sed -n 's/.*:\/\/\([^:]*\):.*/\1/p'); \
	psql -h $$DB_HOST -U $$DB_USER -d postgres -tc "SELECT 1 FROM pg_database WHERE datname = '$$DB_NAME'" | grep -q 1 || \
		psql -h $$DB_HOST -U $$DB_USER -d postgres -c "CREATE DATABASE $$DB_NAME;" && \
		echo "✅ Database created: $$DB_NAME" || \
		echo "✅ Database already exists: $$DB_NAME"

# Run database migrations
migrate:
	@echo "🔄 Running migrations..."
	@if [ -z "$$CEDROS_DATABASE_URL" ]; then \
		echo "❌ CEDROS_DATABASE_URL not set. Loading from .env..."; \
		export $$(grep -v '^#' .env | xargs) && cargo sqlx migrate run; \
	else \
		cargo sqlx migrate run; \
	fi
	@echo "✅ Migrations complete!"

# Build release binary
build:
	@echo "🔨 Building release binary..."
	@cargo build --release --bin claw-spawn-server
	@echo "✅ Build complete!"
	@echo "   Binary: target/release/claw-spawn-server"
	@echo ""

# Quick build for development
dev-build:
	@echo "🔨 Building (dev mode)..."
	@cargo build --bin claw-spawn-server

# Start the server
run:
	@echo "🚀 Starting server..."
	@if [ -z "$$CEDROS_DATABASE_URL" ]; then \
		echo "📋 Loading environment from .env..."; \
		export $$(grep -v '^#' .env | xargs) && ./target/release/claw-spawn-server; \
	else \
		./target/release/claw-spawn-server; \
	fi

# Quick dev mode - compile and run with hot reload on file changes
dev: check-deps dev-build
	@echo "🚀 Starting server in dev mode..."
	@echo "   (Use Ctrl+C to stop)"
	@echo ""
	@if [ -z "$$CEDROS_DATABASE_URL" ]; then \
		export $$(grep -v '^#' .env | xargs) && cargo run --bin claw-spawn-server; \
	else \
		cargo run --bin claw-spawn-server; \
	fi

# Run all tests
test:
	@echo "🧪 Running tests..."
	@cargo test
	@echo "✅ Tests complete!"

# Check code without building
check:
	@echo "🔍 Checking code..."
	@cargo check
	@echo "✅ Code check passed!"

# Format code
fmt:
	@echo "✨ Formatting code..."
	@cargo fmt
	@echo "✅ Code formatted!"

# Run linter
lint:
	@echo "🔍 Running clippy..."
	@cargo clippy -- -D warnings
	@echo "✅ Linting complete!"

# Clean build artifacts
clean:
	@echo "🧹 Cleaning build artifacts..."
	@cargo clean
	@rm -rf target/
	@echo "✅ Clean complete!"

# Reset everything (database, builds)
reset: clean
	@echo "⚠️  This will delete the database and all data!"
	@read -p "Are you sure? [y/N] " confirm && [ $$confirm = "y" ] || exit 1
	@if [ -z "$$CEDROS_DATABASE_URL" ]; then export $$(grep -v '^#' .env | xargs); fi; \
	DB_NAME=$$(echo "$$CEDROS_DATABASE_URL" | sed -n 's/.*\/\([^?]*\).*/\1/p'); \
	DB_HOST=$$(echo "$$CEDROS_DATABASE_URL" | sed -n 's/.*@\([^:]*\):.*/\1/p'); \
	DB_USER=$$(echo "$$CEDROS_DATABASE_URL" | sed -n 's/.*:\/\/\([^:]*\):.*/\1/p'); \
	psql -h $$DB_HOST -U $$DB_USER -d postgres -c "DROP DATABASE IF EXISTS $$DB_NAME;" && \
	echo "✅ Database dropped"

# Build Docker image
docker-build:
	@echo "🐳 Building Docker image..."
	@docker build -t claw-spawn:latest .
	@echo "✅ Docker image built: claw-spawn:latest"

# Run with Docker Compose
docker-run:
	@echo "🐳 Starting with Docker Compose..."
	@if [ -f docker-compose.yml ]; then \
		docker-compose up --build; \
	else \
		echo "❌ docker-compose.yml not found"; \
		echo "Create one or use: make run"; \
	fi

# Install sqlx-cli if not present
install-sqlx:
	@echo "📦 Installing sqlx-cli..."
	@cargo install sqlx-cli --no-default-features --features native-tls,postgres
	@echo "✅ sqlx-cli installed!"

# Create a new database migration
migrate-add:
	@read -p "Migration name: " name; \
	cargo sqlx migrate add $$name

# Show database status
migrate-status:
	@cargo sqlx migrate info

# Revert last migration
migrate-revert:
	@cargo sqlx migrate revert

# Quick health check
check-server:
	@echo "🏥 Checking server health..."
	@curl -s http://localhost:8080/health && echo "" || echo "❌ Server not running"

# Generate a new encryption key
generate-key:
	@echo "🔑 New encryption key:"
	@openssl rand -base64 32
	@echo ""
	@echo "Add this to your .env file as CEDROS_ENCRYPTION_KEY"

# Show current environment
env:
	@echo "📋 Current Environment:"
	@echo "======================"
	@echo "CEDROS_DATABASE_URL:      $${CEDROS_DATABASE_URL:-<not set>}"
	@echo "CEDROS_SERVER_HOST:       $${CEDROS_SERVER_HOST:-<not set>}"
	@echo "CEDROS_SERVER_PORT:       $${CEDROS_SERVER_PORT:-<not set>}"
	@echo "CEDROS_OPENCLAW_IMAGE:    $${CEDROS_OPENCLAW_IMAGE:-<not set>}"
	@echo "CEDROS_DIGITALOCEAN_TOKEN: $${CEDROS_DIGITALOCEAN_TOKEN:+<set>}"
	@echo "CEDROS_ENCRYPTION_KEY:     $${CEDROS_ENCRYPTION_KEY:+<set>}"
