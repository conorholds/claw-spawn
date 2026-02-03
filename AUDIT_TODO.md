# Audit Fix Checklist
# Created from CODE_REVIEW.md findings
# Status: [ ] Pending | [~] In Progress | [x] Complete

## CRITICAL SEVERITY

### [x] CRIT-001: Authentication Bypass - Registration Tokens Not Validated
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
- **Status:** Complete
- **Completion Note:** Implemented registration token validation. Token is generated and stored when bot is spawned. register_bot handler now validates token against database using get_by_id_with_token(). Returns 401 for invalid/missing tokens. Migration 002_add_registration_token.sql adds the column.

### [x] CRIT-002: Race Condition - Account Limit Check Not Atomic
- **File:** `src/application/provisioning.rs:74-91`
- **Issue:** Account limit check queries then counts, allowing concurrent requests to exceed limits
- **Fix:**
  - Created migration `003_account_bot_counters.sql` with atomic counter table
  - Added SQL functions `increment_bot_counter()` and `decrement_bot_counter()`
  - Updated `BotRepository` trait with `increment_bot_counter()` and `decrement_bot_counter()` methods
  - Implemented methods in `PostgresBotRepository` with atomic SQL operations
  - Modified `create_bot()` to use atomic counter instead of query+count
  - Modified `destroy_bot()` to decrement counter on successful destruction
  - Added triggers to auto-initialize counter on account creation and update on max_bots change
- **Test Plan:**
  - Set account limit to 1
  - Send 10 concurrent create_bot requests
  - Verify only 1 succeeds, 9 get AccountLimitReached
- **Status:** Complete
- **Completion Note:** Atomic counter prevents TOCTOU race condition. Counter is updated atomically with limit check in single SQL operation. Cleanup on failure ensures counter consistency.

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

### [x] CRIT-005: Resource Leak - Droplets Orphaned on Partial Failure
- **File:** `src/application/provisioning.rs:150-180`
- **Issue:** Droplet created in DO, but if DB operations fail, droplet is untracked
- **Fix:**
  - Modified `spawn_bot()` to use compensating transaction pattern
  - Create DO droplet first, then attempt DB persistence
  - If DB operations fail, immediately destroy the droplet via DO API
  - Log cleanup success/failure for monitoring orphaned resources
  - Update bot status to Error on persistence failure
- **Test Plan:**
  - Mock DB to fail after DO creation
  - Verify droplet is destroyed in DO
  - Verify error is returned to caller
- **Status:** Complete
- **Completion Note:** Implements proper resource cleanup. Droplet is created first, then DB operations are attempted. On failure, the droplet is destroyed and appropriate error logs are generated. Prevents orphaned resources in DigitalOcean.

### [x] CRIT-006: Hardcoded Control Plane URL
- **File:** `src/application/provisioning.rs:202`
- **Issue:** Control plane URL hardcoded to "https://api.cedros.io"
- **Fix:**
  - Added `control_plane_url` field to AppConfig with default value
  - Pass control_plane_url to ProvisioningService constructor
  - Use `self.control_plane_url` in generate_user_data() instead of hardcoded string
- **Test Plan:**
  - Set custom bootstrap URL in config
  - Create bot
  - Verify user_data contains custom URL
- **Status:** Complete
- **Completion Note:** Control plane URL is now configurable via CEDROS_CONTROL_PLANE_URL environment variable. Default remains https://api.cedros.io for backwards compatibility.

### [x] CRIT-007: Duplicate Config Version Race Condition
- **File:** `src/application/lifecycle.rs:82-86`
- **Issue:** get_next_version() queries max then increments - not atomic
- **Fix:**
  - Created migration `004_config_version_sequence.sql` with advisory lock-based function
  - Added SQL function `get_next_config_version_atomic()` using `pg_advisory_xact_lock()`
  - Updated `ConfigRepository` trait with `get_next_version_atomic()` method
  - Implemented method in `PostgresConfigRepository` calling the atomic SQL function
  - Modified `create_bot_config()` in lifecycle.rs to use atomic version generation
  - Removed old non-atomic `get_next_version()` method
- **Test Plan:**
  - Send 5 concurrent config updates
  - Verify all versions are unique (1,2,3,4,5)
  - No duplicates allowed
- **Status:** Complete
- **Completion Note:** Advisory locks ensure exclusive access per-bot during version generation. Lock is automatically released at transaction end. Prevents duplicate version numbers under concurrent config updates.

## HIGH SEVERITY

### [x] HIGH-001: Missing Heartbeat Timeout Detection
- **File:** `src/application/lifecycle.rs:127-130`
- **Issue:** Heartbeats recorded but no logic to detect stale bots
- **Fix:**
  - Added check_stale_bots() method to BotLifecycleService
  - Added list_stale_bots() to BotRepository trait and PostgresBotRepository implementation
  - Query bots with status='online' AND (last_heartbeat_at < threshold OR NULL)
  - Mark stale bots as Error status with appropriate logging
- **Test Plan:**
  - Create bot, mark as online
  - Wait 5 minutes (or mock time)
  - Run health check
  - Verify bot status changed to Error
- **Status:** Complete
- **Completion Note:** Build successful. Stale bot detection queries for online bots with heartbeat older than threshold. Includes NULL heartbeat handling for bots that never reported.

### [x] HIGH-002: Resume Bot Doesn't Check Droplet State
- **File:** `src/application/provisioning.rs:249-262`
- **Issue:** resume_bot() doesn't verify droplet exists before reboot
- **Fix:**
  - Verify bot is in Paused state before attempting resume
  - Query droplet status via DO API before reboot
  - Handle different states: Off (reboot), Active (no-op), New/Destroyed (error)
  - Return clear error messages for invalid states
- **Test Plan:**
  - Create bot, pause it
  - Destroy droplet in DO console
  - Try resume → expect clear error
- **Status:** Complete
- **Completion Note:** resume_bot() now checks droplet state before reboot. Returns clear error if droplet doesn't exist, is still being created, or bot has no droplet. Prevents errors from trying to resume orphaned bots.

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

### [x] MED-002: Unsafe String Splitting
- **File:** `src/application/provisioning.rs:133`
- **Issue:** UUID split on '-' could panic if format changes
- **Fix:**
  - Changed to safe truncation: `&id_str[..8.min(id_str.len())]`
  - Stores UUID in variable first to avoid multiple to_string() calls
- **Test Plan:**
  - Create bot with various UUID formats
  - Verify no panic
- **Status:** Complete
- **Completion Note:** Build successful. Safe truncation prevents panic if UUID format changes.

### [x] MED-003: Bootstrap Script Runs as Root
- **File:** `scripts/openclaw-bootstrap.sh:186`
- **Issue:** Systemd service runs as root unnecessarily
- **Fix:**
  - Changed User=root to User=openclaw
  - Added Group=openclaw to service file
  - Added chown commands to set proper ownership for /opt/openclaw directory
  - Added touch and chown for /var/log/openclaw-bot.log
- **Test Plan:**
  - Deploy bot
  - Verify service runs as openclaw user
- **Status:** Complete
- **Completion Note:** Service now runs as openclaw user. Permissions set during bootstrap ensure bot user can access working directory and log files.

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

### [x] MED-006: Missing Encryption Key Validation
- **File:** `src/infrastructure/crypto.rs:24-41`
- **Issue:** Key validated for length but not entropy
- **Fix:**
  - Added validate_key_entropy() method to SecretsEncryption
  - Checks for all-zeros, uniform values, and low entropy patterns
  - Warns when key contains <50% unique bytes
  - Detects dictionary words (password, secret, 123, key) in keys
  - Uses tracing::warn for development mode visibility
- **Test Plan:**
  - Test with weak key (e.g., all zeros)
  - Verify warning or rejection
- **Status:** Complete
- **Completion Note:** Build successful. Key entropy validation provides security warnings without breaking functionality. Low entropy keys log warnings but continue to work for backwards compatibility.

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
