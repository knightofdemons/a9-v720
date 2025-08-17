use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::config::AppConfig;
use crate::types::{CameraManager, ProtocolState};
use crate::protocol::{ProtocolHeader, RegistrationRequest, RegistrationResponse, SnapshotRequest, SnapshotResponse, StreamingRequest, StreamingResponse};
use std::net::IpAddr;
use tokio::sync::Mutex;
use crate::protocol::ForwardCommand;

pub struct TcpRouter {
    config: AppConfig,
    camera_manager: Arc<RwLock<CameraManager>>,
}

impl TcpRouter {
    pub fn new(config: AppConfig, camera_manager: Arc<RwLock<CameraManager>>) -> Self {
        Self {
            config,
            camera_manager,
        }
    }

    pub async fn run(&self, listener: TcpListener) -> Result<()> {
        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    tracing::info!("TCP connection from {}", addr);
                    
                    let camera_manager = self.camera_manager.clone();
                    let config = self.config.clone();
                    
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(socket, addr, camera_manager, config).await {
                            tracing::error!("TCP connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("TCP accept error: {}", e);
                }
            }
        }
    }

    async fn handle_connection(
        mut socket: TcpStream,
        addr: std::net::SocketAddr,
        camera_manager: Arc<RwLock<CameraManager>>,
        config: AppConfig,
    ) -> Result<()> {
        let source_ip = addr.ip();
        
        // Split TCP stream for concurrent read/write
        let (mut read_half, write_half) = socket.into_split();
        
        // Store TCP connection in camera manager
        {
            let mut manager = camera_manager.write().await;
            let camera = manager.get_or_create_camera(source_ip).await;
            let mut camera_guard = camera.write().await;
            camera_guard.tcp_conn = Some(Arc::new(tokio::sync::Mutex::new(write_half)));
            camera_guard.state = ProtocolState::Configuring;
        }

        let mut buffer = [0u8; 4096];
        
        loop {
            match read_half.read(&mut buffer).await {
                Ok(0) => {
                    tracing::info!("TCP connection closed by {}", addr);
                    break;
                }
                Ok(n) => {
                    let data = &buffer[..n];
                    if let Err(e) = Self::process_message(data, source_ip, &camera_manager, &config).await {
                        tracing::error!("Error processing message from {}: {}", source_ip, e);
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("TCP read error from {}: {}", addr, e);
                    break;
                }
            }
        }

        // Clean up connection
        {
            let mut manager = camera_manager.write().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let mut camera_guard = camera.write().await;
                camera_guard.tcp_conn = None;
                camera_guard.state = ProtocolState::Disconnected;
            }
        }
        
        Ok(())
    }

    async fn process_message(
        data: &[u8],
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
        _config: &AppConfig,
    ) -> Result<()> {
        // Parse protocol header
        let (header, payload) = ProtocolHeader::from_bytes(data)?;
        
        tracing::debug!("Received message from {}: CMD={}, length={}", 
                       source_ip, header.cmd, payload.len());
        
        match header.cmd {
            0 | 87 => {
                // JSON message (CMD=0 or CMD=87)
                if let Ok(json_str) = String::from_utf8(payload.to_vec()) {
                    // Strip null bytes from the beginning of the JSON string
                    let clean_json_str = json_str.trim_start_matches('\0');
                    tracing::debug!("Received JSON message from {}: {}", source_ip, clean_json_str);
                    
                    match serde_json::from_str::<serde_json::Value>(&clean_json_str) {
                        Ok(json) => {
                            tracing::debug!("Parsed JSON successfully");
                            if let Some(code) = json["code"].as_u64() {
                                match code {
                                    100 => {
                                        // Registration request
                                        Self::handle_registration(clean_json_str.to_string(), source_ip, camera_manager).await?;
                                    }
                                    12 => {
                                        // NAT probe response
                                        Self::handle_nat_probe_response(clean_json_str.to_string(), source_ip, camera_manager).await?;
                                    }
                                    51 => {
                                        // Device info request - send device info response with Command 51
                                        Self::handle_device_info_request(clean_json_str.to_string(), source_ip, camera_manager).await?;
                                    }
                                    201 => {
                                        // Snapshot request (simple protocol)
                                        Self::handle_snapshot_request(clean_json_str.to_string(), source_ip, camera_manager).await?;
                                    }
                                    301 => {
                                        // Check if this is a forward command echo or actual streaming request
                                        if json.get("target").is_some() && json.get("content").is_some() {
                                            // This is an echoed forward command
                                            if let Some(content) = json.get("content") {
                                                if let Some(code) = content.get("code") {
                                                    if let Some(code_val) = code.as_u64() {
                                                        match code_val {
                                                            298 => {
                                                                // 301/298 (retransmission) - no response expected
                                                                tracing::debug!("Ignoring 301/298 retransmission command from {}: {}", source_ip, clean_json_str);
                                                            }
                                                             4 => {
                                                                // 301/4 (base info) - camera is responding with device info
                                                                tracing::info!("Received 301/4 device info response from {}: {}", source_ip, clean_json_str);
                                                                
                                                                // Now send the streaming command (301/3)
                                                                Self::send_streaming_command(source_ip, camera_manager).await?;
                                                            }
                                                            3 => {
                                                                // 301/3 (streaming) - echoed command
                                                                tracing::info!("Received echoed 301/3 streaming command from {}: {}", source_ip, clean_json_str);
                                                                
                                                                // Now send 301/0 (stop streaming command) to complete the sequence
                                                                Self::send_stop_streaming_command(source_ip, camera_manager).await?;
                                                            }
                                                            0 => {
                                                                // 301/0 (stop streaming) - echoed command
                                                                tracing::info!("Received echoed 301/0 stop streaming command from {}: {}", source_ip, clean_json_str);
                                                                
                                                                // Streaming sequence is now complete!
                                                                tracing::info!("Camera {} streaming sequence complete - video should start on UDP", source_ip);
                                                            }
                                                            _ => {
                                                                tracing::debug!("Ignoring echoed forward command with unknown content code {} from {}: {}", code_val, source_ip, clean_json_str);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        } else if json.get("uid").is_some() {
                                            // This is an actual streaming request
                                            Self::handle_streaming_request(clean_json_str.to_string(), source_ip, camera_manager).await?;
                                        } else {
                                            tracing::debug!("Unknown Code 301 message format from {}: {}", source_ip, clean_json_str);
                                        }
                                    }
                                    _ => {
                                        tracing::debug!("Unhandled JSON message code {} from {}", code, source_ip);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse JSON from {}: {} - Error: {}", source_ip, clean_json_str, e);
                        }
                    }
                }
            }
            99 => {
                // Keepalive message (20 bytes)
                tracing::debug!("Received keepalive from {}", source_ip);
                Self::handle_keepalive(source_ip, camera_manager).await?;
            }
            100 => {
                // Binary heartbeat
                tracing::debug!("Received heartbeat from {}", source_ip);
                Self::handle_heartbeat(source_ip, camera_manager).await?;
            }
            1 | 4 | 6 | 7 => {
                // Video/audio frames - process on TCP
                Self::handle_video_frame_tcp(payload, source_ip, camera_manager).await?;
            }
            _ => {
                tracing::debug!("Unhandled binary message CMD {} from {}", header.cmd, source_ip);
            }
        }
        
        Ok(())
    }

    async fn handle_registration(
        json_str: String,
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        match serde_json::from_str::<RegistrationRequest>(&json_str) {
            Ok(request) => {
                tracing::info!("Registration request from {}: device_id={}", source_ip, request.uid);
            
                // Update camera state
                {
                    let mut manager = camera_manager.write().await;
                    let camera = manager.get_or_create_camera(source_ip).await;
                    let mut camera_guard = camera.write().await;
                    camera_guard.device_id = Some(request.uid.clone());
                    camera_guard.state = ProtocolState::Registering;
                }
                
                            // Send registration response
            let response = RegistrationResponse::new();
            let response_json = serde_json::to_string(&response)?;
            
            let header = ProtocolHeader::json(0, response_json.len());
            
            let mut message = Vec::new();
            message.extend_from_slice(&header.to_bytes());
            message.extend_from_slice(response_json.as_bytes());
            
            // Get TCP connection from camera manager
            {
                let manager = camera_manager.read().await;
                if let Some(camera) = manager.get_camera(source_ip).await {
                    let camera_guard = camera.read().await;
                    if let Some(tcp_conn) = &camera_guard.tcp_conn {
                        let mut socket_guard = tcp_conn.lock().await;
                        socket_guard.write_all(&message).await?;
                        tracing::info!("Registration response sent to {}", source_ip);
                    }
                }
            }
                
                // Update camera state to Idle (simple protocol)
                {
                    let mut manager = camera_manager.write().await;
                    let camera = manager.get_or_create_camera(source_ip).await;
                    let mut camera_guard = camera.write().await;
                    camera_guard.state = ProtocolState::Idle;
                    tracing::info!("Camera {} registered successfully, state set to Idle", source_ip);
                }
            }
            Err(e) => {
                tracing::error!("Failed to parse RegistrationRequest from {}: {} - Error: {}", source_ip, json_str, e);
            }
        }
        
        Ok(())
    }



    async fn handle_snapshot_request(
        json_str: String,
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        match serde_json::from_str::<SnapshotRequest>(&json_str) {
            Ok(request) => {
                tracing::info!("Snapshot request from {}: device_id={}", source_ip, request.uid);
                
                // Send snapshot response (Code 202)
                let response = SnapshotResponse {
                    code: 202,
                    status: 200,
                };
                let response_json = serde_json::to_string(&response)?;
                
                let header = ProtocolHeader::json(0, response_json.len());
                let mut message = Vec::new();
                message.extend_from_slice(&header.to_bytes());
                message.extend_from_slice(response_json.as_bytes());
                
                // Get TCP connection from camera manager
                {
                    let manager = camera_manager.read().await;
                    if let Some(camera) = manager.get_camera(source_ip).await {
                        let camera_guard = camera.read().await;
                        if let Some(tcp_conn) = &camera_guard.tcp_conn {
                            let mut socket_guard = tcp_conn.lock().await;
                            socket_guard.write_all(&message).await?;
                            tracing::info!("Snapshot response (Code 202) sent to {}", source_ip);
                        }
                    }
                }
                
                // Update camera state
                {
                    let mut manager = camera_manager.write().await;
                    let camera = manager.get_or_create_camera(source_ip).await;
                    let mut camera_guard = camera.write().await;
                    camera_guard.state = ProtocolState::Idle;
                    tracing::info!("Camera {} snapshot request handled, state set to Idle", source_ip);
                }
            }
            Err(e) => {
                tracing::error!("Failed to parse SnapshotRequest from {}: {} - Error: {}", source_ip, json_str, e);
            }
        }
        
        Ok(())
    }

    async fn handle_device_info_request(
        json_str: String,
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Device info request (code 51) from {}: {}", source_ip, json_str);
        
        // Parse the code 51 message to get device info
        match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(json) => {
                if let Some(dev_target) = json["devTarget"].as_str() {
                    tracing::info!("Camera {} sending device info with target: {}", source_ip, dev_target);
                }
                if let Some(status) = json["status"].as_u64() {
                    tracing::info!("Camera {} status: {}", source_ip, status);
                }
            }
            Err(e) => {
                tracing::error!("Failed to parse code 51 JSON from {}: {} - Error: {}", source_ip, json_str, e);
            }
        }
        
        // Respond with code 50 (as per STA mode protocol)
        {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let camera_guard = camera.read().await;
                if let Some(tcp_conn) = &camera_guard.tcp_conn {
                    let mut socket_guard = tcp_conn.lock().await;
                    
                    // Create code 50 response (matching working Python script)
                    let code_50_response = serde_json::json!({
                        "code": 50
                    });
                    
                    let json_str = serde_json::to_string(&code_50_response)?;
                    let json_bytes = json_str.as_bytes();
                    
                    // Create protocol header with Command 0 (JSON)
                    let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
                    let message = header.to_bytes();
                    
                    socket_guard.write_all(&message).await?;
                    socket_guard.write_all(json_bytes).await?;
                    tracing::info!("Code 50 response sent to {}: {}", source_ip, json_str);
                    
                    // Update camera state to indicate streaming is ready
                    {
                        let mut manager = camera_manager.write().await;
                        let camera = manager.get_or_create_camera(source_ip).await;
                        let mut camera_guard = camera.write().await;
                        camera_guard.state = ProtocolState::Streaming;
                        tracing::info!("Camera {} code 50/51 exchange complete, streaming should start", source_ip);
                    }
                }
            }
        }
        
        Ok(())
    }

    async fn handle_nat_probe_response(
        json_str: String,
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Received NAT probe response from {}", source_ip);
        
        // NAT traversal is complete - send device status (code 53) and then 301 sequence
        // This follows the exact pattern from the working Python script
        let tcp_conn = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let camera_guard = camera.read().await;
                camera_guard.tcp_conn.clone()
            } else {
                None
            }
        };
        
        if let Some(tcp_conn) = tcp_conn {
            let mut socket_guard = tcp_conn.lock().await;
            
            // Step 1: Send device status (Code 53)
            let device_status_command = serde_json::json!({
                "code": 53,
                "status": 1
            });
            let json_str = serde_json::to_string(&device_status_command)?;
            let json_bytes = json_str.as_bytes();
            let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
            let message = header.to_bytes();
            socket_guard.write_all(&message).await?;
            socket_guard.write_all(json_bytes).await?;
            tracing::info!("Device status (Code 53) sent to {}: {}", source_ip, json_str);
            
            // Step 2: Send 301 sequence (298, 4)
            let code_301_298 = serde_json::json!({
                "code": 301,
                "target": "00112233445566778899aabbccddeeff",
                "content": {
                    "code": 298
                }
            });
            let json_str = serde_json::to_string(&code_301_298)?;
            let json_bytes = json_str.as_bytes();
            let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
            let message = header.to_bytes();
            socket_guard.write_all(&message).await?;
            socket_guard.write_all(json_bytes).await?;
            tracing::info!("Code 301/298 sent to {}: {}", source_ip, json_str);
            
            let code_301_4 = serde_json::json!({
                "code": 301,
                "target": "00112233445566778899aabbccddeeff",
                "content": {
                    "unitTimer": 1755338876,
                    "code": 4
                }
            });
            let json_str = serde_json::to_string(&code_301_4)?;
            let json_bytes = json_str.as_bytes();
            let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
            let message = header.to_bytes();
            socket_guard.write_all(&message).await?;
            socket_guard.write_all(json_bytes).await?;
            tracing::info!("Code 301/4 sent to {}: {}", source_ip, json_str);
        }
        
        // Update camera state (separate lock)
        {
            let mut manager = camera_manager.write().await;
            let camera = manager.get_or_create_camera(source_ip).await;
            let mut camera_guard = camera.write().await;
            camera_guard.state = ProtocolState::Streaming;
            tracing::info!("Camera {} state set to Streaming, sent 53 and 301 sequence", source_ip);
        }
        
        Ok(())
    }



    async fn send_streaming_command(
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Sending streaming command to {} after receiving base info response", source_ip);
        
        let tcp_conn = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let camera_guard = camera.read().await;
                camera_guard.tcp_conn.clone()
            } else {
                None
            }
        };
        
        if let Some(tcp_conn) = tcp_conn {
            let mut socket_guard = tcp_conn.lock().await;
            
            // Send forward streaming command (Code 301 with content code 3)
            let forward_streaming_command = serde_json::json!({
                "code": 301,
                "target": "00112233445566778899aabbccddeeff",
                "content": {
                    "code": 3
                }
            });
            
            let json_str = serde_json::to_string(&forward_streaming_command)?;
            let json_bytes = json_str.as_bytes();
            let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
            let message = header.to_bytes();
            
            socket_guard.write_all(&message).await?;
            socket_guard.write_all(json_bytes).await?;
            tracing::info!("Forward streaming command sent to {}: {}", source_ip, json_str);
        }
        
        Ok(())
    }

    async fn send_stop_streaming_command(
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Sending stop streaming command to {} to complete sequence", source_ip);
        
        let tcp_conn = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let camera_guard = camera.read().await;
                camera_guard.tcp_conn.clone()
            } else {
                None
            }
        };
        
        if let Some(tcp_conn) = tcp_conn {
            let mut socket_guard = tcp_conn.lock().await;
            
            // Send stop streaming command (Code 301 with content code 0)
            let stop_streaming_command = serde_json::json!({
                "code": 301,
                "target": "00112233445566778899aabbccddeeff",
                "content": {
                    "code": 0
                }
            });
            
            let json_str = serde_json::to_string(&stop_streaming_command)?;
            let json_bytes = json_str.as_bytes();
            let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
            let message = header.to_bytes();
            
            socket_guard.write_all(&message).await?;
            socket_guard.write_all(json_bytes).await?;
            tracing::info!("Stop streaming command sent to {}: {}", source_ip, json_str);
        }
        
        Ok(())
    }

    async fn handle_streaming_request(
        json_str: String,
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        match serde_json::from_str::<StreamingRequest>(&json_str) {
            Ok(request) => {
                tracing::info!("Streaming request from {}: device_id={}", source_ip, request.uid);
                
                // Send streaming response (Code 302)
                let response = StreamingResponse {
                    code: 302,
                    status: 200,
                };
                let response_json = serde_json::to_string(&response)?;
                
                let header = ProtocolHeader::json(0, response_json.len());
                let mut message = Vec::new();
                message.extend_from_slice(&header.to_bytes());
                message.extend_from_slice(response_json.as_bytes());
                
                // Get TCP connection from camera manager
                {
                    let manager = camera_manager.read().await;
                    if let Some(camera) = manager.get_camera(source_ip).await {
                        let camera_guard = camera.read().await;
                        if let Some(tcp_conn) = &camera_guard.tcp_conn {
                            let mut socket_guard = tcp_conn.lock().await;
                            socket_guard.write_all(&message).await?;
                            tracing::info!("Streaming response (Code 302) sent to {}", source_ip);
                        }
                    }
                }
                
                // Update camera state to Streaming
                {
                    let mut manager = camera_manager.write().await;
                    let camera = manager.get_or_create_camera(source_ip).await;
                    let mut camera_guard = camera.write().await;
                    camera_guard.state = ProtocolState::Streaming;
                    tracing::info!("Camera {} streaming request handled, state set to Streaming", source_ip);
                }
            }
            Err(e) => {
                tracing::error!("Failed to parse StreamingRequest from {}: {} - Error: {}", source_ip, json_str, e);
            }
        }
        
        Ok(())
    }

    pub async fn start_streaming_for_camera(
        source_ip: IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Starting streaming for camera {}", source_ip);
        
        // Send NAT probe request to initiate NAT traversal
        // Camera needs to complete NAT traversal before responding to forward commands
        {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let camera_guard = camera.read().await;
                if let Some(tcp_conn) = &camera_guard.tcp_conn {
                    let mut socket_guard = tcp_conn.lock().await;
                    
                    // Send NAT probe request: {"code": 11, "cliTarget": "00112233445566778899aabbccddeeff", "cliToken": "deadc0de", ...}
                    // This initiates NAT traversal (Code 11 = CODE_S2D_NAT_REQ)
                    let nat_probe_command = serde_json::json!({
                        "code": 11,
                        "cliTarget": "00112233445566778899aabbccddeeff",
                        "cliToken": "deadc0de",
                        "cliIp": "255.255.255.255",
                        "cliPort": 0,
                        "cliNatIp": "192.168.1.99",
                        "cliNatPort": 6123
                    });
                    
                    let json_str = serde_json::to_string(&nat_probe_command)?;
                    let json_bytes = json_str.as_bytes();
                    
                    // Create protocol header: cmd=0, length, msg_flag=0, pkg_id=0
                    let header = ProtocolHeader::new(0, json_bytes.len() as u32, 0, 0);
                    let message = header.to_bytes();
                    
                    socket_guard.write_all(&message).await?;
                    socket_guard.write_all(json_bytes).await?;
                    tracing::info!("NAT probe request sent to {}: {}", source_ip, json_str);
                }
            }
        }
        
        // Update camera state to Streaming (camera will send Code 301 request after NAT traversal)
        {
            let mut manager = camera_manager.write().await;
            let camera = manager.get_or_create_camera(source_ip).await;
            let mut camera_guard = camera.write().await;
            camera_guard.state = ProtocolState::Streaming;
            // Reset first_retransmission_sent flag when starting streaming
            camera_guard.first_retransmission_sent = false;
            tracing::info!("Camera {} state set to Streaming, first_retransmission_sent reset to false", source_ip);
        }
        
        Ok(())
    }

    pub async fn stop_streaming_for_camera(
        source_ip: IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Stopping streaming for camera {}", source_ip);
        
        // Close all UDP connections for this camera
        {
            let mut manager = camera_manager.write().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let mut camera_guard = camera.write().await;
                
                // Close all UDP sockets
                camera_guard.udp_ports.clear();
                tracing::info!("Closed {} UDP connections for camera {}", 
                    camera_guard.udp_ports.len(), source_ip);
                
                // Clear video buffer
                camera_guard.stream_buffer.clear();
                tracing::info!("Cleared video buffer for camera {}", source_ip);
                
                // Set state back to Idle
                camera_guard.state = ProtocolState::Idle;
                tracing::info!("Camera {} state set to Idle", source_ip);
            }
        }
        

        
        Ok(())
    }



    async fn handle_heartbeat(
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        // Update camera heartbeat
        {
            let mut manager = camera_manager.write().await;
            let camera = manager.get_or_create_camera(source_ip).await;
            let mut camera_guard = camera.write().await;
            camera_guard.update_heartbeat();
        }
        
        Ok(())
    }

    async fn handle_keepalive(
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        // Send keepalive response (20 bytes)
        let header = ProtocolHeader::binary(99, 0, 0);
        let message = header.to_bytes();
        
        // Get TCP connection from camera manager
        {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.get_camera(source_ip).await {
                let camera_guard = camera.read().await;
                if let Some(tcp_conn) = &camera_guard.tcp_conn {
                    let mut socket_guard = tcp_conn.lock().await;
                    socket_guard.write_all(&message).await?;
                    tracing::debug!("Keepalive response sent to {}", source_ip);
                }
            }
        }
        
        Ok(())
    }



    pub async fn trigger_snapshot_for_camera(
        source_ip: IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Triggering snapshot for camera {}", source_ip);
        
        // Update camera state to trigger snapshot (camera will send Code 201 request)
        {
            let mut manager = camera_manager.write().await;
            let camera = manager.get_or_create_camera(source_ip).await;
            let mut camera_guard = camera.write().await;
            camera_guard.state = ProtocolState::Idle;
            tracing::info!("Camera {} snapshot triggered, waiting for Code 201 request", source_ip);
        }
        
        Ok(())
    }

    async fn handle_video_frame_tcp(
        frame_payload: &[u8],
        source_ip: std::net::IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
    ) -> Result<()> {
        tracing::info!("Processing TCP video frame from {} with {} bytes", source_ip, frame_payload.len());

        // Get camera and update heartbeat
        let camera = {
            let mut manager = camera_manager.write().await;
            manager.get_or_create_camera(source_ip).await
        };
        let mut camera_guard = camera.write().await;
        camera_guard.update_heartbeat();
        
        // Add frame to camera's buffer (simplified for TCP - no fragmentation needed)
        // Since TCP is reliable, we can assume the frame is complete
        let frame_complete = camera_guard.stream_buffer.add_fragment(
            1, // cmd (video frame)
            0, // msg_flag
            0, // pkg_id (not used for TCP)
            frame_payload
        );
        
        if frame_complete {
            tracing::info!("Complete TCP frame added to buffer for {}: {} bytes (buffer: {}/{} frames)", 
                source_ip, frame_payload.len(), 
                camera_guard.stream_buffer.frame_count(), 
                camera_guard.stream_buffer.max_frames);
        }
        
        Ok(())
    }
}
