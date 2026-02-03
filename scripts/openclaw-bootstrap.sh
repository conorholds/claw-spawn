#!/bin/bash
# OpenClaw Bot Bootstrap Script
# This script is embedded in the DigitalOcean droplet user_data

set -e

# NOTE: Do not enable `set -x` (xtrace). This script handles secrets (registration token)
# and xtrace would leak them into cloud-init logs.

export DEBIAN_FRONTEND=noninteractive

# Configuration from environment (passed by provisioning service)
REGISTRATION_TOKEN="${REGISTRATION_TOKEN}"
BOT_ID="${BOT_ID}"
CONTROL_PLANE_URL="${CONTROL_PLANE_URL:-https://api.cedros.io}"
BOT_CONFIG='${BOT_CONFIG}'

# Workspace/customization (janebot-cli)
CUSTOMIZER_REPO_URL="${CUSTOMIZER_REPO_URL:-https://github.com/janebot2026/janebot-cli.git}"
CUSTOMIZER_REF="${CUSTOMIZER_REF:-4b170b4aa31f79bda84f7383b3992ca8681d06d3}"
CUSTOMIZER_WORKSPACE_DIR="${CUSTOMIZER_WORKSPACE_DIR:-/opt/openclaw/workspace}"
CUSTOMIZER_AGENT_NAME="${CUSTOMIZER_AGENT_NAME:-Jane}"
CUSTOMIZER_OWNER_NAME="${CUSTOMIZER_OWNER_NAME:-Cedros}"
CUSTOMIZER_SKIP_QMD="${CUSTOMIZER_SKIP_QMD:-true}"
CUSTOMIZER_SKIP_CRON="${CUSTOMIZER_SKIP_CRON:-true}"
CUSTOMIZER_SKIP_GIT="${CUSTOMIZER_SKIP_GIT:-true}"
CUSTOMIZER_SKIP_HEARTBEAT="${CUSTOMIZER_SKIP_HEARTBEAT:-true}"

echo "=== OpenClaw Bot Setup Starting ==="
echo "Bot ID: $BOT_ID"
echo "Control Plane: $CONTROL_PLANE_URL"
echo "Date: $(date)"

# Update system
echo "=== Updating System ==="
apt-get update
apt-get upgrade -y

# Install dependencies
echo "=== Installing Dependencies ==="
apt-get install -y \
    curl \
    wget \
    git \
    ca-certificates \
    gnupg \
    lsb-release \
    software-properties-common \
    apt-transport-https \
    jq

# Install Node.js 18+ (janebot-cli requires node >=18)
echo "=== Installing Node.js (for janebot-cli) ==="
if command -v node >/dev/null 2>&1; then
    NODE_MAJOR=$(node -v 2>/dev/null | sed 's/^v\([0-9]*\).*/\1/')
else
    NODE_MAJOR=0
fi

if [ "${NODE_MAJOR:-0}" -lt 18 ]; then
    curl -fsSL https://deb.nodesource.com/setup_18.x | bash -
    apt-get install -y nodejs
else
    echo "Node already present: $(node -v)"
fi

# Install Docker
echo "=== Installing Docker ==="
install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
chmod a+r /etc/apt/keyrings/docker.gpg

echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null

apt-get update
apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin

# Start Docker
systemctl enable docker
systemctl start docker

# Create bot user
echo "=== Creating Bot User ==="
useradd -m -s /bin/bash -U openclaw || true
usermod -aG docker openclaw

# Prepare log file early so bootstrap steps are captured
touch /var/log/openclaw-bot.log
chown openclaw:openclaw /var/log/openclaw-bot.log

# Create working directories
mkdir -p /opt/openclaw
cd /opt/openclaw

# Bootstrap customized workspace layout (best-effort)
echo "=== Bootstrapping Clawdbot Workspace (janebot-cli) ==="
CUSTOMIZER_LOG="/var/log/openclaw-bot.log"
CUSTOMIZER_MARKER="/opt/openclaw/.customizer_ran"
CUSTOMIZER_STATUS_FILE="/opt/openclaw/customizer_status.txt"

if [ -f "$CUSTOMIZER_MARKER" ]; then
    echo "Customizer already ran; skipping workspace bootstrap" | tee -a "$CUSTOMIZER_LOG"
else
CUSTOMIZER_STATUS=0
set +e

JANE_DIR="/opt/openclaw/tools/janebot-cli"
mkdir -p "$(dirname "$JANE_DIR")" "$CUSTOMIZER_WORKSPACE_DIR"
chown -R openclaw:openclaw "$CUSTOMIZER_WORKSPACE_DIR"

if [ ! -d "$JANE_DIR/.git" ]; then
    git clone --no-checkout "$CUSTOMIZER_REPO_URL" "$JANE_DIR" >>"$CUSTOMIZER_LOG" 2>&1
fi

(
    cd "$JANE_DIR" || exit 1

    # Fetch the pinned ref (tag/branch/SHA) and check it out.
    git fetch --depth 1 origin "$CUSTOMIZER_REF" \
        || git fetch --depth 1 origin "refs/tags/$CUSTOMIZER_REF" \
        || git fetch origin "$CUSTOMIZER_REF"

    git checkout -f FETCH_HEAD || git checkout -f "$CUSTOMIZER_REF"

    npm ci
) >>"$CUSTOMIZER_LOG" 2>&1 \
    || CUSTOMIZER_STATUS=$?

CUSTOMIZER_ARGS=(
    init
    -d "$CUSTOMIZER_WORKSPACE_DIR"
    --yes
    --force
    --agent-name "$CUSTOMIZER_AGENT_NAME"
    --owner-name "$CUSTOMIZER_OWNER_NAME"
)

if [ "$CUSTOMIZER_SKIP_QMD" = "true" ]; then
    CUSTOMIZER_ARGS+=(--skip-qmd)
fi
if [ "$CUSTOMIZER_SKIP_CRON" = "true" ]; then
    CUSTOMIZER_ARGS+=(--skip-cron)
fi
if [ "$CUSTOMIZER_SKIP_GIT" = "true" ]; then
    CUSTOMIZER_ARGS+=(--skip-git)
fi
if [ "$CUSTOMIZER_SKIP_HEARTBEAT" = "true" ]; then
    CUSTOMIZER_ARGS+=(--skip-heartbeat)
fi

if [ $CUSTOMIZER_STATUS -eq 0 ]; then
    sudo -u openclaw -H node "$JANE_DIR/bin/janebot-cli.js" "${CUSTOMIZER_ARGS[@]}" \
        >>"$CUSTOMIZER_LOG" 2>&1
    CUSTOMIZER_STATUS=$?
fi

set -e

if [ $CUSTOMIZER_STATUS -ne 0 ]; then
    echo "WARN: janebot-cli customization failed (status=$CUSTOMIZER_STATUS) at $(date); continuing bootstrap" \
        | tee -a "$CUSTOMIZER_LOG"
fi

# Mark as attempted so reboots don't re-run customization.
{
    echo "customizer_repo_url=$CUSTOMIZER_REPO_URL"
    echo "customizer_ref=$CUSTOMIZER_REF"
    echo "workspace_dir=$CUSTOMIZER_WORKSPACE_DIR"
    echo "agent_name=$CUSTOMIZER_AGENT_NAME"
    echo "owner_name=$CUSTOMIZER_OWNER_NAME"
    echo "exit_status=$CUSTOMIZER_STATUS"
    echo "finished_at=$(date -Iseconds)"
} >"$CUSTOMIZER_STATUS_FILE" 2>/dev/null || true

touch "$CUSTOMIZER_MARKER" 2>/dev/null || true
chown openclaw:openclaw "$CUSTOMIZER_MARKER" "$CUSTOMIZER_STATUS_FILE" 2>/dev/null || true
fi

# Create the bot configuration file
echo "=== Creating Bot Configuration ==="
if [ -n "$BOT_CONFIG" ] && [ "$BOT_CONFIG" != "\${BOT_CONFIG}" ]; then
    cat > config.json << EOFCFG
$BOT_CONFIG
EOFCFG
else
    # No inline config provided; service will fetch desired config from control plane.
    echo '{}' > config.json
fi

# Register with control plane
echo "=== Registering with Control Plane ==="
MAX_RETRIES=5
RETRY_COUNT=0

while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    HTTP_CODE=$(curl -s -o /tmp/register_response.json -w "%{http_code}" \
        -X POST \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer $REGISTRATION_TOKEN" \
        -d "{\"bot_id\": \"$BOT_ID\"}" \
        "$CONTROL_PLANE_URL/bot/register" 2>/dev/null || echo "000")
    
    if [ "$HTTP_CODE" = "200" ] || [ "$HTTP_CODE" = "201" ]; then
        echo "Successfully registered with control plane"
        break
    else
        echo "Registration attempt $((RETRY_COUNT + 1)) failed with HTTP $HTTP_CODE, retrying..."
        RETRY_COUNT=$((RETRY_COUNT + 1))
        sleep 10
    fi
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    echo "ERROR: Failed to register with control plane after $MAX_RETRIES attempts"
    # Continue anyway - bot can retry registration later
fi

# Create the main bot runner script
echo "=== Creating Bot Runner ==="
cat > /opt/openclaw/run.sh << 'EOFSCRIPT'
#!/bin/bash
set -e

cd /opt/openclaw

# Load configuration from control plane
CONTROL_PLANE_URL="${CONTROL_PLANE_URL}"
BOT_ID="${BOT_ID}"
REGISTRATION_TOKEN="${REGISTRATION_TOKEN}"

# Function to fetch latest config
fetch_config() {
    local tmp_config="/tmp/latest_config.json"
    local http_code
    http_code=$(curl -s -o "$tmp_config" -w "%{http_code}" \
        -H "Authorization: Bearer $REGISTRATION_TOKEN" \
        "$CONTROL_PLANE_URL/bot/$BOT_ID/config" 2>/dev/null || echo "000")

    if [ "$http_code" != "200" ]; then
        echo "Config fetch failed with HTTP $http_code at $(date)"
        return 1
    fi

    # Only update config when response is valid JSON
    if ! jq -e . "$tmp_config" >/dev/null 2>&1; then
        echo "Config fetch returned invalid JSON at $(date)"
        return 1
    fi

    cp "$tmp_config" config.json
    echo "Updated configuration at $(date)"
    return 0
}

# Function to send heartbeat
send_heartbeat() {
    curl -s -o /dev/null -w "%{http_code}" \
        -X POST \
        -H "Authorization: Bearer $REGISTRATION_TOKEN" \
        "$CONTROL_PLANE_URL/bot/$BOT_ID/heartbeat"
}

# Function to acknowledge config
ack_config() {
    local config_id=$1
    curl -s -o /dev/null -w "%{http_code}" \
        -X POST \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer $REGISTRATION_TOKEN" \
        -d "{\"config_id\": \"$config_id\"}" \
        "$CONTROL_PLANE_URL/bot/$BOT_ID/config_ack"
}

# Main loop
echo "Bot starting at $(date)"

# Fetch initial config
fetch_config || echo "Warning: Could not fetch initial config, using local"

# Start heartbeat and config sync loop
while true; do
    # Send heartbeat every 30 seconds
    HB_RESULT=$(send_heartbeat)
    echo "Heartbeat: HTTP $HB_RESULT at $(date)"
    
    # Try to fetch new config every 2 minutes (every 4th iteration)
    if [ $(($(date +%s) % 120)) -lt 30 ]; then
        if fetch_config; then
            # Extract config ID and acknowledge
            CONFIG_ID=$(jq -r '.id' /tmp/latest_config.json 2>/dev/null || echo "null")
            if [ "$CONFIG_ID" != "null" ] && [ -n "$CONFIG_ID" ]; then
                ack_config "$CONFIG_ID"
            fi
        fi
    fi
    
    sleep 30
done
EOFSCRIPT

chmod +x /opt/openclaw/run.sh

# Create systemd service
echo "=== Creating Systemd Service ==="
cat > /etc/systemd/system/openclaw-bot.service << 'EOFSERVICE'
[Unit]
Description=OpenClaw Bot
After=docker.service network.target
Wants=docker.service

[Service]
Type=simple
User=openclaw
Group=openclaw
WorkingDirectory=/opt/openclaw
Environment="CONTROL_PLANE_URL=${CONTROL_PLANE_URL}"
Environment="BOT_ID=${BOT_ID}"
Environment="REGISTRATION_TOKEN=${REGISTRATION_TOKEN}"
ExecStart=/opt/openclaw/run.sh
Restart=always
RestartSec=10
StandardOutput=append:/var/log/openclaw-bot.log
StandardError=append:/var/log/openclaw-bot.log

[Install]
WantedBy=multi-user.target
EOFSERVICE

# Set proper ownership for /opt/openclaw and log file
echo "=== Setting Permissions ==="
chown -R openclaw:openclaw /opt/openclaw
touch /var/log/openclaw-bot.log
chown openclaw:openclaw /var/log/openclaw-bot.log

# Setup firewall
echo "=== Configuring Firewall ==="
ufw default deny incoming
ufw default allow outgoing
ufw allow ssh
ufw --force enable

# Start the bot service
echo "=== Starting Bot Service ==="
systemctl daemon-reload
systemctl enable openclaw-bot.service
systemctl start openclaw-bot.service

# Create a simple health check endpoint
cat > /opt/openclaw/health.sh << 'EOFHEALTH'
#!/bin/bash
if systemctl is-active --quiet openclaw-bot.service; then
    echo "OK"
    exit 0
else
    echo "ERROR: Bot service not running"
    exit 1
fi
EOFHEALTH
chmod +x /opt/openclaw/health.sh

echo "=== Setup Complete at $(date) ==="
echo "Bot $BOT_ID is now running"
