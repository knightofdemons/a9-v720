use std::net::SocketAddr;
use serde::{Deserialize, Serialize};

/// Streaming protocol state tracking
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingProtocolState {
    /// Initial state - no streaming protocol active
    Idle,
    /// Code 11 (NAT probe) sent, waiting for Code 12 response
    WaitingForNatResponse,
    /// Code 12 received, waiting for Code 20 (UDP probe)
    WaitingForUdpProbe,
    /// Code 20 received, Code 21 sent, waiting for Code 12 final response
    WaitingForFinalNatResponse,
    /// Protocol complete, ready for streaming control
    ProtocolComplete,
    /// Streaming active
    Streaming,
}

/// Raw frame received from network
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub conn_id: Option<ConnId>,
    pub addr: Option<SocketAddr>,
    pub bytes: bytes::Bytes,
}

/// Connection identifier for TCP connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnId(pub u64);

/// Message types for the A9 V720 protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Camera registration message
    Register {
        device_id: String,
        token: Option<String>,
        ip: String,
        port: u16,
    },
    /// Keepalive message (20-byte binary)
    Keepalive,
    /// Start UDP streaming command (code 11)
    StartStreaming {
        device_id: String,
        cli_target: String,
        cli_token: String,
        cli_nat_port: u16,
    },
    /// Stop streaming command
    StopStreaming {
        device_id: String,
    },
    /// Trigger streaming command (code 51)
    TriggerStreaming {
        device_id: String,
        dev_target: String,
    },
    /// NAT probe response (code 12)
    NatProbeResponse {
        status: u16,
        dev_ip: String,
        dev_port: u16,
        dev_nat_ip: String,
        dev_nat_port: u16,
        cli_target: String,
        cli_token: String,
    },
    /// UDP probe request (code 20)
    UdpProbeRequest,
    /// UDP probe response (code 21)
    UdpProbeResponse {
        ip: String,
        port: u16,
    },
    /// Unknown message type
    Unknown {
        code: u16,
        data: Vec<u8>,
    },
}

/// Response types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Registration success
    RegisterSuccess {
        device_id: String,
        status: String,
    },
    /// Keepalive response (20-byte binary)
    KeepaliveResponse,
    /// Streaming started
    StreamingStarted {
        device_id: String,
    },
    /// Streaming stopped
    StreamingStopped {
        device_id: String,
    },
    /// Streaming triggered (code 50 response to code 51)
    StreamingTriggered {
        device_id: String,
    },
    /// NAT probe response sent
    NatProbeResponseSent,
    /// UDP probe response sent
    UdpProbeResponseSent,
    /// Error response
    Error {
        code: u16,
        message: String,
    },
}

/// Camera session information
#[derive(Debug, Clone)]
pub struct CameraSession {
    pub last_keepalive: std::time::Instant,
    pub streaming: bool,
    pub stream_buffer: Vec<u8>,
    pub stream_addr: Option<std::net::SocketAddr>,
    pub pending_command: Option<String>, // Commands from web interface
    pub device_id: Option<String>, // Actual device ID from camera (e.g., "0800c001XPTN")
    pub token: Option<String>, // Token from camera registration
    pub addr: std::net::SocketAddr, // Camera's IP address and port
    pub streaming_protocol_state: StreamingProtocolState, // Track streaming protocol progress
}

impl CameraSession {
    pub fn new(_device_id: String, addr: std::net::SocketAddr) -> Self {
        Self {
            last_keepalive: std::time::Instant::now(),
            streaming: false,
            stream_buffer: Vec::new(),
            stream_addr: None,
            pending_command: None,
            device_id: None,
            token: None,
            addr,
            streaming_protocol_state: StreamingProtocolState::Idle,
        }
    }
    
    pub fn update_keepalive(&mut self) {
        self.last_keepalive = std::time::Instant::now();
    }
    
    pub fn start_streaming(&mut self, addr: std::net::SocketAddr) {
        self.streaming = true;
        self.stream_addr = Some(addr);
        self.stream_buffer.clear();
    }
    
    pub fn stop_streaming(&mut self) {
        self.streaming = false;
        self.stream_addr = None;
        self.stream_buffer.clear();
    }
    
    pub fn add_stream_data(&mut self, data: &[u8]) {
        // Keep buffer size manageable (e.g., last 1MB of stream data)
        const MAX_BUFFER_SIZE: usize = 1024 * 1024;
        
        self.stream_buffer.extend_from_slice(data);
        
        // Trim buffer if it gets too large
        if self.stream_buffer.len() > MAX_BUFFER_SIZE {
            let excess = self.stream_buffer.len() - MAX_BUFFER_SIZE;
            self.stream_buffer.drain(0..excess);
        }
    }
    
    pub fn get_stream_data(&self) -> &[u8] {
        &self.stream_buffer
    }
    
    pub fn take_pending_command(&mut self) -> Option<String> {
        self.pending_command.take()
    }
    
    pub fn set_device_id(&mut self, device_id: String) {
        self.device_id = Some(device_id);
    }
}
