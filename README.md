# claw-spawn

Complete Digital Ocean VPS provisioning and OpenClaw bot orchestration service.

This repo supports:
- Standalone microservice (`claw-spawn-server`)
- Embedding into a larger Axum server via `claw_spawn::server::router(...)`

## 🚀 Super Quick Start (One Command)

Just run `make` and you're ready to go:

```bash
# Clone the repository
git clone https://github.com/conorholds/claw-spawn.git
cd claw-spawn

# Run everything (creates .env, sets up DB, runs migrations, builds, and starts server)
make
```

**That's it!** The server will start on http://localhost:8080

Then edit `.env` and add your DigitalOcean API token:
```bash
# Edit the .env file
CLAW_DIGITALOCEAN_TOKEN=your_actual_token_here
```

## 📋 Makefile Commands

The `Makefile` provides easy commands for development:

| Command | Description |
|---------|-------------|
| `make` | **Full setup and start** (default) |
| `make dev` | Quick dev mode with hot reload |
| `make setup` | Initial environment setup only |
| `make db` | Create database |
| `make migrate` | Run database migrations |
| `make build` | Build release binary |
| `make run` | Start the server |
| `make test` | Run all tests |
| `make clean` | Clean build artifacts |
| `make docker-run` | Run with Docker Compose |
| `make help` | Show all available commands |

## 🔧 Manual Setup (If You Prefer)

### Prerequisites
- Rust/Cargo: https://rustup.rs/
- PostgreSQL: `brew install postgresql` (macOS) or `apt-get install postgresql`
- sqlx-cli: `cargo install sqlx-cli`

### 1. Set Environment Variables

```bash
export CLAW_DATABASE_URL="postgres://user:password@localhost/claw_spawn"
export CLAW_DIGITALOCEAN_TOKEN="your_digitalocean_api_token"
export CLAW_ENCRYPTION_KEY="$(openssl rand -base64 32)"
```

### 2. Setup Database

```bash
# Create database
createdb claw_spawn

# Run migrations
sqlx migrate run
```

### 3. Build and Run

```bash
# Build release binary
cargo build --release --bin claw-spawn-server

# Start server
./target/release/claw-spawn-server
```

## 🐳 Docker Quick Start

```bash
# Start with Docker Compose (includes PostgreSQL)
make docker-run

# Or manually:
docker-compose up --build
```

The Docker setup includes:
- PostgreSQL database (auto-created)
- Automatic migrations on startup
- Server exposed on port 8080

## 📦 Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `CLAW_DATABASE_URL` | Yes | - | PostgreSQL connection string |
| `CLAW_DIGITALOCEAN_TOKEN` | Yes | - | DigitalOcean API token |
| `CLAW_ENCRYPTION_KEY` | Yes | - | Base64-encoded 32-byte key |
| `CLAW_SERVER_HOST` | No | `0.0.0.0` | Server bind address |
| `CLAW_SERVER_PORT` | No | `8080` | Server port |
| `CLAW_OPENCLAW_IMAGE` | No | `ubuntu-22-04-x64` | DO droplet image |
| `CLAW_CONTROL_PLANE_URL` | No | `https://api.example.com` | Bot control-plane base URL |
| `CLAW_CUSTOMIZER_REPO_URL` | No | `https://github.com/janebot2026/janebot-cli.git` | Public git repo for workspace customizer |
| `CLAW_CUSTOMIZER_REF` | No | pinned SHA | Git ref (tag/branch/SHA) to checkout for reproducible bootstrap |
| `CLAW_CUSTOMIZER_WORKSPACE_DIR` | No | `/opt/openclaw/workspace` | Workspace directory on droplet |
| `CLAW_CUSTOMIZER_AGENT_NAME` | No | `Jane` | Agent name passed to customizer |
| `CLAW_CUSTOMIZER_OWNER_NAME` | No | `Cedros` | Owner name passed to customizer |
| `CLAW_CUSTOMIZER_SKIP_QMD` | No | `true` | Skip QMD install at droplet bootstrap |
| `CLAW_CUSTOMIZER_SKIP_CRON` | No | `true` | Skip OpenClaw cron install at droplet bootstrap |
| `CLAW_CUSTOMIZER_SKIP_GIT` | No | `true` | Skip git init at droplet bootstrap |
| `CLAW_CUSTOMIZER_SKIP_HEARTBEAT` | No | `true` | Skip heartbeat install at droplet bootstrap |

## 🪂 Droplet Bootstrap Notes

- The droplet must be able to reach `CLAW_CONTROL_PLANE_URL` over HTTPS.
  If you run `claw-spawn` locally, you typically need a tunnel (ngrok/cloudflared) until you have a live URL.
- Workspace customization (janebot-cli) runs once and writes:
  - Marker: `/opt/openclaw/.customizer_ran`
  - Status: `/opt/openclaw/customizer_status.txt`

## 🧩 Embedded Usage (Integrate Into Larger Axum Server)

`claw-spawn` supports a dual architecture:
- Standalone microservice via the `claw-spawn-server` binary
- Embedded router via `claw_spawn::server::router(...)`

Example (host app nests this service under `/spawn`):

```rust,ignore
use axum::Router;
use claw_spawn::infrastructure::AppConfig;
use claw_spawn::server::{build_state_with_pool, router};
use sqlx::PgPool;

let cfg = AppConfig::from_env()?;
let pool = PgPool::connect(&cfg.database_url).await?;
let state = build_state_with_pool(cfg, pool, /* run_migrations */ true).await?;

let app = Router::new().nest("/spawn", router(state));
```

## 📦 Crate Usage

Add to `Cargo.toml`:

```toml
[dependencies]
# Core library only (no Axum server/router)
claw-spawn = { version = "0.1", default-features = false }
```

Or, if you want the embeddable HTTP server/router:

```toml
[dependencies]
claw-spawn = { version = "0.1", features = ["server"] }
```

## 🎯 API Usage Examples

### Create a Bot

```bash
curl -X POST http://localhost:8080/bots \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "123e4567-e89b-12d3-a456-426614174000",
    "name": "My First Bot",
    "persona": "beginner",
    "asset_focus": "majors",
    "algorithm": "trend",
    "strictness": "medium",
    "paper_mode": true,
    "max_position_size_pct": 10.0,
    "max_daily_loss_pct": 5.0,
    "max_drawdown_pct": 15.0,
    "max_trades_per_day": 20,
    "llm_provider": "openai",
    "llm_api_key": "sk-your-openai-key-here"
  }'
```

Response:
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "account_id": "123e4567-e89b-12d3-a456-426614174000",
  "name": "My First Bot",
  "persona": "beginner",
  "status": "provisioning",
  "droplet_id": 123456789,
  "created_at": "2024-01-15T10:30:00Z"
}
```

### Check Bot Status

```bash
curl http://localhost:8080/bots/{bot_id}
```

### Bot Actions

```bash
# Pause
curl -X POST http://localhost:8080/bots/{bot_id}/actions -d '{"action": "pause"}'

# Resume  
curl -X POST http://localhost:8080/bots/{bot_id}/actions -d '{"action": "resume"}'

# Destroy
curl -X POST http://localhost:8080/bots/{bot_id}/actions -d '{"action": "destroy"}'
```

## 📚 API Endpoints

### App Endpoints
- `POST /bots` - Create bot
- `GET /bots/:id` - Get bot details
- `GET /accounts/:id/bots` - List account bots
- `POST /bots/:id/actions` - pause/resume/redeploy/destroy

### Bot Agent Endpoints
- `GET /bot/:id/config` - Pull config
- `POST /bot/:id/config_ack` - Acknowledge config
- `POST /bot/:id/heartbeat` - Health check
- `POST /bot/register` - Initial registration

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       claw-spawn                            │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Axum HTTP Server                          │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐│  │
│  │  │  App API     │  │  Bot API     │  │  Health      ││  │
│  │  │  (/bots/*)   │  │  (/bot/*)    │  │  (/health)   ││  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘│  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Application Layer                       │  │
│  │  ┌──────────────┐  ┌──────────────┐                  │  │
│  │  │Provisioning │  │   BotLifecycle              │  │
│  │  │   Service    │  │   Service                     │  │
│  │  └──────────────┘  └──────────────┘                  │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              Infrastructure Layer                    │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌────────────┐│  │
│  │  │DigitalOcean │  │PostgreSQL    │  │  Crypto    ││  │
│  │  │   Client     │  │Repositories  │  │(AES-256)   ││  │
│  │  └──────────────┘  └──────────────┘  └────────────┘│  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
                  ┌───────────────────────┐
                  │   DigitalOcean VPS    │
                  │  ┌─────────────────┐  │
                  │  │  OpenClaw Bot   │  │
                  │  │   Agent         │  │
                  │  └─────────────────┘  │
                  └───────────────────────┘
```

## 🔐 Security

- **AES-256-GCM encryption** for all secrets (LLM API keys)
- **Per-bot registration tokens** for authentication
- **Firewall rules** on droplets (default deny inbound)
- **No secrets in logs** - all sensitive data redacted

## 📖 Documentation

- **Setup**: See [Super Quick Start](#-super-quick-start-one-command) and [Manual Setup](#-manual-setup-if-you-prefer)
- **API Reference**: See [API Usage Examples](#api-usage-examples)
- **Architecture**: See [Architecture](#architecture) section

## 🛠️ Development

```bash
# Quick dev cycle
make dev

# Run tests
make test

# Check code
make check

# Format code
make fmt

# Lint
make lint
```

## 📝 Database Migrations

```bash
# Create migration
make migrate-add

# Run migrations
make migrate

# Check status
make migrate-status

# Revert last
make migrate-revert
```

## 🧹 Troubleshooting

### "sqlx-cli not found"
```bash
make install-sqlx
```

### "Database connection failed"
```bash
# Check PostgreSQL is running
make db
```

### "Port 8080 already in use"
```bash
# Edit .env and change CLAW_SERVER_PORT
CLAW_SERVER_PORT=8081 make run
```

## 🤝 Library Usage

Use as a library in your Rust project:

```rust
use claw_spawn::{
    application::{ProvisioningService, BotLifecycleService},
    domain::{Account, BotConfig, Persona, SubscriptionTier},
    infrastructure::{AppConfig, DigitalOceanClient, PostgresAccountRepository},
};

// See README.md for full example
```

## 📄 License

MIT
