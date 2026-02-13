# Audit Master Checklist

Legend: `[ ]` pending, `[x]` completed

## Priority Order
1. Critical/High correctness and security
2. High-impact performance and reliability
3. Cleanup and maintainability

- [x] **F-001 Silent no-op repository updates return success**
  - Files: `src/infrastructure/repository.rs`, `tests/integration_tests.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Validate `rows_affected()` for write operations that target a single row.
    - Return `RepositoryError::NotFound(...)` when no row is updated/deleted.
    - Add repository-level verification via integration mock behavior.
  - Test plan:
    - `cargo test --all-targets`
    - Focus: lifecycle/actions paths that call update/delete methods.
  - Completion note:
    - Added `ensure_single_row_affected(...)` and wired it into single-row account/bot update/delete methods in `PostgresAccountRepository` and `PostgresBotRepository`.
    - Added `infrastructure::repository::tests::ensure_single_row_affected_returns_not_found_when_no_rows_updated`.
    - Verified with `cargo test --all-targets` (pass).

- [ ] **F-002 Rate-limit flow is rolled back as hard failure**
  - Files: `src/application/provisioning.rs`, `src/server/http.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Classify retryable provisioning failures (DO 429) separately from fatal failures.
    - Keep bot row/counter for retryable failures and only rollback fatal failures.
    - Preserve existing API semantics (`429` on create).
  - Test plan:
    - Add targeted unit test in provisioning module.
    - `cargo test --all-targets`

- [ ] **F-003 User-data shell interpolation injection risk**
  - Files: `src/application/provisioning.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Add shell-safe escaping helper for interpolated env values.
    - Apply escaping to all interpolated user-data exports.
    - Verify generated script behavior with special characters.
  - Test plan:
    - Add unit tests for escaping and generated output.
    - `cargo test --all-targets`

- [ ] **F-004 Bootstrap can fail on missing `ufw` under `set -e`**
  - Files: `scripts/openclaw-bootstrap.sh`, `src/application/provisioning.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Guard firewall setup with `command -v ufw`.
    - Keep setup behavior unchanged when `ufw` is present.
    - Log explicit warning when `ufw` is absent.
  - Test plan:
    - Extend provisioning script-content assertions.
    - `cargo test --all-targets`

- [ ] **F-005 Bootstrap `curl` calls lack timeouts**
  - Files: `scripts/openclaw-bootstrap.sh`, `src/application/provisioning.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Add bounded connect and total timeouts to registration/config/heartbeat calls.
    - Preserve retry/backoff behavior.
    - Keep response handling unchanged.
  - Test plan:
    - Extend script-content assertions.
    - `cargo test --all-targets`

- [ ] **F-006 Incorrect 404 mapping for non-not-found bot read errors**
  - Files: `src/server/http.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Differentiate repository not-found from infrastructure/internal errors.
    - Return `500` for internal failures.
    - Keep `404` for true missing resources.
  - Test plan:
    - Add handler error mapping tests.
    - `cargo test --all-targets`

- [ ] **F-007 `GET /accounts/{id}` is wired but returns 501**
  - Files: `src/server/http.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Implement account lookup route using existing repository.
    - Return `404` when account missing and `500` on internal errors.
    - Update OpenAPI response docs accordingly.
  - Test plan:
    - Add endpoint-level unit tests for not found and success path helpers.
    - `cargo test --all-targets`

- [ ] **F-008 Oversized modules reduce maintainability**
  - Files: `src/server/http.rs`, `src/server/http_types.rs`, `src/server/http_auth.rs`, `src/server/http_parse.rs`, `src/server/http_errors.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Split `http.rs` into focused internal modules (types/auth/parsing/errors).
    - Keep existing route behavior and signatures stable.
    - Avoid cross-cutting refactors outside server module.
  - Test plan:
    - `cargo check --all-targets`
    - `cargo test --all-targets`

- [ ] **F-009 Unused dependency `tower-http`**
  - Files: `Cargo.toml`, `Cargo.lock`, `AUDIT_TODO.md`
  - Planned fix:
    - Remove unused dependency and feature linkage.
    - Verify no code paths require it.
  - Test plan:
    - `cargo check --all-targets`
    - `cargo clippy --all-targets -- -D warnings`

- [ ] **F-010 Sanitization test does not validate sanitization logic**
  - Files: `src/application/provisioning.rs`, `tests/integration_tests.rs`, `AUDIT_TODO.md`
  - Planned fix:
    - Add direct unit tests that assert transformation behavior.
    - Replace or trim misleading integration test coverage.
    - Verify UTF-8 and special-char handling.
  - Test plan:
    - `cargo test --all-targets`
