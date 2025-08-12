#!/usr/bin/env python3
import json
import socket
import struct

# Create streaming command
msg = {
    "code": 3,  # CODE_START_STREAMING
    "devTarget": "deadbeef"
}

# Serialize to JSON
json_data = json.dumps(msg).encode('utf-8')

# Create protocol header (18 bytes) - match server's expected format
header = struct.pack('<I', len(json_data))  # Message length (4 bytes)
header += b'\x00'  # Message flag (1 byte)
header += struct.pack('<I', 0)  # Package ID (4 bytes)
header += b'\x00'  # Deal flag (1 byte)
header += b'\x00' * 8  # Forward ID (8 bytes)

# Combine header and data
message = header + json_data

print(f"Message length: {len(message)} bytes")
print(f"JSON data: {json_data}")
print(f"Header length: {len(header)} bytes")
print(f"Full message (hex): {message.hex()}")

# Send to server (which will forward to camera)
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(('192.168.1.200', 6123))
sock.send(message)
print("Streaming command sent to server!")
sock.close()
