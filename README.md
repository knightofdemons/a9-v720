# A9 V720 Server

A Rust implementation of the A9 V720 camera server protocol, designed to handle camera registration, NAT traversal, and command mode operations.

## Overview

This server implements the A9 V720 protocol as documented in `fake_server.md`, providing:

- HTTP API for camera registration and server configuration
- TCP/UDP protocol handling for camera communication
- NAT traversal support
- Command mode operations
- Comprehensive logging and debugging

## Architecture

The server runs three main components:

1. **HTTP Server (Port 80)**: Handles camera registration and server configuration requests
2. **TCP Server (Port 6123)**: Manages camera protocol communication
3. **UDP Server (Port 6124)**: Handles NAT traversal and data streaming

## Protocol Flow

### 1. HTTP Registration (Optional)
- Camera registers via `/app/api/ApiSysDevicesBatch/registerDevices`
- Camera confirms registration via `/app/api/ApiSysDevicesBatch/confirm`
- Camera gets server configuration via `/app/api/ApiServer/getA9ConfCheck`

### 2. TCP Connection
- Camera connects to TCP port 6123
- Camera sends registration message (code 100)
- Server responds with registration confirmation (code 101)
- Server sends NAT request (code 11)

### 3. UDP Handshake
- Camera connects to UDP port 6124
- Camera sends UDP request (code 20)
- Server responds with UDP response (code 21)

### 4. NAT Completion
- Camera sends NAT response (code 12) via TCP
- Server updates camera session state

### 5. Probe and Command Mode
- Camera sends probe request (code 50)
- Server responds with probe response (code 51)
- Camera enters command mode

## Installation

### Prerequisites

- Rust (will be installed automatically if not present)
- Debian/Ubuntu server
- SSH access to server

### Deployment

1. **Local Setup**:
   ```bash
   # Make deployment script executable
   chmod +x deploy.sh
   
   # Run deployment
   ./deploy.sh
   ```

2. **Manual Deployment**:
   ```bash
   # Transfer files to server
   scp -i .ssh/a9_camera_key -r ./* maal@192.168.0.253:/home/maal/a9/
   
   # SSH to server and build
   ssh -i .ssh/a9_camera_key maal@192.168.0.253
   cd /home/maal/a9
   cargo build --release
   
   # Install and start service
   sudo cp a9-v720-server.service /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo systemctl enable a9-v720-server.service
   sudo systemctl start a9-v720-server.service
   ```

## Configuration

### Server Configuration

The server is configured to run on:
- **HTTP**: 0.0.0.0:80
- **TCP**: 0.0.0.0:6123
- **UDP**: 0.0.0.0:6124
- **Host IP**: 192.168.1.200 (for camera connections)

### Environment Variables

- `RUST_LOG`: Log level (default: info)
- Set in systemd service file

## API Endpoints

### Device Registration
```
POST /app/api/ApiSysDevicesBatch/registerDevices?batch={batch}&random={random}&token={token}
```

Response:
```json
{
  "code": 200,
  "message": "操作成功",
  "data": "device_code"
}
```

### Device Confirmation
```
POST /app/api/ApiSysDevicesBatch/confirm?devices_code={code}&random={random}&token={token}
```

Response:
```json
{
  "code": 200,
  "message": "操作成功",
  "data": null
}
```

### Server Configuration
```
POST /app/api/ApiServer/getA9ConfCheck?devices_code={code}&random={random}&token={token}
```

Response:
```json
{
  "code": 200,
  "message": "操作成功",
  "data": {
    "tcp_port": 6123,
    "uid": "device_code",
    "is_bind": "8",
    "domain": "v720.naxclow.com",
    "update_url": null,
    "host": "192.168.1.200",
    "curr_time": "timestamp",
    "pwd": "password",
    "version": null
  }
}
```

## Protocol Messages

### Registration (Code 100)
```json
{
  "code": 100,
  "uid": "device_id",
  "token": "password",
  "domain": "v720.naxclow.com"
}
```

### Registration Response (Code 101)
```json
{
  "code": 101,
  "status": 200
}
```

### NAT Request (Code 11)
```json
{
  "code": 11,
  "cli_target": "uuid",
  "cli_token": "token",
  "cli_ip": "192.168.1.200",
  "cli_port": 6124,
  "cli_nat_ip": "192.168.1.200",
  "cli_nat_port": 6124
}
```

### UDP Request (Code 20)
```json
{
  "code": 20
}
```

### UDP Response (Code 21)
```json
{
  "code": 21,
  "ip": "192.168.1.200",
  "port": 6124
}
```

### NAT Response (Code 12)
```json
{
  "code": 12,
  "status": 200,
  "dev_ip": "camera_ip",
  "dev_port": 6124,
  "dev_nat_ip": "camera_nat_ip",
  "dev_nat_port": 6124,
  "cli_target": "uuid",
  "cli_token": "token"
}
```

### Probe Request (Code 50)
```json
{
  "code": 50
}
```

### Probe Response (Code 51)
```json
{
  "code": 51,
  "dev_target": "device_id",
  "status": 200
}
```

## Monitoring

### Service Status
```bash
sudo systemctl status a9-v720-server.service
```

### Logs
```bash
# Follow logs
sudo journalctl -u a9-v720-server.service -f

# Recent logs
sudo journalctl -u a9-v720-server.service --no-pager -n 50
```

### Camera Sessions
The server maintains active camera sessions with:
- Device ID
- IP address
- Protocol state
- NAT information
- Last seen timestamp

## Testing

### Test with Camera
1. Configure camera to connect to 192.168.1.200
2. Monitor server logs for connection events
3. Verify protocol flow completion

### Manual Testing
```bash
# Test HTTP endpoints
curl -X POST "http://192.168.1.200/app/api/ApiSysDevicesBatch/registerDevices?batch=TEST&random=ABC123&token=test123"

# Test TCP connection
nc 192.168.1.200 6123

# Test UDP connection
nc -u 192.168.1.200 6124
```

## Troubleshooting

### Common Issues

1. **Service won't start**:
   - Check logs: `sudo journalctl -u a9-v720-server.service`
   - Verify port availability: `sudo netstat -tlnp | grep :6123`

2. **Camera connection issues**:
   - Verify firewall settings
   - Check network connectivity
   - Monitor server logs for protocol errors

3. **Build errors**:
   - Ensure Rust is installed: `rustc --version`
   - Update dependencies: `cargo update`

### Debug Mode
Set log level to debug:
```bash
sudo systemctl edit a9-v720-server.service
# Add: Environment=RUST_LOG=debug
sudo systemctl restart a9-v720-server.service
```

## Development

### Project Structure
```
src/
├── main.rs          # Main server implementation
├── protocol.rs      # Protocol message handling
└── camera_session.rs # Camera session management
```

### Building Locally
```bash
cargo build
cargo test
cargo run
```

### Adding Features
1. Implement new protocol codes in `protocol.rs`
2. Add handlers in `main.rs`
3. Update session management in `camera_session.rs`
4. Add tests and documentation

## License

This project is part of the A9 camera system implementation.

## Support

For issues and questions, refer to:
- `fake_server.md` for protocol documentation
- `TODO.md` for implementation status
- `setup-local.md` for deployment instructions


