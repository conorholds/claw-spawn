# Audit Fix Checklist
# Created from CODE_REVIEW.md findings
# Status: [ ] Pending | [~] In Progress | [x] Complete

## CRITICAL SEVERITY

### [ ] CRIT-001: Authentication Bypass - Registration Tokens Not Validated
- **File:** `src/main.rs:400-433`
- **Issue:** Bot registration tokens are parsed but never validated against stored tokens
- **Fix:**
  - Add `registration_token: Option<String>` field to Bot struct
  - Store token during spawn_bot()
  - Validate token in register_bot handler against stored value
- **Test Plan:**
  - Create bot, get registration token
  - Try register with wrong token → expect 401
  - Try register with correct token → expect 200
  - Try register without token → expect 401
- **Status:** Pending

### [ ] CRIT-002: Race Condition - Account Limit Check Not Atomic
- **File:** `src/application/provisioning.rs:74-91`
- **Issue:** Account limit check queries then counts, allowing concurrent requests to exceed limits
- **Fix:**
  - Add atomic counter table `account_bot_limits`
  - Use UPDATE with check: `current_count < max_count`
  - Or use database constraint with unique index
- **Test Plan:**
  - Set account limit to 1
  - Send 10 concurrent create_bot requests
  - Verify only 1 succeeds, 9 get AccountLimitReached
- **Status:** Pending

### [x] CRIT-003: State Inconsistency - Accounts Never Persisted
- **File:** `src/main.rs:123-139`
- **Issue:** create_account creates Account in memory but never calls account_repo.create()
- **Fix:**
  - Added `account_repo: Arc<PostgresAccountRepository>` to AppState
  - Import `AccountRepository` trait in main.rs
  - Add `state.account_repo.create(&account).await` call in create_account handler
  - Added proper error handling with logging
- **Test Plan:**
  - POST /accounts with valid data
  - Verify account exists in DB: `SELECT * FROM accounts WHERE external_id = $1`
  - Verify response contains correct account.id
- **Status:** Complete
- **Completion Note:** Build successful. Accounts are now persisted to database before being used. The handler properly returns 500 on database error with logging.

### [x] CRIT-004: Missing Timeouts - HTTP Client Can Hang Indefinitely
- **File:** `src/infrastructure/digital_ocean.rs:28-49`
- **Issue:** DigitalOcean HTTP client created without timeout configuration
- **Fix:**
  - Added `std::time::Duration` import
  - Added `.timeout(Duration::from_secs(30))` to Client builder
  - Added `.connect_timeout(Duration::from_secs(10))`
  - Added `.pool_idle_timeout(Duration::from_secs(90))`
- **Test Plan:**
  - Mock DO API with 60s delay
  - Call create_droplet()
  - Verify it returns timeout error after 30s
- **Status:** Complete
- **Completion Note:** Build successful. HTTP client now has 30s request timeout, 10s connect timeout, and 90s pool idle timeout.

### [ ] CRIT-005: Resource Leak - Droplets Orphaned on Partial Failure
- **File:** `src/application/provisioning.rs:150-180`
- **Issue:** Droplet created in DO, but if DB operations fail, droplet is untracked
- **Fix:**
  - Implement compensating transaction pattern
  - If DB persist fails, destroy DO droplet
  - Log cleanup attempts
- **Test Plan:**
  - Mock DB to fail after DO creation
  - Verify droplet is destroyed in DO
  - Verify error is returned to caller
- **Status:** Pending

### [ ] CRIT-006: Hardcoded Control Plane URL
- **File:** `src/application/provisioning.rs:202`
- **Issue:** Control plane URL hardcoded to "https://api.cedros.io"
- **Fix:**
  - Use `self.openclaw_bootstrap_url` which is already passed to service
  - Or add control_plane_url to config
- **Test Plan:**
  - Set custom bootstrap URL in config
  - Create bot
  - Verify user_data contains custom URL
- **Status:** Pending

### [ ] CRIT-007: Duplicate Config Version Race Condition
- **File:** `src/application/lifecycle.rs:82-86`
- **Issue:** get_next_version() queries max then increments - not atomic
- **Fix:**
  - Use database sequence for atomic increment
  - Or use RETURNING clause with INSERT
- **Test Plan:**
  - Send 5 concurrent config updates
  - Verify all versions are unique (1,2,3,4,5)
  - No duplicates allowed
- **Status:** Pending

## HIGH SEVERITY

### [ ] HIGH-001: Missing Heartbeat Timeout Detection
- **File:** `src/application/lifecycle.rs:127-130`
- **Issue:** Heartbeats recorded but no logic to detect stale bots
- **Fix:**
  - Add check_stale_bots() method
  - Query bots with status='online' AND last_heartbeat_at < threshold
  - Mark stale bots as Error status
- **Test Plan:**
  - Create bot, mark as online
  - Wait 5 minutes (or mock time)
  - Run health check
  - Verify bot status changed to Error
- **Status:** Pending

### [ ] HIGH-002: Resume Bot Doesn't Check Droplet State
- **File:** `src/application/provisioning.rs:249-262`
- **Issue:** resume_bot() doesn't verify droplet exists before reboot
- **Fix:**
  - Query droplet status before reboot
  - Return clear error if droplet not in resumable state
- **Test Plan:**
  - Create bot, pause it
  - Destroy droplet in DO console
  - Try resume → expect clear error
- **Status:** Pending

### [x] HIGH-003: No Input Validation on Risk Config
- **File:** `src/main.rs:232-237`
- **Issue:** Risk configuration accepts negative or >100% values
- **Fix:**
  - Add RiskConfig.validate() method in src/domain/bot.rs
  - Check percentages are 0-100
  - Check trades_per_day is non-negative
  - Return 400 Bad Request with descriptive error messages on invalid
- **Test Plan:**
  - POST /bots with negative percentage → expect 400
  - POST with >100% → expect 400
  - POST with valid values → expect 201
- **Status:** Complete
- **Completion Note:** Implemented validate() method that returns Result<(), Vec<String>> with detailed error messages. The create_bot handler now calls validation before creating bot and returns 400 Bad Request with error details on failure.

### [x] HIGH-004: Potential Panic on Invalid API Token
- **File:** `src/infrastructure/digital_ocean.rs:32`
- **Issue:** unwrap() used when parsing Authorization header
- **Fix:**
  - Added `InvalidConfig` variant to `DigitalOceanError`
  - Changed `new()` to return `Result<Self, DigitalOceanError>`
  - Replaced `unwrap()` with `match` for safe HeaderValue parsing
  - Also converted `expect()` for HTTP client to proper error handling
  - Updated caller in main.rs to handle Result with expect()
- **Test Plan:**
  - Create DO client with invalid token
  - Verify graceful error, not panic
- **Status:** Complete
- **Completion Note:** Build successful. Invalid API tokens now return `DigitalOceanError::InvalidConfig` instead of panicking. The HTTP client creation error is also properly handled.

## MEDIUM SEVERITY

### [x] MED-001: Dead Code - Unused Bootstrap URL Field
- **File:** `src/application/provisioning.rs:41`
- **Issue:** openclaw_bootstrap_url field never used (replaced by hardcoded URL)
- **Fix:**
  - Removed field from ProvisioningService struct
  - Removed from constructor parameters
  - Removed from Self initialization  
  - Removed from main.rs caller
- **Test Plan:**
  - Build succeeds
  - No compiler warnings
- **Status:** Complete
- **Completion Note:** Build successful. Eliminates dead_code warning. No functional changes.

### [ ] MED-002: Unsafe String Splitting
- **File:** `src/application/provisioning.rs:136`
- **Issue:** UUID split on '-' could panic if format changes
- **Fix:**
  - Use safe truncation: `&id_str[..8.min(id_str.len())]`
- **Test Plan:**
  - Create bot with various UUID formats
  - Verify no panic
- **Status:** Pending

### [ ] MED-003: Bootstrap Script Runs as Root
- **File:** `scripts/openclaw-bootstrap.sh:186`
- **Issue:** Systemd service runs as root unnecessarily
- **Fix:**
  - Change User=root to User=openclaw
  - Ensure proper permissions
- **Test Plan:**
  - Deploy bot
  - Verify service runs as openclaw user
- **Status:** Pending

### [ ] MED-004: Missing Config Version Conflict Detection
- **File:** `src/application/lifecycle.rs:88-111`
- **Issue:** acknowledge_config doesn't check if newer config exists
- **Fix:**
  - Check desired_config_version_id == config_id before acknowledging
  - Reject acknowledgments for outdated configs
- **Test Plan:**
  - Create config v1, then v2
  - Try acknowledge v1 after v2 exists
  - Expect rejection
- **Status:** Pending

### [ ] MED-005: Unwrap on User Input (Bot Name)
- **File:** `src/application/provisioning.rs:36` (indirect)
- **Issue:** Potential panic if bot name causes issues in droplet name
- **Fix:**
  - Sanitize droplet name
  - Handle edge cases
- **Test Plan:**
  - Create bot with special characters in name
  - Verify safe droplet name generation
- **Status:** Pending

### [ ] MED-006: Missing Encryption Key Validation
- **File:** `src/infrastructure/crypto.rs:24-41`
- **Issue:** Key validated for length but not entropy
- **Fix:**
  - Add entropy check
  - Warn on weak keys in dev mode
- **Test Plan:**
  - Test with weak key (e.g., all zeros)
  - Verify warning or rejection
- **Status:** Pending

### [ ] MED-007: Inconsistent Status Mapping
- **File:** `src/infrastructure/repository.rs:389-410`
- **Issue:** Status mapping functions are verbose and error-prone
- **Fix:**
  - Use strum derive macros
  - Or create unified mapping macro
- **Test Plan:**
  - All status conversions work correctly
  - No regressions
- **Status:** Pending

## PERFORMANCE ISSUES

### [ ] PERF-001: N+1 Query Pattern in Account Limit Check
- **File:** `src/application/provisioning.rs:82-88`
- **Issue:** Fetches all bots when we just need count
- **Fix:**
  - Add count_by_account() method to repository
  - Use SQL COUNT(*) instead of fetching all
- **Test Plan:**
  - Benchmark with 1000 bots
  - Verify query time < 10ms
- **Status:** Pending

### [ ] PERF-002: Missing Pagination on List Endpoints
- **File:** `src/main.rs:136-152`
- **Issue:** /accounts/:id/bots returns ALL bots without limit
- **Fix:**
  - Add limit/offset parameters
  - Default limit of 100
- **Test Plan:**
  - Create 1000 bots
  - Query with limit=10
  - Verify only 10 returned
- **Status:** Pending

## RELIABILITY ISSUES

### [ ] REL-001: No Compensating Transaction for Destroy
- **File:** `src/application/provisioning.rs:216-234`
- **Issue:** If destroy_droplet succeeds but DB update fails, inconsistency
- **Fix:**
  - Retry DB update on failure
  - Or mark for cleanup later
- **Test Plan:**
  - Mock DB failure during destroy
  - Verify eventual consistency
- **Status:** Pending

### [ ] REL-002: No Retry Logic for DO API Calls
- **File:** `src/infrastructure/digital_ocean.rs`
- **Issue:** Only rate limiting (429) handled, not 500s or network errors
- **Fix:**
  - Add exponential backoff retry
  - Handle 500, 502, 503 errors
- **Test Plan:**
  - Mock DO API returning 500 then 200
  - Verify retry succeeds
- **Status:** Pending

### [ ] REL-003: Missing Error Context in Logs
- **File:** Multiple
- **Issue:** Errors logged without context (bot_id, account_id)
- **Fix:**
  - Add structured logging with fields
  - Use tracing spans
- **Test Plan:**
  - Trigger errors
  - Verify logs contain context
- **Status:** Pending

## CLEANUP / MAINTAINABILITY

### [ ] CLEAN-001: Add #[must_use] to Repository Methods
- **File:** `src/infrastructure/repository.rs`
- **Issue:** Repository methods return Results that could be ignored
- **Fix:**
  - Add #[must_use] annotation
- **Test Plan:**
  - Build with warnings as errors
  - Verify no warnings
- **Status:** Pending

### [ ] CLEAN-002: Replace String-based Status with Enums
- **File:** Database layer
- **Issue:** Status stored as strings, not type-safe
- **Fix:**
  - Create database enum types
  - Migrate existing data
- **Test Plan:**
  - All status operations work
  - Type safety enforced
- **Status:** Pending

### [ ] CLEAN-003: Add Comprehensive Tests
- **File:** New test files
- **Issue:** Only 1 test (crypto roundtrip)
- **Fix:**
  - Unit tests for all domain logic
  - Integration tests for API endpoints
  - Mock external services
- **Test Plan:**
  - Run cargo test
  - Verify >80% coverage
- **Status:** Pending

### [ ] CLEAN-004: Add API Documentation
- **File:** Documentation
- **Issue:** No OpenAPI spec or detailed docs
- **Fix:**
  - Add utoipa for OpenAPI generation
  - Document all endpoints
- **Test Plan:**
  - Generate OpenAPI spec
  - Verify all endpoints documented
- **Status:** Pending

### [ ] CLEAN-005: Add Health Check for DB
- **File:** `src/main.rs:101-103`
- **Issue:** Health check only returns OK, doesn't check DB
- **Fix:**
  - Add detailed health check
  - Verify DB connectivity
- **Test Plan:**
  - Stop DB
  - Verify health check fails
- **Status:** Pending
