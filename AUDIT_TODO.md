# Audit Master Checklist (Current Findings)

Legend: `[ ]` pending, `[x]` completed

Priority order: Critical/High correctness & security -> performance/reliability -> cleanup/maintainability.

- [x] **F-001 Counter leak on DigitalOcean rate limiting**
  - Files touched: `src/application/provisioning.rs`, `src/application/provisioning.rs` (tests), `AUDIT_TODO.md`
  - Planned fix:
    - Ensure create flow compensates persisted state when droplet creation returns `RateLimited`.
    - Decrement account bot counter and remove partial bot row for failed create requests.
    - Keep behavior explicit and test-backed.
  - Test plan:
    - Add/update provisioning unit tests for 429 rollback semantics.
    - Run `cargo test`.
  - Completion note:
    - Removed the special-case rollback bypass for `DigitalOceanError::RateLimited` in `create_bot(...)`.
    - Create failures now consistently trigger compensating cleanup (`hard_delete` + `decrement_bot_counter`), preventing quota/state leaks.
    - Verified with `cargo test` (pass).

- [ ] **F-002 Incorrect 500 for missing account on `POST /bots`**
  - Files touched: `src/server/http.rs`, `src/server/http_errors.rs`, `src/server/http.rs` (tests), `AUDIT_TODO.md`
  - Planned fix:
    - Map `RepositoryError::NotFound` from create-bot path to `404`.
    - Preserve `500` for unexpected/internal failures.
  - Test plan:
    - Add/update HTTP error mapping test.
    - Run `cargo test`.

- [ ] **F-003 Incorrect 500 mapping on `GET /bots/{id}/config` for not-found**
  - Files touched: `src/server/http.rs`, `src/server/http_errors.rs`, `src/server/http.rs` (tests), `AUDIT_TODO.md`
  - Planned fix:
    - Distinguish not-found lifecycle/repository errors and return `404`.
    - Keep other failures mapped to `500`.
  - Test plan:
    - Add/update mapping test(s).
    - Run `cargo test`.

- [ ] **F-004 `config_ack` collapses all failures to 400**
  - Files touched: `src/server/http.rs`, `src/server/http_errors.rs`, `src/server/http.rs` (tests), `AUDIT_TODO.md`
  - Planned fix:
    - Return `404` for missing bot/config and `409` for config version conflict.
    - Keep validation errors as `400` and unknown failures as `500`.
  - Test plan:
    - Add mapping tests for not-found/conflict/internal paths.
    - Run `cargo test`.

- [ ] **F-005 Silent no-op droplet updates**
  - Files touched: `src/infrastructure/postgres_droplet_repo.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Validate `rows_affected()` for single-row droplet updates.
    - Return `RepositoryError::NotFound` when no row is updated.
  - Test plan:
    - Run `cargo test`.
    - Verify compile-time and behavioral consistency.

- [ ] **F-006 `make db` fails with password-protected Postgres**
  - Files touched: `Makefile`, `AUDIT_TODO.md`
  - Planned fix:
    - Parse and export password (or use full URL) for `psql` calls.
    - Keep existing DB create-if-missing behavior.
  - Test plan:
    - Run `make -n db` for command validation.
    - Run `cargo test` regression sanity.

- [ ] **F-007 Insecure docker-compose defaults (password + exposed DB port)**
  - Files touched: `docker-compose.yml`, `README.md`, `AUDIT_TODO.md`
  - Planned fix:
    - Remove hardcoded default DB password from compose source.
    - Restrict DB host exposure by default and document opt-in local mapping.
  - Test plan:
    - Run `docker compose config`.

- [ ] **F-008 CI missing Docker/bootstrap checks**
  - Files touched: `.github/workflows/ci.yml`, `AUDIT_TODO.md`
  - Planned fix:
    - Add Docker image build step.
    - Add bootstrap script lint via `shellcheck`.
  - Test plan:
    - Validate workflow YAML locally (syntax review) and run `cargo test`.

- [ ] **F-009 Dead migration objects (unused sequence/function)**
  - Files touched: `migrations/008_remove_unused_config_version_objects.sql`, `AUDIT_TODO.md`
  - Planned fix:
    - Add forward migration removing obsolete objects from migration 004.
    - Keep active atomic function intact.
  - Test plan:
    - Run `cargo test`.
    - Confirm migration SQL syntax.

- [ ] **F-010 DigitalOcean client retry logic duplication**
  - Files touched: `src/infrastructure/digital_ocean.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Introduce shared retry/request helper to remove duplication.
    - Preserve endpoint behavior and error mapping.
  - Test plan:
    - Run `cargo test` and `cargo clippy -- -D warnings`.
