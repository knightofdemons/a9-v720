#!/bin/bash

# A9 V720 Server Deployment Script
# This script deploys the latest build to the Debian server

set -e  # Exit on any error

echo "🚀 Starting A9 V720 Server deployment..."

# Configuration
SERVER_USER="maal"
SERVER_IP="192.168.0.253"
SERVER_PATH="/home/maal/a9"
SSH_KEY=".ssh/a9_camera_key"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if SSH key exists
if [ ! -f "$SSH_KEY" ]; then
    print_error "SSH key not found: $SSH_KEY"
    exit 1
fi

# Step 1: Archive current server files
print_status "Archiving current server files..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "mkdir -p $SERVER_PATH/../archive/server/$(date +%Y-%m-%d_%H-%M-%S)"
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "cp -r $SERVER_PATH/* $SERVER_PATH/../archive/server/$(date +%Y-%m-%d_%H-%M-%S)/ 2>/dev/null || true"

# Step 2: Stop the service
print_status "Stopping A9 V720 server service..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo systemctl stop a9-v720-server.service || true"

# Step 3: Clean server directory
print_status "Cleaning server directory..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "rm -rf $SERVER_PATH/*"

# Step 4: Transfer files to server
print_status "Transferring files to server..."
scp -i "$SSH_KEY" -r ./* "$SERVER_USER@$SERVER_IP:$SERVER_PATH/"

# Step 5: Build the project
print_status "Building project on server..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "cd $SERVER_PATH && cargo build --release"

# Step 6: Install the binary
print_status "Installing binary..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo cp $SERVER_PATH/target/release/a9-v720-server /usr/local/bin/"

# Step 7: Install and start service
print_status "Installing and starting service..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo cp $SERVER_PATH/a9-v720-server.service /etc/systemd/system/"
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo systemctl daemon-reload"
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo systemctl enable a9-v720-server.service"
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo systemctl start a9-v720-server.service"

# Step 8: Check service status
print_status "Checking service status..."
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo systemctl status a9-v720-server.service --no-pager"

# Step 9: Show recent logs
print_status "Recent service logs:"
ssh -i "$SSH_KEY" "$SERVER_USER@$SERVER_IP" "sudo journalctl -u a9-v720-server.service --no-pager -n 10"

print_status "✅ Deployment completed successfully!"
print_status "To monitor logs: ssh -i $SSH_KEY $SERVER_USER@$SERVER_IP 'sudo journalctl -u a9-v720-server.service -f'"
