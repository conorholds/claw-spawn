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

# Create working directories
mkdir -p /opt/openclaw
cd /opt/openclaw

# Create the bot configuration file
echo "=== Creating Bot Configuration ==="
cat > config.json << EOFCFG
$BOT_CONFIG
EOFCFG

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
    curl -s -H "Authorization: Bearer $REGISTRATION_TOKEN" \
        "$CONTROL_PLANE_URL/bot/$BOT_ID/config" > /tmp/latest_config.json
    
    if [ -s /tmp/latest_config.json ]; then
        cp /tmp/latest_config.json config.json
        echo "Updated configuration at $(date)"
        return 0
    fi
    return 1
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
