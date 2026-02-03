# mvp-plan.md — Trawling Traders (1-week ship)

## Goal (MVP)
Ship a working React Native app that lets a subscribed user:
- create up to **4** OpenClaw bots (each on its own DigitalOcean VPS),
- set a **simple trading profile** (persona + algorithm + asset focus + risk caps),
- store secrets (LLM API key) securely,
- see **bot status + basic performance** (one chart + event list),
- pause/resume/redeploy/destroy bots.

**No** social features, **no** leaderboard, **no** advanced builder UI.

---

## MVP personas (top 3)
These are *UX personas* (how we present complexity), not “AI personalities.”

1) **Set & Forget Beginner**
- Wants: “Pick a style and go.”
- UI defaults: Paper mode ON, strict safety caps, Majors-only.
- Explanations: short, reassuring, “waiting for setups” state.

2) **Hands-on Tweaker**
- Wants: choose assets, tune risk, see why trades happen.
- UI: can edit asset list (with guardrails) + risk caps + algorithm mode.
- Explanations: “why this trade” bullets and simple history.

3) **Quant-lite Power User**
- Wants: more control, but not code.
- UI: exposes a small set of “signal knobs” (3–5 toggles/weights),
  portfolio caps, and execution profile.
- Still avoids full factor/regime builder (not in MVP).

---

## Simplified algorithm options (v1)
Keep this extremely small and internally map each option to OpenClaw config.

### Algorithm Mode (choose 1)
A) **Trend**
- “Ride momentum with confirmations.”
- Controls shown: confidence threshold, max hold time.

B) **Mean Reversion**
- “Fade extremes; take smaller, frequent trades.”
- Controls shown: reversion sensitivity (low/med/high), max hold time.

C) **Breakout**
- “Trade breakouts with volume confirmation.”
- Controls shown: breakout strictness (low/med/high), cooldown.

### Optional “Signal Knobs” (Quant-lite only)
Expose at most 4 knobs (0/1 toggles or Low/Med/High):
- Volume confirmation
- Volatility brake (reduce size in high vol)
- Liquidity filter strictness
- Correlation brake (avoid stacking similar bets)

> Implementation note: these compile into a tiny JSON config and are not “free-form.”

---

## MVP bot settings (what the app exposes)
Per bot:

### Identity
- Name (string) + optional icon color/avatar
- Persona: Beginner / Tweaker / Quant-lite (drives which settings UI shows)

### Trading focus
- **Asset focus:** Majors (default) | Memes | Custom allowlist
  - Guardrails always on: min liquidity/volume/spread thresholds (server-side enforced)

### Algorithm
- Mode: Trend | Mean Reversion | Breakout
- Strictness: Low | Medium | High (maps to internal thresholds)

### Risk & safety (always shown)
- Max position size (% of portfolio)
- Max daily loss (stop trading)
- Max drawdown (stop trading)
- Max trades/day
- Mode: **Paper** (default) | Live

### Secrets (minimal)
- LLM provider selection + API key (encrypted at rest)

---

## Backend (Rust control plane) — minimum required
Wrap Cedros Login + Cedros Pay; build only what’s needed for provisioning + config + telemetry.

### Services/modules
1) **Auth & Subscription**
- Cedros Login integration
- Cedros Pay subscription gating
- Entitlement: max_bots = 4

2) **Bot lifecycle**
- Create / Pause / Resume / Redeploy / Destroy
- Provisioning queue worker (respect DO “<=10 concurrent creates”)
- Store `droplet_id`, region, status

3) **Config versions + secrets**
- `PATCH /bots/{id}/config` creates a new immutable config version
- Secrets stored encrypted (LLM key)
- Desired-state pointer on bot: `desired_version_id`

4) **Reconciliation loop**
- Bot pulls `/bot/{id}/config`
- Bot posts `/bot/{id}/config_ack` with effective hash/version
- Server marks applied

5) **Telemetry ingest (minimal)**
- Heartbeat: online/offline
- Metrics: a single series (equity or pnl) batched
- Events: simple list (trade opened/closed, stop triggered, error)

### Minimal API surface
App → Server
- `POST /bots`
- `GET /bots`
- `GET /bots/{id}`
- `PATCH /bots/{id}/config`
- `POST /bots/{id}/actions` (pause/resume/redeploy/destroy)
- `GET /bots/{id}/metrics?range=7d|30d`
- `GET /bots/{id}/events?cursor=...`

Bot → Server
- `POST /bot/register`
- `POST /bot/heartbeat`
- `GET /bot/{id}/config`
- `POST /bot/{id}/config_ack`
- `POST /bot/{id}/metrics_batch`
- `POST /bot/{id}/events_batch`

---

## DigitalOcean provisioning (MVP)
- 1 droplet per bot
- `user_data` bootstrap starts OpenClaw agent and registers once
- Firewall: default deny inbound; outbound allowlist (control plane + required endpoints)
- Queue creates to avoid >10 concurrent create operations

---

## React Native app (MVP) — screens
1) **Auth**
- Cedros Login flow

2) **Subscribe**
- Cedros Pay flow (single tier MVP)

3) **Bots List**
- Show up to 4 bots
- Create Bot button
- Each bot card: status, today PnL (if available), last heartbeat

4) **Create Bot**
- Name + Persona
- Asset focus (Majors/Memes/Custom)
- Algorithm mode (Trend/Reversion/Breakout)
- Risk caps
- Paper vs Live (default Paper)
- Save → triggers provisioning

5) **Bot Detail**
- Status line + last heartbeat
- Basic chart (equity/PnL over time)
- Events list (most recent)
- Actions: Pause/Resume/Redeploy/Destroy

6) **Settings (per bot)**
- Edit the same fields as Create Bot
- Apply → creates new config version and shows “Pending/Applied”

---

## 1-week execution plan (day-by-day)
Assuming Cedros Login/Pay already exist and OpenClaw agent supports register/heartbeat/config/metrics.

### Day 1 — Scaffolding
- RN project + navigation + API client + auth shell
- Rust control plane skeleton + DB models (User, Bot, ConfigVersion, Secret, Metric, Event)
- Define Config Schema v1 (MVP subset only)

### Day 2 — DigitalOcean provisioning
- Provisioning queue worker
- Create droplet + user_data bootstrap + tag droplet
- Bot registration endpoint + one-time token

### Day 3 — Desired config + reconciliation
- Config version endpoints + encryption for secrets
- Bot config pull + ack endpoints
- App: Create Bot flow writes desired config

### Day 4 — Telemetry v1
- Heartbeat ingest + status updates
- Metrics ingest (single series) + query
- Events ingest + query
- App: Bot detail shows status + events list

### Day 5 — UI polish + guardrails
- Bots list polish + loading/provisioning states
- Bot settings edit + “Pending/Applied/Failed” indicators
- Enforce safety defaults server-side (even if UI misconfigured)

### Day 6 — Hardening
- Retry logic for provisioning + reconciliation
- Redeploy/destroy flows
- Basic rate limiting and auth checks
- Crash/restart scenarios (bot comes back and re-acks)

### Day 7 — Ship checklist
- E2E test: subscribe → create bot → online → config apply → metrics show
- App store build pipeline sanity (TestFlight/internal)
- Logging + minimal monitoring (alerts on provisioning failures)

---

## MVP success criteria
- User can subscribe, create a bot, see it come online, update settings, and see confirmation of apply.
- Paper mode works end-to-end (live can be gated behind a toggle/acknowledgment).
- Bot detail shows a chart (even if sparse) and recent events.
- System stays stable under multiple simultaneous bot creates (queue handles DO limit).

---

## Explicit non-goals (MVP)
- No social, no leaderboard
- No full factor/regime builder UI
- No complex backtesting UI (paper/shadow only)
- No multi-bot portfolio coordination
