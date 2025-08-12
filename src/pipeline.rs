use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn, error, instrument, debug};
use crate::types::{RawFrame, Message, Response, CameraSession, ConnId};
use crate::net::tcp::TcpRouter;
use crate::net::udp::UdpSender;
use crate::net::udp::UdpStreamingReceiver;

/// Generate expected device ID from IP address (camXX where XX are last 2 digits)
fn generate_expected_device_id(ip: &str) -> String {
    // Parse IP address and extract last 2 digits from the last octet
    if let Some(last_octet) = ip.split('.').last() {
        if let Ok(num) = last_octet.parse::<u8>() {
            // Extract only the last 2 digits (modulo 100)
            let last_two_digits = num % 100;
            return format!("cam{:02}", last_two_digits);
        }
    }
    
    // Fallback: use hash of IP if parsing fails
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    ip.hash(&mut hasher);
    let hash = hasher.finish();
    format!("cam{:02}", (hash % 100) as u8)
}

/// Worker pool for processing incoming frames
pub struct WorkerPool {
    ingress_rx: mpsc::Receiver<RawFrame>,
    concurrency: Arc<tokio::sync::Semaphore>,
    tcp_router: Arc<TcpRouter>,
    udp_sender: Arc<UdpSender>,
    camera_sessions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, CameraSession>>>,
}

impl WorkerPool {
    pub fn new(
        ingress_rx: mpsc::Receiver<RawFrame>,
        concurrency: Arc<tokio::sync::Semaphore>,
        tcp_router: Arc<TcpRouter>,
        udp_sender: Arc<UdpSender>,
        camera_sessions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, CameraSession>>>,
    ) -> Self {
        Self {
            ingress_rx,
            concurrency,
            tcp_router,
            udp_sender,
            camera_sessions,
        }
    }
    
    /// Run the worker pool
    pub async fn run(mut self) {
        info!("🚀 Starting worker pool");
        
        while let Some(frame) = self.ingress_rx.recv().await {
            // Clone the semaphore for this iteration
            let concurrency = self.concurrency.clone();
            let tcp_router = self.tcp_router.clone();
            let udp_sender = self.udp_sender.clone();
            let camera_sessions = self.camera_sessions.clone();
            
            // Spawn a task to process this frame
            tokio::spawn(async move {
                // Acquire concurrency permit
                let permit = match concurrency.acquire().await {
                    Ok(permit) => permit,
                    Err(_) => {
                        error!("Semaphore closed, shutting down worker task");
                        return;
                    }
                };
                
                if let Err(e) = process_frame(frame, tcp_router, udp_sender, camera_sessions).await {
                    error!("Failed to process frame: {}", e);
                }
                
                drop(permit); // Release concurrency permit
            });
        }
        
        info!("🛑 Worker pool stopped");
    }
}

/// Process a single frame
#[instrument(skip(tcp_router, udp_sender, camera_sessions))]
async fn process_frame(
    frame: RawFrame,
    tcp_router: Arc<TcpRouter>,
    udp_sender: Arc<UdpSender>,
    camera_sessions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, CameraSession>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Enhanced debug logging for incoming frame
    if let Some(conn_id) = frame.conn_id {
        if let Some(addr) = frame.addr {
            debug!("🔄 Processing frame from camera {} ({}): {} bytes", conn_id.0, addr.ip(), frame.bytes.len());
            
            // Log JSON data if available
            if frame.bytes.len() > 20 {
                let json_part = &frame.bytes[20..];
                if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(json_part) {
                    debug!("📋 Processing JSON for camera {}: {}", conn_id.0, serde_json::to_string_pretty(&json_value).unwrap_or_default());
                }
            } else if frame.bytes.len() == 20 {
                debug!("💓 Processing keepalive for camera {}: {:?}", conn_id.0, frame.bytes);
            }
        }
    }
    
    // Check if this is UDP streaming data (large packets, no conn_id)
    if frame.conn_id.is_none() && frame.bytes.len() > 1024 {
        if let Some(addr) = frame.addr {
            // This looks like streaming data
            let streaming_receiver = UdpStreamingReceiver::new(camera_sessions.clone());
            streaming_receiver.process_stream_data(addr, &frame.bytes).await;
            return Ok(());
        }
    }
    
    // Parse message
    let message = match parse_message(&frame.bytes, frame.addr) {
        Ok(msg) => msg,
        Err(e) => {
            warn!("Failed to parse message: {}", e);
            return Ok(());
        }
    };
    
    // Handle message with actual IP address
    let response = handle_message_with_addr(message, frame.addr, frame.conn_id, camera_sessions, Some(tcp_router.clone()), udp_sender.clone()).await?;
    
    // Send response
    if let Some(response_bytes) = serialize_response(&response)? {
        match frame.conn_id {
            Some(conn_id) => {
                // TCP response
                if let Some(addr) = frame.addr {
                    debug!("📤 Sending TCP response to camera {} ({}): {} bytes", conn_id.0, addr.ip(), response_bytes.len());
                }
                tcp_router.send_to_conn(&conn_id, response_bytes).await?;
            }
            None => {
                // UDP response
                if let Some(addr) = frame.addr {
                    debug!("📤 Sending UDP response to {}: {} bytes", addr, response_bytes.len());
                    udp_sender.send_to(addr, &response_bytes).await?;
                }
            }
        }
    }
    
    Ok(())
}

/// Parse raw bytes into a Message
fn parse_message(bytes: &[u8], addr: Option<std::net::SocketAddr>) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
    // Check for 20-byte binary keepalive
    if bytes.len() == 20 {
        return Ok(Message::Keepalive);
    }
    
    // Try to parse as JSON first
    match serde_json::from_slice::<Message>(bytes) {
        Ok(msg) => Ok(msg),
        Err(_) => {
            // Try to parse as binary protocol message
            if let Ok(protocol_msg) = crate::protocol::parse_protocol_message(bytes) {
                // Handle based on the code field
                match protocol_msg.code {
                    100 => {
                        // Registration message
                        if let Some(uid) = protocol_msg.uid {
                            return Ok(Message::Register {
                                device_id: uid,
                                token: protocol_msg.token,
                                ip: "0.0.0.0".to_string(), // We'll get this from the connection
                                port: 6123,
                            });
                        }
                    }
                    12 => {
                        // NAT probe response
                        return Ok(Message::NatProbeResponse {
                            status: protocol_msg.status.unwrap_or(0) as u16,
                            dev_ip: protocol_msg.dev_ip.unwrap_or_default(),
                            dev_port: protocol_msg.dev_port.unwrap_or(0) as u16,
                            dev_nat_ip: protocol_msg.dev_nat_ip.unwrap_or_default(),
                            dev_nat_port: protocol_msg.dev_nat_port.unwrap_or(0) as u16,
                            cli_target: protocol_msg.cli_target.unwrap_or_default(),
                            cli_token: protocol_msg.cli_token.unwrap_or_default(),
                        });
                    }
                    999 => {
                        // Special keepalive code
                        return Ok(Message::Keepalive);
                    }
                    _ => {
                        // Other protocol codes
                        return Ok(Message::Unknown {
                            code: protocol_msg.code as u16,
                            data: bytes.to_vec(),
                        });
                    }
                }
            } else {
                // Try to extract JSON from the message (camera sends binary header + JSON)
                if bytes.len() > 20 {
                    let json_part = &bytes[20..];
                    if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(json_part) {
                        if let Some(code) = json_value["code"].as_i64() {
                            match code {
                                100 => {
                                    // Registration
                                    let uid = json_value["uid"].as_str().unwrap_or("unknown").to_string();
                                    let ip = if let Some(addr) = addr {
                                        addr.ip().to_string()
                                    } else {
                                        "unknown".to_string()
                                    };
                                    let port = addr.map(|a| a.port()).unwrap_or(0);
                                    
                                    return Ok(Message::Register {
                                        device_id: uid,
                                        token: json_value["token"].as_str().map(|s| s.to_string()),
                                        ip,
                                        port,
                                    });
                                }
                                11 => {
                                    // NAT probe request (server to device)
                                    let device_id = json_value["cliTarget"].as_str().unwrap_or("unknown").to_string();
                                    let cli_target = json_value["cliTarget"].as_str().unwrap_or("").to_string();
                                    let cli_token = json_value["cliToken"].as_str().unwrap_or("").to_string();
                                    let cli_nat_port = json_value["cliNatPort"].as_u64().unwrap_or(41234) as u16;
                                    
                                    return Ok(Message::StartStreaming {
                                        device_id,
                                        cli_target,
                                        cli_token,
                                        cli_nat_port,
                                    });
                                }
                                12 => {
                                    // NAT probe response (device to server)
                                    let status = json_value["status"].as_u64().unwrap_or(0) as u16;
                                    let dev_ip = json_value["devIp"].as_str().unwrap_or("").to_string();
                                    let dev_port = json_value["devPort"].as_u64().unwrap_or(0) as u16;
                                    let dev_nat_ip = json_value["devNatIp"].as_str().unwrap_or("").to_string();
                                    let dev_nat_port = json_value["devNatPort"].as_u64().unwrap_or(0) as u16;
                                    let cli_target = json_value["cliTarget"].as_str().unwrap_or("").to_string();
                                    let cli_token = json_value["cliToken"].as_str().unwrap_or("").to_string();
                                    
                                    return Ok(Message::NatProbeResponse {
                                        status,
                                        dev_ip,
                                        dev_port,
                                        dev_nat_ip,
                                        dev_nat_port,
                                        cli_target,
                                        cli_token,
                                    });
                                }
                                20 => {
                                    // UDP probe request (device to server)
                                    return Ok(Message::UdpProbeRequest);
                                }
                                21 => {
                                    // UDP probe response (server to device)
                                    return Ok(Message::Unknown {
                                        code: 21,
                                        data: json_part.to_vec(),
                                    });
                                }
                                51 => {
                                    // Trigger streaming
                                    let device_id = json_value["devTarget"].as_str().unwrap_or("unknown").to_string();
                                    let dev_target = json_value["devTarget"].as_str().unwrap_or("unknown").to_string();
                                    
                                    return Ok(Message::TriggerStreaming {
                                        device_id,
                                        dev_target,
                                    });
                                }
                                999 => {
                                    // Keepalive
                                    return Ok(Message::Keepalive);
                                }
                                _ => {
                                    // Other known codes
                                    return Ok(Message::Unknown {
                                        code: code as u16,
                                        data: json_part.to_vec(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            
            // Unknown message
            Ok(Message::Unknown {
                code: 0,
                data: bytes.to_vec(),
            })
        }
    }
}

/// Handle a parsed message with actual address
async fn handle_message_with_addr(
    message: Message,
    addr: Option<std::net::SocketAddr>,
    conn_id: Option<ConnId>,
    camera_sessions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, CameraSession>>>,
    tcp_router: Option<Arc<TcpRouter>>,
    udp_sender: Arc<UdpSender>,
) -> Result<Response, Box<dyn std::error::Error + Send + Sync>> {
    match message {
        Message::Register { device_id, token, ip, port } => {
            // Use actual IP from connection if available
            let actual_ip = if let Some(addr) = addr {
                addr.ip().to_string()
            } else {
                ip
            };
            
            // Generate expected device ID based on IP (camXX where XX are last 2 digits)
            let expected_device_id = generate_expected_device_id(&actual_ip);
            
            info!("📝 Camera registration: {} (IP: {}) -> expected: {}", device_id, actual_ip, expected_device_id);
            
            let addr: std::net::SocketAddr = format!("{}:{}", actual_ip, port).parse()?;
            
            {
                let mut sessions = camera_sessions.write().await;
                
                // Check if we already have a session for this device ID
                if let Some(existing_session) = sessions.get_mut(&device_id) {
                    // Update existing session with new address and token, but preserve protocol state
                    let protocol_state = existing_session.streaming_protocol_state.clone();
                    let pending_command = existing_session.pending_command.clone();
                    
                    debug!("📝 Updating existing camera session for device {} at IP {} (preserved protocol state: {:?})", device_id, addr.ip(), protocol_state.clone());
                    
                    *existing_session = CameraSession::new(device_id.clone(), addr);
                    existing_session.set_device_id(device_id.clone());
                    existing_session.token = token;
                    existing_session.streaming_protocol_state = protocol_state;
                    existing_session.pending_command = pending_command;
                } else {
                    // Create new session
                    let mut session = CameraSession::new(device_id.clone(), addr);
                    session.set_device_id(device_id.clone());
                    session.token = token;
                    
                    debug!("📝 Created new camera session for device {} at IP {}", device_id, addr.ip());
                    sessions.insert(device_id.clone(), session);
                }
                
                debug!("📝 Camera sessions after registration: {:?}", sessions.keys().collect::<Vec<_>>());
            }
            
            Ok(Response::RegisterSuccess {
                device_id: device_id,
                status: "registered".to_string(),
            })
        }
        
        Message::Keepalive => {
            info!("💓 Keepalive received");
            if let Some(addr) = addr {
                debug!("💓 Keepalive from IP: {}", addr.ip());
                {
                    let mut sessions = camera_sessions.write().await;
                    
                    // Find camera session by IP address
                    let mut found_session = None;
                    let mut found_device_id = None;
                    
                    for (device_id, session) in sessions.iter_mut() {
                        if session.addr.ip() == addr.ip() {
                            found_session = Some(session);
                            found_device_id = Some(device_id.clone());
                            break;
                        }
                    }
                    
                    if let (Some(session), Some(device_id)) = (found_session, found_device_id) {
                        session.update_keepalive();
                        debug!("💓 Updated keepalive for camera {} (IP: {})", device_id, addr.ip());
                        
                        // Check for pending commands
                        debug!("💓 Checking for pending commands for camera {}: {:?}", device_id, session.pending_command);
                        if let Some(command) = session.take_pending_command() {
                            debug!("💓 Found pending command for camera {}: {}", device_id, command);
                            info!("💓 Processing pending command '{}' for camera {} via bounded queue", command, device_id);
                            if command == "code11" {
                                info!("🎬 Sending code 51 command to camera {} (IP: {})", device_id, addr.ip());
                                
                                // Send code 11 (NAT probe request) via TCP
                                if let Some(tcp_router) = &tcp_router {
                                    if let Some(conn_id) = conn_id {
                                        // Create code 11 message with exact field order and camera's actual token
                                        let _camera_token = session.token.clone().unwrap_or_else(|| "91edf41f".to_string());
                                        
                                        // Use compact JSON format to match pcap exactly
                                        let server_ip = "192.168.1.200"; // Server's actual IP
                                        let udp_port = 6123; // Main UDP port from config
                                        let code11_json_string = format!(r#"{{"code":11,"cliTarget":"00112233445566778899aabbccddeeff","cliToken":"deadc0de","cliIp":"255.255.255.255","cliPort":0,"cliNatIp":"{}","cliNatPort":{}}}"#, server_ip, udp_port);
                                        
                                        info!("🎬 Code 11 JSON string: {}", code11_json_string);
                                        
                                        // Parse the JSON string to create the Value for binary encoding
                                        let code11_message: serde_json::Value = serde_json::from_str(&code11_json_string).unwrap();
                                        debug!("🎬 Code 11 JSON payload: {}", serde_json::to_string_pretty(&code11_message).unwrap());
                                        
                                        info!("🎬 Creating Code 11 message with reference format:");
                                        info!("🎬   code: 11");
                                        info!("🎬   cliTarget: 00112233445566778899aabbccddeeff");
                                        info!("🎬   cliToken: 55ABfb77");
                                        info!("🎬   cliIp: 192.168.1.200");
                                        info!("🎬   cliPort: 53221");
                                        info!("🎬   cliNatIp: 192.168.1.200");
                                        info!("🎬   cliNatPort: 41234");
                                        
                                        // Create the binary header + JSON payload using exact header from pcap (195 bytes total)
                                        // Use correct protocol header format like Python script
                                        let json_bytes = code11_json_string.as_bytes();
                                        let json_len = json_bytes.len() as u32;
                                        
                                        // Create protocol header: [len(4), cmd(2), msg_flag(1), deal_flag(1), forward_id(8), pkg_id(4)]
                                        let mut payload = Vec::new();
                                        
                                        // len (4 bytes, little-endian)
                                        payload.extend_from_slice(&json_len.to_le_bytes());
                                        
                                        // cmd (2 bytes, little-endian) - P2P_UDP_CMD_JSON = 0
                                        payload.extend_from_slice(&0u16.to_le_bytes());
                                        
                                        // msg_flag (1 byte) - DEFAULT_MSG_FLAG = 0
                                        payload.push(0);
                                        
                                        // deal_flag (1 byte) - 0
                                        payload.push(0);
                                        
                                        // forward_id (8 bytes) - "00000000"
                                        payload.extend_from_slice(b"00000000");
                                        
                                        // pkg_id (4 bytes, little-endian) - 0
                                        payload.extend_from_slice(&0u32.to_le_bytes());
                                        
                                        // JSON payload
                                        payload.extend_from_slice(json_bytes);
                                        debug!("🎬 Code 11 JSON payload: {}", serde_json::to_string_pretty(&code11_message).unwrap());
                                        info!("🎬 Code 11 JSON string: {}", code11_json_string);
                                        debug!("🎬 Code 11 binary payload ({} bytes): {:?}", payload.len(), payload);
                                        debug!("🎬 Sending code 11 (NAT probe) to camera {} via connection {} with cliTarget: {}", device_id, conn_id.0, device_id);
                                        let _ = tcp_router.send_to_conn(&conn_id, payload).await;
                                        info!("🎬 Code 11 (NAT probe) sent to camera {} via TCP", device_id);
                                        
                                        // Update protocol state
                                        session.streaming_protocol_state = crate::types::StreamingProtocolState::WaitingForNatResponse;
                                        info!("🎬 Updated protocol state to WaitingForNatResponse for camera {}", device_id);
                                        
                                        // Don't send keepalive response after Code 11 - wait for camera's response
                                        return Ok(Response::NatProbeResponseSent);
                                    } else {
                                        warn!("🎬 No connection ID available to send code 11 command");
                                    }
                                } else {
                                    warn!("🎬 No TCP router available to send code 11 command");
                                }
                            }
                        }
                    } else {
                        debug!("💓 No camera session found for IP: {}", addr.ip());
                        // Debug: List all camera sessions
                        let sessions = camera_sessions.read().await;
                        debug!("💓 Available camera sessions: {:?}", sessions.keys().collect::<Vec<_>>());
                        for (device_id, session) in sessions.iter() {
                            debug!("💓 Session {} -> IP: {}", device_id, session.addr.ip());
                        }
                    }
                }
            }
            Ok(Response::KeepaliveResponse)
        }
        
        Message::StartStreaming { device_id, cli_target: _, cli_token: _, cli_nat_port: _ } => {
            info!("🎥 Start streaming for device: {}", device_id);
            
            // Update camera session
            {
                let mut sessions = camera_sessions.write().await;
                if let Some(session) = sessions.get_mut(&device_id) {
                    session.update_keepalive();
                }
            }
            
            Ok(Response::StreamingStarted { device_id })
        }
        
        Message::StopStreaming { device_id } => {
            info!("⏹️ Stop streaming for device: {}", device_id);
            Ok(Response::StreamingStopped { device_id })
        }
        
        Message::TriggerStreaming { device_id, dev_target } => {
            info!("🎬 Trigger streaming for device: {} (target: {})", device_id, dev_target);
            
            // Update camera session and start streaming
            if let Some(addr) = addr {
                let mut sessions = camera_sessions.write().await;
                
                // Find camera session by IP address
                let mut found_session = None;
                let mut found_device_id = None;
                
                for (session_device_id, session) in sessions.iter_mut() {
                    if session.addr.ip() == addr.ip() {
                        found_session = Some(session);
                        found_device_id = Some(session_device_id.clone());
                        break;
                    }
                }
                
                if let (Some(session), Some(session_device_id)) = (found_session, found_device_id) {
                    session.start_streaming(addr);
                    info!("🎬 Started streaming for camera {} from {}", session_device_id, addr);
                } else {
                    warn!("🎬 Camera session not found for IP: {}", addr.ip());
                }
            }
            
            Ok(Response::StreamingTriggered { device_id })
        }
        
        Message::NatProbeResponse { status, dev_ip, dev_port, dev_nat_ip: _, dev_nat_port: _, cli_target: _, cli_token: _ } => {
            info!("📡 Received NAT probe response (code 12) from camera: status={}, dev_ip={}, dev_port={}", status, dev_ip, dev_port);
            
            if let Some(addr) = addr {
                let mut sessions = camera_sessions.write().await;
                
                // Find camera session by IP address
                let mut found_session = None;
                let mut found_device_id = None;
                
                for (device_id, session) in sessions.iter_mut() {
                    if session.addr.ip() == addr.ip() {
                        found_session = Some(session);
                        found_device_id = Some(device_id.clone());
                        break;
                    }
                }
                
                if let (Some(session), Some(device_id)) = (found_session, found_device_id) {
                    // Update protocol state based on current state
                    match session.streaming_protocol_state {
                        crate::types::StreamingProtocolState::WaitingForNatResponse => {
                            session.streaming_protocol_state = crate::types::StreamingProtocolState::WaitingForUdpProbe;
                            info!("🎬 Updated protocol state to WaitingForUdpProbe for camera {}", device_id);
                        }
                        crate::types::StreamingProtocolState::WaitingForFinalNatResponse => {
                            session.streaming_protocol_state = crate::types::StreamingProtocolState::ProtocolComplete;
                            info!("🎬 Updated protocol state to ProtocolComplete for camera {}", device_id);
                            
                            // Now send the streaming control sequence
                            if let Some(tcp_router) = &tcp_router {
                                if let Some(conn_id) = conn_id {
                                    // Send Code 301/298 (Streaming Control) with correct protocol header
                                    let code298_message = serde_json::json!({
                                        "code": 301,
                                        "target": "00112233445566778899aabbccddeeff",
                                        "content": {"code": 298}
                                    });
                                    
                                    let json_bytes = serde_json::to_vec(&code298_message).unwrap();
                                    let json_len = json_bytes.len() as u32;
                                    
                                    let mut payload = Vec::new();
                                    payload.extend_from_slice(&json_len.to_le_bytes());
                                    payload.extend_from_slice(&0u16.to_le_bytes());
                                    payload.push(0);
                                    payload.push(0);
                                    payload.extend_from_slice(b"00000000");
                                    payload.extend_from_slice(&0u32.to_le_bytes());
                                    payload.extend_from_slice(&json_bytes);
                                    
                                    debug!("🎬 Code 301/298 JSON payload: {}", serde_json::to_string_pretty(&code298_message).unwrap());
                                    let _ = tcp_router.send_to_conn(&conn_id, payload).await;
                                    info!("🎬 Code 301/298 (Streaming Control) sent to camera {}", device_id);
                                    
                                    // Send Code 301/4 (Device Info Request) with correct protocol header
                                    let code4_message = serde_json::json!({
                                        "code": 301,
                                        "target": "00112233445566778899aabbccddeeff",
                                        "content": {"unixTimer": 1755023490, "code": 4}
                                    });
                                    
                                    let json_bytes = serde_json::to_vec(&code4_message).unwrap();
                                    let json_len = json_bytes.len() as u32;
                                    
                                    let mut payload = Vec::new();
                                    payload.extend_from_slice(&json_len.to_le_bytes());
                                    payload.extend_from_slice(&0u16.to_le_bytes());
                                    payload.push(0);
                                    payload.push(0);
                                    payload.extend_from_slice(b"00000000");
                                    payload.extend_from_slice(&0u32.to_le_bytes());
                                    payload.extend_from_slice(&json_bytes);
                                    
                                    debug!("🎬 Code 301/4 JSON payload: {}", serde_json::to_string_pretty(&code4_message).unwrap());
                                    let _ = tcp_router.send_to_conn(&conn_id, payload).await;
                                    info!("🎬 Code 301/4 (Device Info Request) sent to camera {}", device_id);
                                    
                                    // Send Code 301/3 (Streaming Start) with correct protocol header
                                    let code3_message = serde_json::json!({
                                        "code": 301,
                                        "target": "00112233445566778899aabbccddeeff",
                                        "content": {"code": 3}
                                    });
                                    
                                    let json_bytes = serde_json::to_vec(&code3_message).unwrap();
                                    let json_len = json_bytes.len() as u32;
                                    
                                    let mut payload = Vec::new();
                                    payload.extend_from_slice(&json_len.to_le_bytes());
                                    payload.extend_from_slice(&0u16.to_le_bytes());
                                    payload.push(0);
                                    payload.push(0);
                                    payload.extend_from_slice(b"00000000");
                                    payload.extend_from_slice(&0u32.to_le_bytes());
                                    payload.extend_from_slice(&json_bytes);
                                    
                                    debug!("🎬 Code 301/3 JSON payload: {}", serde_json::to_string_pretty(&code3_message).unwrap());
                                    let _ = tcp_router.send_to_conn(&conn_id, payload).await;
                                    info!("🎬 Code 301/3 (Streaming Start) sent to camera {}", device_id);
                                    
                                    session.streaming_protocol_state = crate::types::StreamingProtocolState::Streaming;
                                    info!("🎬 Updated protocol state to Streaming for camera {}", device_id);
                                }
                            }
                        }
                        _ => {
                            warn!("🎬 Unexpected NAT probe response in state {:?} for camera {}", session.streaming_protocol_state, device_id);
                        }
                    }
                }
            }
            
            Ok(Response::NatProbeResponseSent)
        }
        
        Message::UdpProbeRequest => {
            info!("📡 Received UDP probe request (code 20) from camera");
            
            if let Some(addr) = addr {
                let mut sessions = camera_sessions.write().await;
                
                // Find camera session by IP address
                let mut found_session = None;
                let mut found_device_id = None;
                
                for (device_id, session) in sessions.iter_mut() {
                    if session.addr.ip() == addr.ip() {
                        found_session = Some(session);
                        found_device_id = Some(device_id.clone());
                        break;
                    }
                }
                
                if let (Some(session), Some(device_id)) = (found_session, found_device_id) {
                    // Check if we're in the right state
                    if session.streaming_protocol_state == crate::types::StreamingProtocolState::WaitingForUdpProbe {
                        // Send UDP probe response (code 21)
                        let code21_message = serde_json::json!({
                            "code": 21,
                            "ip": "192.168.1.200",
                            "port": 6123
                        });
                        
                        let json_bytes = serde_json::to_vec(&code21_message).unwrap();
                        let json_len = json_bytes.len() as u32;
                        
                        let mut payload = Vec::new();
                        payload.extend_from_slice(&json_len.to_le_bytes());
                        payload.extend_from_slice(&0u16.to_le_bytes());
                        payload.push(0);
                        payload.push(0);
                        payload.extend_from_slice(b"00000000");
                        payload.extend_from_slice(&0u32.to_le_bytes());
                        payload.extend_from_slice(&json_bytes);
                        
                        debug!("🎬 Code 21 JSON payload: {}", serde_json::to_string_pretty(&code21_message).unwrap());
                        
                        // Send via UDP
                        if let Err(e) = udp_sender.send_to(addr, &payload).await {
                            warn!("📡 Failed to send UDP probe response: {}", e);
                        } else {
                            info!("📡 UDP probe response (code 21) sent to {}:{}", addr.ip(), addr.port());
                        }
                        
                        session.streaming_protocol_state = crate::types::StreamingProtocolState::WaitingForFinalNatResponse;
                        info!("🎬 Updated protocol state to WaitingForFinalNatResponse for camera {}", device_id);
                    } else {
                        warn!("🎬 UDP probe request received in unexpected state {:?} for camera {}", session.streaming_protocol_state, device_id);
                    }
                }
            }
            
            Ok(Response::UdpProbeResponseSent)
        }
        
        Message::UdpProbeResponse { ip, port } => {
            info!("📡 Received UDP probe response (code 21) from camera: ip={}, port={}", ip, port);
            Ok(Response::UdpProbeResponseSent)
        }
        
        Message::Unknown { code, data } => {
            warn!("❓ Unknown message code: {}, data length: {}", code, data.len());
            Ok(Response::Error {
                code: code,
                message: "Unknown message type".to_string(),
            })
        }
    }
}

/// Serialize response to bytes
fn serialize_response(response: &Response) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    match response {
        Response::RegisterSuccess { device_id: _, status: _ } => {
            let json_response = serde_json::json!({
                "code": 101,
                "status": 200
            });
            let json_bytes = serde_json::to_vec(&json_response)?;
            
            let mut response = vec![0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
            response.extend_from_slice(&json_bytes);
            
            Ok(Some(response))
        }
        Response::KeepaliveResponse => {
            // Correct keepalive response format:
            // [0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00,
            //  0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 
            //  0x00, 0x00, 0x00, 0x00]
            let mut response = vec![0u8; 20];
            response[4] = 0x64; // keepalive code
            response[8] = 0x30; // ASCII '0'
            response[9] = 0x30; // ASCII '0'
            response[10] = 0x30; // ASCII '0'
            response[11] = 0x30; // ASCII '0'
            response[12] = 0x30; // ASCII '0'
            response[13] = 0x30; // ASCII '0'
            response[14] = 0x30; // ASCII '0'
            response[15] = 0x30; // ASCII '0'
            Ok(Some(response))
        }
        Response::StreamingStarted { device_id } => {
            let json_response = serde_json::json!({
                "code": 12,
                "deviceId": device_id,
                "status": "streaming_started"
            });
            let json_bytes = serde_json::to_vec(&json_response)?;
            
            let mut response = vec![0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
            response.extend_from_slice(&json_bytes);
            
            Ok(Some(response))
        }
        Response::StreamingStopped { device_id } => {
            let json_response = serde_json::json!({
                "code": 13,
                "deviceId": device_id,
                "status": "streaming_stopped"
            });
            let json_bytes = serde_json::to_vec(&json_response)?;
            
            let mut response = vec![0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
            response.extend_from_slice(&json_bytes);
            
            Ok(Some(response))
        }
        Response::StreamingTriggered { device_id: _ } => {
            let json_response = serde_json::json!({
                "code": 50
            });
            let json_bytes = serde_json::to_vec(&json_response)?;
            
            let mut response = vec![0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
            response.extend_from_slice(&json_bytes);
            
            Ok(Some(response))
        }
        Response::Error { code, message } => {
            let json_response = serde_json::json!({
                "code": code,
                "error": message
            });
            let json_bytes = serde_json::to_vec(&json_response)?;
            
            let mut response = vec![0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00];
            response.extend_from_slice(&json_bytes);
            
            Ok(Some(response))
        }
        Response::NatProbeResponseSent => {
            // No response needed for NAT probe response sent
            Ok(None)
        }
        Response::UdpProbeResponseSent => {
            // No response needed for UDP probe response sent
            Ok(None)
        }
    }
}
