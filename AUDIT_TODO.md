# Audit TODO

This checklist tracks every issue identified in the end-to-end audit. Each item is fixed in an isolated commit (one commit per ID).

## Critical

- [x] F-001 Stop leaking registration tokens via shell tracing
  - Files: `src/application/provisioning.rs`, `scripts/openclaw-bootstrap.sh`
  - Planned fix:
    - Remove `set -x` from droplet user-data prelude and embedded bootstrap script
    - Ensure no token-bearing commands are traced to logs
  - Test plan:
    - `cargo test`
    - Verify generated user-data no longer contains `set -x`
  - Completed:
    - Removed xtrace (`set -x`) from user-data and bootstrap script; added `f001_user_data_does_not_enable_xtrace`; verified via `cargo test`

- [x] F-002 Fix advisory-lock key generation for atomic config versioning
  - Files: `migrations/*` (new migration)
  - Planned fix:
    - Replace lock key derivation with a safe bigint (e.g., `hashtextextended(p_bot_id::text, 0)`)
    - Keep behavior: per-bot serialized version increments via `pg_advisory_xact_lock`
  - Test plan:
    - `cargo test`
    - (Manual) Run `SELECT get_next_config_version_atomic('<uuid>'::uuid);` twice and confirm increments
  - Completed:
    - Added `migrations/005_fix_config_version_lock.sql` using `hashtextextended` for the advisory lock key; verified via `cargo test`

## High

- [x] F-003 Require Bearer token auth for all bot-agent endpoints
  - Files: `src/main.rs`
  - Planned fix:
    - Require `Authorization: Bearer <token>` for `/bot/{id}/config`, `/bot/{id}/config_ack`, `/bot/{id}/heartbeat`
    - Validate via `state.lifecycle.get_bot_with_token(bot_id, token)`
  - Test plan:
    - `cargo test`
    - Add unit tests for header parsing helper
  - Completed:
    - Enforced Bearer auth on bot-agent endpoints using `extract_bearer_token`; added unit tests in `src/main.rs`; verified via `cargo test`

- [x] F-004 Fix retry attempt accounting in provisioning backoff helper
  - Files: `src/application/provisioning.rs`
  - Planned fix:
    - Ensure retry helper runs exactly `RETRY_ATTEMPTS` times
    - Only sleeps between attempts (not after last)
  - Test plan:
    - Add a unit test that counts closure invocations
    - `cargo test`
  - Completed:
    - Made retries run exactly `RETRY_ATTEMPTS` with sleeps only between attempts; added `f004_retry_with_backoff_uses_exact_attempt_count`; verified via `cargo test`

- [x] F-005 Reject invalid enum-like inputs instead of silently defaulting
  - Files: `src/main.rs`
  - Planned fix:
    - Return 400 for unknown `tier`, `persona`, `asset_focus`, `algorithm`, `strictness`
    - Include allowed values in error response
  - Test plan:
    - `cargo test`
    - (Manual) curl invalid payloads -> 400
  - Completed:
    - Added strict parsers and 400 responses for invalid inputs; added `f005_parse_invalid_inputs_return_none`; verified via `cargo test`

## Medium

- [ ] F-006 Make repo pass `cargo fmt --check`
  - Files: `src/**/*.rs`
  - Planned fix:
    - Run `cargo fmt` (format-only changes)
  - Test plan:
    - `cargo fmt --check`

- [x] F-007 Add DB index for account bot pagination ordering
  - Files: `migrations/*` (new migration)
  - Planned fix:
    - Add index on `bots(account_id, created_at DESC)`
  - Test plan:
    - `cargo test`
    - (Manual) `EXPLAIN ANALYZE` list query
  - Completed:
    - Added `migrations/006_idx_bots_account_created_at.sql`; verified via `cargo test`

- [x] F-008 Add DB index for stale heartbeat scan
  - Files: `migrations/*` (new migration)
  - Planned fix:
    - Add index on `bots(status, last_heartbeat_at)`
  - Test plan:
    - `cargo test`
    - (Manual) `EXPLAIN ANALYZE` stale scan
  - Completed:
    - Added `migrations/007_idx_bots_status_heartbeat.sql`; verified via `cargo test`

- [x] F-009 Add DB index for latest config-by-version query
  - Files: `migrations/*` (new migration)
  - Planned fix:
    - Add index on `bot_configs(bot_id, version DESC)`
  - Test plan:
    - `cargo test`
    - (Manual) `EXPLAIN ANALYZE` latest config query
  - Completed:
    - No new index needed: `migrations/001_init.sql` defines `UNIQUE(bot_id, version)`, which creates a btree index usable for `ORDER BY version DESC LIMIT 1` via backward index scan; verified via schema review

- [x] F-010 Remove stored DigitalOcean API token from client struct
  - Files: `src/infrastructure/digital_ocean.rs`
  - Planned fix:
    - Remove `api_token` field (avoid accidental logging)
  - Test plan:
    - `cargo test`
  - Completed:
    - Removed stored token field from `DigitalOceanClient`; verified via `cargo test`

- [x] F-011 Remove/ignore assistant tool metadata from runtime repo
  - Files: `.claude/**`, `.gemini/**`, `.opencode/**`, `.mcp.json`, `.gitignore`
  - Planned fix:
    - Stop tracking tool config directories/files; add ignore rules
  - Test plan:
    - `git status` clean; `cargo test`
  - Completed:
    - Removed tracked `.claude/`, `.gemini/`, `.opencode/`, `.mcp.json`; added ignore rules; verified via `cargo test`

- [x] F-012 Commit `Cargo.lock` for reproducible builds
  - Files: `.gitignore`, `Cargo.lock`
  - Planned fix:
    - Remove `Cargo.lock` from `.gitignore` and commit it
  - Test plan:
    - `cargo build`
  - Completed:
    - Un-ignored and committed `Cargo.lock`; verified via `cargo build`

- [x] F-013 Harden bootstrap config fetch: require 200 + valid JSON before overwriting
  - Files: `scripts/openclaw-bootstrap.sh`
  - Planned fix:
    - Check HTTP status code; only write `config.json` when response is 200 and `jq` parses
  - Test plan:
    - (Manual) simulate non-200; ensure config not overwritten
  - Completed:
    - Updated `fetch_config` to require HTTP 200 and valid JSON before overwriting config; verified via `cargo test`

## Low

- [x] F-014 Remove or wire `openclaw_bootstrap_url` config (currently unused)
  - Files: `src/infrastructure/config.rs`, (maybe) `src/application/provisioning.rs`
  - Planned fix:
    - Prefer removal unless there is an intended use; keep config surface minimal
  - Test plan:
    - `cargo test`
  - Completed:
    - Removed unused `openclaw_bootstrap_url` from `AppConfig` and `.env.example`; verified via `cargo test`

- [ ] F-015 Add minimal CI (fmt/clippy/test)
  - Files: `.github/workflows/ci.yml` (new)
  - Planned fix:
    - Add GitHub Actions workflow running `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
  - Test plan:
    - Workflow runs green on PR
