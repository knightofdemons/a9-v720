#!/bin/bash

echo "Testing Code 51 streaming trigger..."

# Send code 51 command to trigger streaming
echo '{"code": 51, "devTarget": "0800c0064767", "status": 200}' | nc -u 192.168.1.200 6123

echo "Code 51 command sent. Checking for response..."

# Wait a moment and check for response
sleep 2

echo "Checking server logs for streaming activity..."
ssh -i .ssh/a9_camera_key maal@192.168.1.200 "tail -10 /home/maal/a9/server.log | grep -E '(stream|51|50|Trigger)'"

echo "Checking for UDP streaming data..."
ssh -i .ssh/a9_camera_key maal@192.168.1.200 "tcpdump -i any -c 5 'udp and port 6123' 2>/dev/null"

echo "Test complete."
