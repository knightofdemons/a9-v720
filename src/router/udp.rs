use crate::{
    config::AppConfig,
    types::{CameraManager, ProtocolState, StreamBuffer, ProbeState},
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use crate::protocol::binary::ProtocolHeader;
use anyhow::Result;
use rand::Rng;

pub struct UdpRouter;

impl UdpRouter {
    pub async fn start(
        socket: UdpSocket,
        camera_manager: Arc<RwLock<CameraManager>>,
        config: AppConfig,
    ) -> Result<()> {
        let local_addr = socket.local_addr()?;
        tracing::info!("UDP router started on {}", local_addr);
        
        // Wrap socket in Arc for sharing between tasks
        let socket = Arc::new(socket);
        let socket_clone = socket.clone();
        let camera_manager_clone = camera_manager.clone();
        
        // Spawn periodic retransmission confirmation task
        tokio::spawn(async move {
            Self::periodic_retransmission_task(socket_clone, camera_manager_clone).await;
        });

        // Spawn periodic incomplete frame completion task
        let camera_manager_clone = camera_manager.clone();
        tokio::spawn(async move {
            Self::periodic_incomplete_frame_task(camera_manager_clone).await;
        });
        
        let mut buffer = [0u8; 4096];
        
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((n, addr)) => {
                    let data = &buffer[..n];
                    tracing::debug!("UDP message from {}: {} bytes", addr, n);
                    
                    if let Err(e) = Self::process_message(data, addr, &camera_manager, &config, &socket, local_addr.port()).await {
                        tracing::error!("Error processing UDP message from {}: {}", addr, e);
                    }
                }
                Err(e) => {
                    tracing::error!("UDP receive error: {}", e);
                }
            }
        }
    }

    async fn process_message(
        data: &[u8],
        addr: SocketAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
        config: &AppConfig,
        socket: &Arc<UdpSocket>,
        local_port: u16,
    ) -> Result<()> {
        tracing::debug!("Processing UDP message from {}: {} bytes on port {}", addr, data.len(), local_port);
        
        if data.len() < ProtocolHeader::SIZE {
            tracing::warn!("UDP message too short from {}: {} bytes", addr, data.len());
            return Ok(());
        }

        let (header, payload) = ProtocolHeader::from_bytes(data)?;
        
        tracing::info!("UDP message from {}: cmd={}, payload_len={} on port {}", addr.ip(), header.cmd, payload.len(), local_port);

        match header.cmd {
            0 => {
                // UDP probe
                Self::handle_udp_probe(addr, camera_manager, config, socket).await?;
            }
            100 => {
                // UDP heartbeat - respond with retransmission confirmation
                Self::handle_udp_heartbeat(addr, camera_manager, socket).await?;
            }
            102 => {
                // UDP keepalive (alternative format)
                Self::handle_udp_keepalive(addr, camera_manager, socket).await?;
            }
            1 | 4 | 6 | 7 => {
                // Video/audio frames - process on any port
                Self::handle_video_frame(addr, data, camera_manager, socket, local_port).await?;
            }
            51 => {
                // Code 51 response
                Self::handle_code51_response(addr, payload, camera_manager, socket).await?;
            }
            _ => {
                // Check if this is a 20-byte UDP keepalive message (raw data, no JSON)
                if data.len() == 20 && header.cmd == 0 && header.msg_flag == 0 {
                    tracing::debug!("Received 20-byte UDP keepalive from {}:{}", addr.ip(), addr.port());
                    Self::handle_raw_udp_keepalive(addr, socket).await?;
                } else {
                    tracing::debug!("Unknown UDP command from {}: cmd={}", addr.ip(), header.cmd);
                }
            }
        }
        
        Ok(())
    }

    async fn handle_video_frame(
        addr: SocketAddr,
        data: &[u8],
        camera_manager: &Arc<RwLock<CameraManager>>,
        socket: &Arc<UdpSocket>,
        local_port: u16,
    ) -> Result<()> {
        let source_ip = addr.ip();
        
        tracing::info!("Processing video frame from {}:{} with {} bytes on port {}", source_ip, addr.port(), data.len(), local_port);
        
        // Parse the protocol header to get frame information
        if data.len() < ProtocolHeader::SIZE {
            tracing::warn!("Video frame too short from {}: {} bytes", source_ip, data.len());
            return Ok(());
        }

        let (header, frame_payload) = match ProtocolHeader::from_bytes(data) {
            Ok((header, payload)) => (header, payload),
            Err(e) => {
                tracing::error!("Failed to parse video frame header from {}: {}", source_ip, e);
                return Ok(());
            }
        };

        tracing::info!("Video frame header: cmd={}, msg_flag={}, pkg_id={}, payload_len={}", 
            header.cmd, header.msg_flag, header.pkg_id, frame_payload.len());

        // Debug: Show first few bytes of payload to check for JPEG magic bytes
        if frame_payload.len() >= 4 {
            let first_bytes: Vec<String> = frame_payload[..4.min(frame_payload.len())]
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect();
            tracing::debug!("Frame payload starts with: {}", first_bytes.join(" "));
        }

        // Debug: Show raw data bytes for comparison
        if data.len() >= 24 {
            let raw_bytes: Vec<String> = data[..24.min(data.len())]
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect();
            tracing::debug!("Raw data starts with: {}", raw_bytes.join(" "));
        }

        // Get camera and update heartbeat
        let camera = {
            let mut manager = camera_manager.write().await;
            manager.get_or_create_camera(source_ip).await
        };
        let mut camera_guard = camera.write().await;
        camera_guard.update_heartbeat();
        
        // Track the UDP port the camera is using
        camera_guard.udp_ports.insert(addr.port(), 1);
        
        // Add frame to camera's buffer
        let frame_complete = camera_guard.stream_buffer.add_fragment(
            header.cmd,
            header.msg_flag,
            header.pkg_id,
            frame_payload
        );
        
        // Handle retransmission logic directly here to avoid deadlocks
        // Add package ID to retransmission bucket
        camera_guard.add_to_retransmission_bucket(header.pkg_id);
        tracing::debug!("Added pkg_id {} to retransmission bucket for {}", header.pkg_id, source_ip);

        if header.msg_flag == 252 {
            // End frame received
            if !camera_guard.first_retransmission_sent {
                // First end frame - send empty retransmission confirmation
                camera_guard.first_retransmission_sent = true;
                tracing::info!("First end frame received, sending empty retransmission confirmation to {}", source_ip);
                
                // Send empty retransmission confirmation
                if let Err(e) = Self::send_empty_retransmission_confirmation(
                    source_ip, camera_manager, socket
                ).await {
                    tracing::error!("Failed to send first empty retransmission confirmation to {}: {}", source_ip, e);
                }
            } else {
                // Subsequent end frame - send batch retransmission with collected package IDs
                let packages = camera_guard.get_and_clear_retransmission_bucket();
                if !packages.is_empty() {
                    tracing::info!("End frame received, sending batch retransmission with {} package IDs to {}", packages.len(), source_ip);
                    
                    // Send batch retransmission confirmation
                    if let Err(e) = Self::send_batch_retransmission_confirmation(
                        source_ip, &packages, camera_manager, socket
                    ).await {
                        tracing::error!("Failed to send batch retransmission confirmation to {}: {}", source_ip, e);
                    }
                } else {
                    tracing::warn!("End frame received but retransmission bucket is empty for {}", source_ip);
                }
            }
        }
        
        if frame_complete {
            // Only log this for end frames (MSG_FLAG=252) when a complete frame is assembled
            if header.msg_flag == 252 {
                tracing::info!("Complete frame assembled and added to buffer for {}: {} bytes (buffer: {}/{} frames)", 
                    source_ip, frame_payload.len(), 
                    camera_guard.stream_buffer.frame_count(), 
                    camera_guard.stream_buffer.max_frames);
            }
        }
        
        // Add package ID to the camera's received packages list for batch retransmission confirmation
        // Track ALL package IDs (both cmd=1 video and cmd=6 audio) like the Python script
        camera_guard.add_received_package(header.pkg_id);
        
        Ok(())
    }

    async fn handle_udp_keepalive(
        addr: SocketAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        // Send Code 101 UDP keepalive response
        // Based on working pcap, the camera expects a simple response
        let response = serde_json::json!({
            "code": 101
        });
        let response_json = serde_json::to_string(&response)?;
        
        let header = ProtocolHeader::json(0, response_json.len());
        let mut message = Vec::new();
        message.extend_from_slice(&header.to_bytes());
        message.extend_from_slice(response_json.as_bytes());
        
        // Send response using the original socket that received the message
        socket.send_to(&message, addr).await?;
        tracing::debug!("Code 101 UDP keepalive response sent to {}:{}", addr.ip(), addr.port());
        
        Ok(())
    }

    async fn handle_udp_probe(
        addr: SocketAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
        config: &AppConfig,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        tracing::info!("Handling UDP probe from {}:{}", addr.ip(), addr.port());
        
        // Generate a random port between 32000-65000 (like the working pcap)
        let random_port = rand::thread_rng().gen_range(32000..65000);
        
        // Try to bind to the random port for video streaming
        let random_socket_result = UdpSocket::bind(format!("0.0.0.0:{}", random_port)).await;
        
        match random_socket_result {
            Ok(random_socket) => {
                tracing::info!("Successfully bound to random port {} for video streaming", random_port);
                
                // Store the random socket in the camera manager for this camera
                let source_ip = addr.ip();
                let random_socket_arc = Arc::new(random_socket);
                {
                    let mut manager = camera_manager.write().await;
                    let camera = manager.get_or_create_camera(source_ip).await;
                    let mut camera_guard = camera.write().await;
                    camera_guard.random_video_socket = Some(random_socket_arc.clone());
                    camera_guard.random_video_port = Some(random_port);
                }
                
                // Send Code 21 UDP probe response with random port
                let response = serde_json::json!({
                    "code": 21,
                    "ip": config.server_ip,
                    "port": random_port
                });
                let response_json = serde_json::to_string(&response)?;
                tracing::info!("Code 21 response JSON: {}", response_json);
                
                let header = ProtocolHeader::json(0, response_json.len());
                let mut message = Vec::new();
                message.extend_from_slice(&header.to_bytes());
                message.extend_from_slice(response_json.as_bytes());
                
                // Send response using the original socket that received the message
                socket.send_to(&message, addr).await?;
                
                tracing::info!("Code 21 UDP probe response sent to {}:{} with random port {}", addr.ip(), addr.port(), random_port);
                
                // Note: The random socket is stored in the camera manager
                // Video frames will be received on the main UDP routers
                tracing::info!("Random port {} bound and stored for camera {}", random_port, source_ip);
            }
            Err(e) => {
                tracing::error!("Failed to bind to random port {}: {}", random_port, e);
                
                // Fallback: use a fixed port from config
                let fallback_port = config.udp_stream_port_2;
                let response = serde_json::json!({
                    "code": 21,
                    "ip": config.server_ip,
                    "port": fallback_port
                });
                let response_json = serde_json::to_string(&response)?;
                
                let header = ProtocolHeader::json(0, response_json.len());
                let mut message = Vec::new();
                message.extend_from_slice(&header.to_bytes());
                message.extend_from_slice(response_json.as_bytes());
                
                socket.send_to(&message, addr).await?;
                tracing::info!("Code 21 UDP probe response sent to {}:{} with fallback port {}", addr.ip(), addr.port(), fallback_port);
            }
        }
        
        Ok(())
    }

    async fn handle_raw_udp_keepalive(
        addr: SocketAddr,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        // Send 20-byte UDP keepalive response (raw data, no JSON)
        let response = vec![0u8; 20]; // 20 bytes of zeros as keepalive response
        
        // Send response using the original socket that received the message
        socket.send_to(&response, addr).await?;
        tracing::debug!("20-byte UDP keepalive response sent to {}:{}", addr.ip(), addr.port());
        
        Ok(())
    }


    async fn handle_code51_response(
        addr: SocketAddr,
        payload: &[u8],
        camera_manager: &Arc<RwLock<CameraManager>>,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        let source_ip = addr.ip();
        
        // Parse Code 51 response
        if let Ok(json_str) = String::from_utf8(payload.to_vec()) {
            let clean_json_str = json_str.trim_start_matches('\0');
            tracing::info!("Code 51 response from {}: {}", source_ip, clean_json_str);
            
            // Get camera and update probe state
            let camera = {
                let mut manager = camera_manager.write().await;
                manager.get_or_create_camera(source_ip).await
            };
            let mut camera_guard = camera.write().await;
            camera_guard.update_heartbeat();
            
            // Update probe state
            match &mut camera_guard.probe_state {
                ProbeState::NotStarted => {
                    camera_guard.probe_state = ProbeState::InProgress { count: 1 };
                    tracing::info!("Code 50/51 probe exchange started for {}", source_ip);
                }
                ProbeState::InProgress { count } => {
                    *count += 1;
                    tracing::info!("Code 50/51 probe exchange count: {} for {}", count, source_ip);
                    
                    // After 3 exchanges, mark as completed
                    if *count >= 3 {
                        camera_guard.probe_state = ProbeState::Completed;
                        tracing::info!("Code 50/51 probe exchange completed for {}", source_ip);
                    }
                }
                ProbeState::Completed => {
                    tracing::debug!("Code 50/51 probe exchange already completed for {}", source_ip);
                }
            }
            
            // Send Code 50 probe request in response
            let request = serde_json::json!({
                "code": 50
            });
            let request_json = serde_json::to_string(&request)?;
            
            let header = ProtocolHeader::json(0, request_json.len());
            let mut message = Vec::new();
            message.extend_from_slice(&header.to_bytes());
            message.extend_from_slice(request_json.as_bytes());
            
            // Send response using the original socket that received the message
            socket.send_to(&message, addr).await?;
            tracing::info!("Code 50 probe request sent to {}:{}", addr.ip(), addr.port());
        }
        
        Ok(())
    }

    async fn send_batch_retransmission_confirmation(
        camera_ip: IpAddr,
        package_ids: &[u32],
        camera_manager: &Arc<RwLock<CameraManager>>,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        if package_ids.is_empty() {
            return Ok(());
        }

        // Get the camera's UDP port (the port the camera is using to send video)
        let camera_udp_port = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.cameras.get(&camera_ip) {
                if let Ok(camera_guard) = camera.try_read() {
                    // Use the first UDP port the camera is using (usually the main video port)
                    camera_guard.udp_ports.keys().next().copied()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(port) = camera_udp_port {
            // Create retransmission confirmation payload following the working pcap format exactly
            let mut payload = Vec::new();
            
            // Target device ID: "00000000" (8 bytes)
            payload.extend_from_slice(b"00000000");
            
            // Add all received package IDs (4 bytes each in little endian)
            for &pkg_id in package_ids {
                payload.extend_from_slice(&pkg_id.to_le_bytes());
            }
            
            // Create the complete message following the pcap format:
            // - Total length (4 bytes, little endian)
            // - CMD 605 (4 bytes, little endian) 
            // - Target device ID (8 bytes)
            // - Package IDs (4 bytes each, little endian)
            let total_length = 4 + 8 + (package_ids.len() * 4); // CMD + device_id + package_ids
            let mut message = Vec::new();
            message.extend_from_slice(&total_length.to_le_bytes()); // Total length
            message.extend_from_slice(&605u32.to_le_bytes());       // CMD 605
            message.extend_from_slice(&payload);                    // Device ID + package IDs
            
            // Send to camera's UDP port (the port the camera is using to send video)
            let addr = SocketAddr::new(camera_ip, port);
            socket.send_to(&message, addr).await?;
            
            tracing::info!("Sent batch retransmission confirmation to {}:{} for {} packages (CMD=605, total_len={})", 
                camera_ip, port, package_ids.len(), total_length);
        } else {
            tracing::warn!("No UDP port found for camera {}, cannot send retransmission confirmation", camera_ip);
        }
        
        Ok(())
    }

    async fn send_empty_retransmission_confirmation(
        camera_ip: IpAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        // Create retransmission confirmation payload following the working pcap format exactly
        let mut payload = Vec::new();
        
        // Target device ID: "00000000" (8 bytes)
        payload.extend_from_slice(b"00000000");
        
        // Create the complete message following the pcap format:
        // - Total length (4 bytes, little endian)
        // - CMD 605 (4 bytes, little endian) 
        // - Target device ID (8 bytes)
        // - Package IDs (empty list for heartbeat response)
        let total_length: u32 = 4 + 8 + 0; // CMD + device_id + no package IDs
        let mut message = Vec::new();
        message.extend_from_slice(&total_length.to_le_bytes()); // Total length
        message.extend_from_slice(&605u32.to_le_bytes());       // CMD 605
        message.extend_from_slice(&payload);                    // Device ID + empty package list
        
        // Send to camera's UDP port (the port the camera is using to send video)
        let camera_udp_port = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.cameras.get(&camera_ip) {
                if let Ok(camera_guard) = camera.try_read() {
                    // Use the first UDP port the camera is using (usually the main video port)
                    camera_guard.udp_ports.keys().next().copied()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(port) = camera_udp_port {
            let addr = SocketAddr::new(camera_ip, port);
            socket.send_to(&message, addr).await?;
            tracing::info!("Sent empty retransmission confirmation to {}:{} (CMD=605, empty list)", camera_ip, port);
        } else {
            tracing::warn!("No UDP port found for camera {}, cannot send empty retransmission confirmation", camera_ip);
        }
        
        Ok(())
    }

    async fn periodic_retransmission_task(
        socket: Arc<UdpSocket>,
        camera_manager: Arc<RwLock<CameraManager>>,
    ) {
        // This task is no longer needed since we handle retransmissions per frame
        // in the handle_video_frame function
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await; // Sleep for 1 hour
        }
    }

    async fn handle_udp_heartbeat(
        addr: SocketAddr,
        camera_manager: &Arc<RwLock<CameraManager>>,
        socket: &Arc<UdpSocket>,
    ) -> Result<()> {
        tracing::debug!("Received UDP heartbeat from {}:{}", addr.ip(), addr.port());
        
        // Update camera heartbeat
        {
            let mut manager = camera_manager.write().await;
            let camera = manager.get_or_create_camera(addr.ip()).await;
            let mut camera_guard = camera.write().await;
            camera_guard.update_heartbeat();
            
            // Store the UDP port for this camera
            camera_guard.udp_ports.insert(addr.port(), 1);
        }
        
        // Respond with retransmission confirmation (empty list since no packages received yet)
        // This follows the pattern from the working pcap: respond to cmd=100 with CMD 605
        let mut payload = Vec::new();
        
        // Target device ID: "00000000" (8 bytes)
        payload.extend_from_slice(b"00000000");
        
        // Create the complete message following the pcap format:
        // - Total length (4 bytes, little endian)
        // - CMD 605 (4 bytes, little endian) 
        // - Target device ID (8 bytes)
        // - Package IDs (empty list for heartbeat response)
        let total_length: u32 = 4 + 8 + 0; // CMD + device_id + no package IDs
        let mut message = Vec::new();
        message.extend_from_slice(&total_length.to_le_bytes()); // Total length
        message.extend_from_slice(&605u32.to_le_bytes());       // CMD 605
        message.extend_from_slice(&payload);                    // Device ID + empty package list
        
        // Send response back to the camera
        socket.send_to(&message, addr).await?;
        
        tracing::debug!("Sent UDP heartbeat response to {}:{} (CMD=605, empty list)", addr.ip(), addr.port());
        
        Ok(())
    }

    async fn periodic_incomplete_frame_task(
        camera_manager: Arc<RwLock<CameraManager>>,
    ) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500)); // Check every 500ms
        
        loop {
            interval.tick().await;
            
            // Check all cameras for incomplete frames
            let mut manager = camera_manager.write().await;
            for (ip, camera) in &mut manager.cameras {
                if let Ok(mut camera_guard) = camera.try_write() {
                    if camera_guard.stream_buffer.complete_incomplete_frame() {
                        tracing::info!("Completed incomplete frame for camera {}", ip);
                    }
                }
            }
        }
    }
    

}
