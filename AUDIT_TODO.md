# Audit Remediation Checklist

Legend: `[ ]` pending, `[x]` completed

## Critical

- [x] **F-001 Enforce authentication on privileged control-plane routes**
  - Files: `src/server/http.rs`, `src/server/state.rs`, `src/infrastructure/config.rs`, `.env.example`, `README.md`, `tests/integration_tests.rs`
  - Planned fix:
    - Add explicit admin bearer-token config and wire it into app state.
    - Require admin bearer auth on privileged routes (`/accounts`, `/bots`, `/accounts/:id/bots`, `/bots/:id`, `/bots/:id/config`, `/bots/:id/actions`).
    - Keep bot registration-token auth only on `/bot/*` agent routes.
  - Test plan:
    - Add unit tests for admin bearer extraction/validation.
    - Run `cargo test`, `cargo check`.
  - Completion note: Added `CLAW_API_BEARER_TOKEN` config/state wiring; privileged `/accounts` + `/bots` routes now require matching bearer token. Verified with `server::http::tests::is_admin_authorized_requires_exact_bearer_match`, `cargo check`, and full `cargo test`.

- [x] **F-002 Fix systemd environment interpolation in bootstrap script**
  - Files: `scripts/openclaw-bootstrap.sh`
  - Planned fix:
    - Change service-file generation so env vars are rendered as concrete values, not literal `${...}` placeholders.
    - Keep sensitive-token handling compatible with existing runner behavior.
  - Test plan:
    - Add script-level assertion test in Rust provisioning tests that generated script does not contain literal placeholders in systemd env lines.
    - Run `cargo test`.
  - Completion note: Switched systemd service heredoc to unquoted delimiter so bootstrap env vars are rendered into unit `Environment=` lines. Verified via `application::provisioning::tests::f002_user_data_exports_customizer_and_toolchain_values` and full `cargo test`.

## High

- [x] **F-003 Prevent partial DB state during bot creation failures**
  - Files: `src/infrastructure/repository.rs`, `src/application/provisioning.rs`, `tests/integration_tests.rs`, `src/application/provisioning.rs` (tests)
  - Planned fix:
    - Add rollback-capable hard delete path in repository.
    - On create flow failure, perform rollback cleanup to avoid orphaned bot/config rows.
    - Ensure bot counter decrement still occurs and rollback errors are logged with context.
  - Test plan:
    - Add/extend service tests to simulate failures after bot row creation and verify rollback path is invoked.
    - Run `cargo test`.
  - Completion note: Added `BotRepository::hard_delete` rollback path and invoked it on `create_bot` failure before counter decrement. Added failure-injection test `application::provisioning::tests::f005_create_bot_rolls_back_partial_state_when_config_create_fails`. Verified with full `cargo test`.

- [x] **F-004 Fix Docker healthcheck dependency mismatch**
  - Files: `Dockerfile`
  - Planned fix:
    - Install `curl` in runtime image used by Docker `HEALTHCHECK`.
  - Test plan:
    - Run `docker build` (or syntax/build validation) and ensure Dockerfile remains valid.
    - Run `cargo check` for regression safety.
  - Completion note: Added `curl` to runtime apt packages so Docker `HEALTHCHECK` command has its required binary. Verified with `cargo check`; attempted `docker build -t claw-spawn:audit-f004 .` but local Docker session was blocked (`only one connection allowed`).

- [x] **F-005 Stop storing registration tokens in plaintext**
  - Files: `src/infrastructure/repository.rs`, `Cargo.toml`, `Cargo.lock`, `tests/integration_tests.rs` (if needed)
  - Planned fix:
    - Hash registration tokens before persistence.
    - Support both hashed and legacy plaintext rows during lookup for safe migration.
    - Keep public API behavior unchanged for callers.
  - Test plan:
    - Add unit test for token hashing/lookup behavior in repository layer.
    - Run `cargo test`.
  - Completion note: Added SHA-256 token hashing at write time (`sha256:<hex>` format) and backward-compatible lookup (`plaintext OR hash`) for legacy rows. Added `infrastructure::repository::tests::hash_registration_token_is_stable_and_prefixed`; verified with full `cargo test`.

## Medium

- [x] **F-006 Make bot-name truncation UTF-8 safe**
  - Files: `src/application/provisioning.rs`
  - Planned fix:
    - Replace byte-slice truncation with char-based truncation.
  - Test plan:
    - Add unit test with multibyte input > max length; ensure no panic and bounded length.
    - Run `cargo test`.
  - Completion note: Replaced byte slicing with char-based truncation in `sanitize_bot_name`, preventing UTF-8 boundary panics. Added `application::provisioning::tests::f006_sanitize_bot_name_truncates_multibyte_input_safely`; verified with full `cargo test`.

- [ ] **F-007 Prefer public IPv4 when parsing droplet IP**
  - Files: `src/domain/droplet.rs`
  - Planned fix:
    - Select first `networks.v4` entry where `type_ == "public"`, fallback to `None`.
  - Test plan:
    - Add unit tests for mixed private/public ordering and missing public network.
    - Run `cargo test`.
  - Completion note: _pending_

- [ ] **F-008 Return precise HTTP status codes for bot actions**
  - Files: `src/server/http.rs`
  - Planned fix:
    - Map known domain failures to 400/404/409/429 where appropriate instead of blanket 500.
    - Preserve generic 500 fallback for unexpected failures.
  - Test plan:
    - Add handler-level tests for invalid action and representative error mapping.
    - Run `cargo test`.
  - Completion note: _pending_

- [ ] **F-009 Make heartbeat loop resilient to transient command failures**
  - Files: `scripts/openclaw-bootstrap.sh`
  - Planned fix:
    - Ensure heartbeat command failure does not terminate runner loop under `set -e`.
  - Test plan:
    - Add script-generation test asserting resilient heartbeat command form.
    - Run `cargo test`.
  - Completion note: _pending_

## Low

- [ ] **F-010 Remove dead code and tighten maintenance checks**
  - Files: `src/application/provisioning.rs`, `src/infrastructure/digital_ocean.rs`, `Cargo.toml`, `Cargo.lock`, `tests/integration_tests.rs`, `.github/workflows/ci.yml`
  - Planned fix:
    - Remove unused `sync_droplet_status` and dead error variant.
    - Remove unused dependencies.
    - Improve pagination mock fidelity and tighten CI clippy scope.
  - Test plan:
    - Run `cargo check`, `cargo clippy --all-targets`, `cargo test`.
  - Completion note: _pending_
