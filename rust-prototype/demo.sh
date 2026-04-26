#!/bin/bash

# Shadow Proxy Rust Prototype Demo Script

set -e

echo "=================================="
echo "Shadow Proxy Rust Prototype Demo"
echo "=================================="
echo

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}Error: Rust/Cargo not found. Please install from https://rustup.rs${NC}"
    exit 1
fi

echo -e "${BLUE}Step 1: Building the Shadow Proxy (release mode)${NC}"
cargo build --release
echo

echo -e "${BLUE}Step 2: Starting test servers${NC}"

# Start old server (port 8080)
echo -e "${GREEN}Starting OLD server on port 8080...${NC}"
mkdir -p /tmp/shadow-demo/old
echo '{"version": "1.0.0", "message": "This is the OLD server"}' > /tmp/shadow-demo/old/index.html
(cd /tmp/shadow-demo/old && python3 -m http.server 8080 > /dev/null 2>&1 &)
OLD_PID=$!
echo "OLD server PID: $OLD_PID"

# Start new server (port 8082)
echo -e "${GREEN}Starting NEW server on port 8082...${NC}"
mkdir -p /tmp/shadow-demo/new
echo '{"version": "2.0.0", "message": "This is the NEW server with changes"}' > /tmp/shadow-demo/new/index.html
(cd /tmp/shadow-demo/new && python3 -m http.server 8082 > /dev/null 2>&1 &)
NEW_PID=$!
echo "NEW server PID: $NEW_PID"

# Wait for servers to start
sleep 2

echo
echo -e "${BLUE}Step 3: Starting Shadow Proxy${NC}"
mkdir -p log

# Start shadow proxy in background
SHADOW_PROXY_ADDR="0.0.0.0:8081" \
SHADOW_OLD_SERVERS="http://localhost:8080" \
SHADOW_NEW_SERVERS="http://localhost:8082" \
SHADOW_LOG_FILE="log/demo-results.log" \
RUST_LOG="shadow_proxy=info" \
./target/release/shadow-proxy > log/shadow-proxy.log 2>&1 &
SHADOW_PID=$!
echo "Shadow Proxy PID: $SHADOW_PID"

# Wait for shadow proxy to start
sleep 2

echo
echo -e "${BLUE}Step 4: Sending test requests${NC}"
echo

# Test 1: Simple GET request
echo -e "${GREEN}Test 1: Simple GET request${NC}"
curl -s http://localhost:8081/ | jq .
echo

# Test 2: Request with query parameters
echo -e "${GREEN}Test 2: Request with query parameters${NC}"
curl -s "http://localhost:8081/?user=john&age=30"
echo
echo

# Test 3: Multiple requests to see logging
echo -e "${GREEN}Test 3: Sending 5 rapid requests${NC}"
for i in {1..5}; do
    echo "Request $i:"
    curl -s http://localhost:8081/ | jq -c .
done
echo

echo
echo -e "${BLUE}Step 5: Viewing logs${NC}"
echo

if [ -f "log/demo-results.log" ]; then
    echo -e "${GREEN}Last 3 log entries:${NC}"
    tail -n 3 log/demo-results.log | jq .
    echo
else
    echo -e "${YELLOW}No log file found yet${NC}"
fi

echo
echo -e "${BLUE}Step 6: Performance comparison${NC}"
echo

echo -e "${GREEN}Testing OLD server directly (100 requests):${NC}"
time for i in {1..100}; do
    curl -s http://localhost:8080/ > /dev/null
done

echo
echo -e "${GREEN}Testing through Shadow Proxy (100 requests):${NC}"
time for i in {1..100}; do
    curl -s http://localhost:8081/ > /dev/null
done

echo
echo -e "${BLUE}Demo complete!${NC}"
echo
echo -e "${YELLOW}Servers are still running. To stop them:${NC}"
echo "  kill $OLD_PID $NEW_PID $SHADOW_PID"
echo
echo -e "${YELLOW}To view real-time logs:${NC}"
echo "  tail -f log/demo-results.log | jq ."
echo
echo -e "${YELLOW}To stop all:${NC}"
echo "  kill $OLD_PID $NEW_PID $SHADOW_PID"
echo

# Save PIDs for cleanup
echo "$OLD_PID $NEW_PID $SHADOW_PID" > /tmp/shadow-demo-pids.txt

echo -e "${GREEN}PIDs saved to /tmp/shadow-demo-pids.txt${NC}"
echo
echo "Press Ctrl+C to exit (servers will keep running)"
echo "Or run: kill \$(cat /tmp/shadow-demo-pids.txt)"

# Keep script running to show logs
echo
echo -e "${BLUE}Watching Shadow Proxy logs (Ctrl+C to stop):${NC}"
tail -f log/shadow-proxy.log
