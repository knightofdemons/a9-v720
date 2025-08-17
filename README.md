# A9 V720 Camera Server

A Rust-based server implementation to imitate and replace the original server for the A9 V720 camera model in STA (Station) mode. This project was developed using **Cursor** for AI-assisted pair programming.

## Overview

This server implements the proprietary communication protocol used by the A9 V720 camera, enabling:
- Camera registration and NAT traversal
- Video streaming with JPEG frame reassembly
- Audio streaming (PCM)
- Web interface for camera management
- Retransmission confirmation system for reliable streaming

## Features

### ✅ Implemented
- **TCP Protocol Handler**: Registration, NAT traversal, streaming control
- **UDP Protocol Handler**: Video/audio streaming, heartbeat management
- **JPEG Frame Reassembly**: Handles fragmented video frames (MSG_FLAG 250/251/252)
- **Retransmission System**: Implements CMD 605 confirmations following Python reference
- **Web Interface**: Camera management and live stream viewing
- **Systemd Service**: Production deployment on Debian servers
- **Multi-camera Support**: Concurrent camera connections

### 🔧 Technical Details
- **Language**: Rust with Tokio async runtime
- **Protocol**: Custom binary protocol with JSON payloads
- **Video Format**: MJPEG streaming with frame fragmentation
- **Audio Format**: PCM audio (cmd=6)
- **Ports**: TCP 6123 (protocol), Web 1234 (interface), UDP dynamic (streaming)

## Architecture

```
src/
├── main.rs              # Application entry point
├── config.rs            # Configuration management
├── types.rs             # Data structures and camera management
├── protocol/
│   ├── binary.rs        # Binary protocol header handling
│   └── messages.rs      # JSON message structures
├── router/
│   ├── tcp.rs          # TCP connection and protocol handling
│   └── udp.rs          # UDP streaming and retransmission
└── web/
    ├── server.rs        # Web server and HTML interface
    └── camera_endpoints.rs # REST API endpoints
```

## Protocol Flow

### STA Mode Connection Sequence
1. **Registration**: Camera registers with device info
2. **NAT Traversal**: Code 11/21 exchange for UDP port assignment
3. **Probe Exchange**: Code 50/51 exchange (3 iterations)
4. **Streaming Initiation**: Code 53, 301/298, 301/4, 301/3, 301/0 sequence
5. **Video Streaming**: UDP-based with retransmission confirmations

### Retransmission System
- **First End Frame**: Send empty CMD 605 confirmation
- **Subsequent Frames**: Send batch CMD 605 with collected package IDs
- **Bucket System**: Collects package IDs between end frames
- **Timing**: Immediate response on end frames (no periodic timer)

## Installation & Deployment

### Prerequisites
- Rust toolchain (rustup)
- Debian/Ubuntu server
- SSH access to camera network

### Build & Deploy
```bash
# Build release version
cargo build --release

# Deploy to Debian server
scp -i .ssh/a9_camera_key target/release/a9-v720-server maal@192.168.0.253:/home/maal/
scp -i .ssh/a9_camera_key a9-v720-server.service maal@192.168.0.253:/etc/systemd/system/

# Start service
sudo systemctl enable a9-v720-server.service
sudo systemctl start a9-v720-server.service
```

### Configuration
Edit `config.toml` or set environment variables:
```toml
server_ip = "192.168.1.99"
tcp_port = 6123
web_port = 1234
```

## API Endpoints

### Camera Management
- `GET /api/cameras` - List connected cameras
- `GET /api/cameras/{device_id}/streaming/start` - Start streaming
- `GET /api/cameras/{device_id}/streaming/stop` - Stop streaming
- `GET /api/cameras/{device_id}/debug` - Debug buffer info

### Web Interface
- `GET /` - Main camera management interface
- `GET /stream/{device_id}` - Live video stream (MJPEG)

## Development

### Key Implementation Details

#### Frame Reassembly
```rust
// Handles JPEG fragmentation
match (cmd, msg_flag) {
    (1, 250) => start_new_frame(payload),      // Start frame
    (1, 251) => add_frame_fragment(payload),   // Continuation
    (1, 252) => complete_frame(payload),       // End frame
    (6, 255) => handle_audio_frame(payload),   // PCM audio
}
```

#### Retransmission Confirmation
```rust
// CMD 605 format: [total_len][cmd][device_id][package_ids...]
let total_length: u32 = 4 + 8 + (packages.len() * 4);
message.extend_from_slice(&total_length.to_le_bytes());
message.extend_from_slice(&605u32.to_le_bytes());
message.extend_from_slice(b"00000000"); // Device ID
```

### Debugging
- Check logs: `sudo journalctl -u a9-v720-server.service -f`
- Monitor retransmissions: `grep -E "(First end frame|End frame received)"`
- Verify frame assembly: `grep -E "(Successfully assembled JPEG frame)"`

## Network Analysis

The project includes network capture analysis tools:
- `docs/repo/working_python_script.pcap` - Reference STA mode traffic
- `docs/repo/fake_server.md` - Protocol documentation
- `docs/repo/v720_sta.py` - Python reference implementation

## Development History

This project was developed using **Cursor** for AI-assisted pair programming, enabling rapid protocol reverse engineering and implementation. Key milestones:

- **Protocol Analysis**: Reverse engineered from network captures
- **Frame Reassembly**: Implemented JPEG fragmentation handling
- **Retransmission System**: Matched Python reference behavior
- **Deadlock Prevention**: Resolved nested lock issues
- **Production Deployment**: Systemd service integration

## License

This project is developed for educational and research purposes to understand and implement the A9 V720 camera protocol.

## Contributing

This is a research project focused on protocol implementation. Contributions should focus on:
- Protocol accuracy and reliability
- Performance optimization
- Documentation improvements
- Bug fixes and stability

---

*Developed with ❤️ and **Cursor** AI assistance*
