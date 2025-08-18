use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::net::SocketAddr;

/// Camera protocol states
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProtocolState {
    Disconnected,    // No connection
    Configuring,     // HTTP config check
    Registering,     // TCP registration
    Idle,           // Ready for commands
    NatTraversal,   // NAT traversal in progress
    Streaming,      // Active video stream
    Snapshot,       // Taking snapshot
    Error,          // Error state, needs reconnection
}

/// Device information from camera
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub code: u32,
    pub udp_play_back: Option<u32>,
    pub dev_power: u32,
    pub sd_move_mode: u32,
    pub sd_dev_status: u32,
    pub ir_led: u32,
    pub inst_led: u32,
    pub speed_grade: u32,
    pub mirror_flip: u32,
    pub wifi_name: String,
    pub version: String,
}

/// Video frame data
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub frame_id: u32,
    pub data: Vec<u8>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub is_keyframe: bool,
}

/// Stream buffer for video data
#[derive(Debug, Clone)]
pub struct StreamBuffer {
    frames: VecDeque<Vec<u8>>,  // Store complete video frames
    pub max_frames: usize,          // Maximum number of frames to keep
    current_frame: Option<FrameFragment>, // Current frame being assembled
}

#[derive(Debug, Clone)]
struct FrameFragment {
    frame_type: u16,           // CMD field (1=JPEG, 4=G711, 7=AVI)
    pkg_id: u32,               // Packet ID for this frame
    fragments: Vec<Vec<u8>>,   // Accumulated fragments
    expected_size: Option<usize>, // Expected total size (from last fragment)
    is_complete: bool,         // Whether frame is complete
}

impl StreamBuffer {
    pub fn new(max_frames: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(max_frames),
            max_frames,
            current_frame: None,
        }
    }

    /// Add a UDP packet fragment to the buffer
    /// Returns true if a complete frame was assembled
        pub fn add_fragment(&mut self, cmd: u16, msg_flag: u8, pkg_id: u32, payload: &[u8]) -> bool {
        tracing::debug!("add_fragment: cmd={}, msg_flag={}, pkg_id={}, payload_len={}",
            cmd, msg_flag, pkg_id, payload.len());

        match (cmd, msg_flag) {
            (6, 255) => {
                // cmd=6 frames are PCM audio frames (not video)
                // These should be processed as audio data, not added to video buffer
                tracing::debug!("Received PCM audio frame (cmd=6): {} bytes", payload.len());
                // For now, just acknowledge receipt but don't add to video buffer
                true
            }
            (6, _) => {
                // Other cmd=6 frames (fallback)
                tracing::debug!("Received PCM audio frame (cmd=6, msg_flag={}): {} bytes", msg_flag, payload.len());
                true
            }
            (1, 250) => {
                // Start of JPEG frame - begin new frame assembly
                tracing::info!("Starting JPEG frame assembly (msg_flag=250): {} bytes", payload.len());
                self.start_new_frame(pkg_id, payload.to_vec());
                true
            }
            (1, 251) => {
                // Continuation of JPEG frame - add to current frame
                tracing::info!("Continuing JPEG frame assembly (msg_flag=251): {} bytes", payload.len());
                self.add_frame_fragment(pkg_id, payload.to_vec());
                true
            }
            (1, 252) => {
                // End of JPEG frame - complete frame assembly
                tracing::info!("Completing JPEG frame assembly (msg_flag=252): {} bytes", payload.len());
                if self.complete_frame(pkg_id, payload.to_vec()) {
                    tracing::info!("JPEG frame assembly completed successfully");
                } else {
                    tracing::warn!("Failed to complete JPEG frame assembly");
                }
                true
            }
            _ => {
                // Unknown command/flag combination
                tracing::debug!("Unknown frame type: cmd={}, msg_flag={}, pkg_id={}", cmd, msg_flag, pkg_id);
                false
            }
        }
    }

    /// Assemble fragments into a complete frame
    fn assemble_frame(&self, frame: FrameFragment) -> Option<Vec<u8>> {
        if frame.fragments.is_empty() {
            return None;
        }

        // Calculate total size (excluding size bytes from last fragment if we have them)
        let mut total_size = 0;
        let mut has_size_info = false;
        
        for (i, fragment) in frame.fragments.iter().enumerate() {
            if i == frame.fragments.len() - 1 {
                // Last fragment: check if it has size information
                if fragment.len() >= 4 {
                    // Try to extract size from last 4 bytes
                    let size_bytes = &fragment[fragment.len() - 4..];
                    let potential_size = u32::from_le_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]) as usize;
                    
                    // Validate the size (should be reasonable for a JPEG frame)
                    if potential_size > 0 && potential_size < 1000000 { // Max 1MB
                        total_size += fragment.len().saturating_sub(4);
                        has_size_info = true;
                        tracing::debug!("Using size info from last fragment: {} bytes", potential_size);
                    } else {
                        // Size info seems invalid, include the whole fragment
                        total_size += fragment.len();
                        tracing::debug!("Invalid size info in last fragment, including whole fragment");
                    }
                } else {
                    // Fragment too small to have size info, include the whole fragment
                    total_size += fragment.len();
                }
            } else {
                total_size += fragment.len();
            }
        }

        // Verify size if we have expected size
        if let Some(expected_size) = frame.expected_size {
            if has_size_info && total_size != expected_size {
                tracing::warn!("Frame size mismatch: expected {}, got {}", expected_size, total_size);
            }
        }

        // Assemble frame
        let mut complete_frame = Vec::with_capacity(total_size);
        for (i, fragment) in frame.fragments.iter().enumerate() {
            if i == frame.fragments.len() - 1 && has_size_info {
                // Last fragment: exclude the 4 size bytes
                complete_frame.extend_from_slice(&fragment[..fragment.len().saturating_sub(4)]);
            } else {
                complete_frame.extend_from_slice(fragment);
            }
        }

        Some(complete_frame)
    }

    /// Add a complete frame to the buffer
    pub fn add_complete_frame(&mut self, frame: Vec<u8>) {
        // Remove oldest frame if buffer is full
        if self.frames.len() >= self.max_frames {
            self.frames.pop_front();
        }
        
        let frame_len = frame.len();
        self.frames.push_back(frame);
        tracing::debug!("Added complete frame: {} bytes (buffer: {}/{})", 
            frame_len, self.frames.len(), self.max_frames);
    }

    // Frame assembly methods for fragmented JPEG frames
    fn start_new_frame(&mut self, _pkg_id: u32, payload: Vec<u8>) {
        let payload_len = payload.len();
        // Start assembling a new frame
        self.current_frame = Some(FrameFragment {
            frame_type: 1, // JPEG
            pkg_id: 0, // Don't use package ID for assembly
            fragments: vec![payload],
            expected_size: None,
            is_complete: false,
        });
        tracing::debug!("Started new frame assembly: {} bytes", payload_len);
    }

    fn add_frame_fragment(&mut self, _pkg_id: u32, payload: Vec<u8>) {
        if let Some(ref mut frame) = self.current_frame {
            // Don't check package ID - just add the fragment
            frame.fragments.push(payload);
            tracing::debug!("Added fragment to frame, total fragments: {}", frame.fragments.len());
        } else {
            tracing::warn!("No current frame to add fragment to");
        }
    }

    fn complete_frame(&mut self, _pkg_id: u32, payload: Vec<u8>) -> bool {
        if let Some(mut frame) = self.current_frame.take() {
            // Don't check package ID - just add the final fragment
            frame.fragments.push(payload);
            
            // The last 4 bytes of the last fragment contain the total frame size
            if let Some(last_fragment) = frame.fragments.last() {
                if last_fragment.len() >= 4 {
                    let size_bytes = &last_fragment[last_fragment.len() - 4..];
                    let expected_size = u32::from_le_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]) as usize;
                    frame.expected_size = Some(expected_size);
                    tracing::debug!("Expected frame size: {} bytes", expected_size);
                }
            }

            // Assemble the complete frame
            if let Some(complete_frame) = self.assemble_frame(frame) {
                tracing::info!("Successfully assembled JPEG frame: {} bytes", complete_frame.len());
                self.add_complete_frame(complete_frame);
                return true;
            } else {
                tracing::error!("Failed to assemble frame");
                return false;
            }
        } else {
            tracing::warn!("No current frame to complete");
            return false;
        }
    }

    // Add a method to complete frames without end fragment (for missing end frames)
    pub fn complete_incomplete_frame(&mut self) -> bool {
        if let Some(frame) = self.current_frame.take() {
            if frame.fragments.len() > 1 {
                // We have multiple fragments, try to assemble what we have
                tracing::info!("Completing incomplete frame with {} fragments", frame.fragments.len());
                if let Some(complete_frame) = self.assemble_frame(frame) {
                    tracing::info!("Successfully assembled incomplete JPEG frame: {} bytes", complete_frame.len());
                    self.add_complete_frame(complete_frame);
                    return true;
                }
            } else {
                tracing::warn!("Incomplete frame has only {} fragment(s), discarding", frame.fragments.len());
            }
        }
        false
    }

    /// Get the latest complete frame
    pub fn get_latest_frame(&self) -> Option<&[u8]> {
        self.frames.back().map(|frame| frame.as_slice())
    }

    /// Get all frames (for debugging)
    pub fn get_all_frames(&self) -> &VecDeque<Vec<u8>> {
        &self.frames
    }

    /// Get frame count
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Clear all frames
    pub fn clear(&mut self) {
        self.frames.clear();
        self.current_frame = None;
    }

    /// Get list of received package IDs for retransmission confirmation
    pub fn get_received_packages(&self) -> Vec<u32> {
        let mut packages = Vec::new();
        
        // Add package ID from current frame being assembled
        if let Some(ref frame) = self.current_frame {
            packages.push(frame.pkg_id);
        }
        
        // Add package IDs from recently received frames (last 10 frames)
        // This is a simplified approach - in a real implementation you'd want to track
        // all received package IDs and manage the list size
        packages
    }

    // Legacy method for backward compatibility
    pub fn add_frame(&mut self, _frame: &[u8]) {
        // This method is deprecated - use add_fragment instead
        tracing::warn!("add_frame is deprecated, use add_fragment instead");
    }

    // Legacy methods for backward compatibility
    pub fn get_latest_data(&self) -> &[u8] {
        self.get_latest_frame().unwrap_or(&[])
    }

    pub fn current_size(&self) -> usize {
        self.frames.iter().map(|f| f.len()).sum()
    }

    pub fn max_size(&self) -> usize {
        self.max_frames * 1024 * 1024 // Rough estimate
    }
}

/// Viewer information
#[derive(Debug, Clone)]
pub struct ViewerInfo {
    pub id: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Camera connection information
#[derive(Debug)]
pub struct CameraConnection {
    pub device_id: String,
    pub ip: IpAddr,
    pub addr: SocketAddr,
    pub state: ProtocolState,
    pub protocol_state: ProtocolState,
    pub tcp_conn: Option<tokio::net::TcpStream>,
    pub stream_buffer: StreamBuffer,
    pub received_packages: Vec<u32>,
    pub last_retransmission_time: chrono::DateTime<chrono::Utc>,
    pub udp_ports: HashMap<u16, u16>, // port -> dummy value (just for tracking)
    pub viewers: HashMap<String, ViewerInfo>,
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
    pub last_keepalive: chrono::DateTime<chrono::Utc>,
    pub retry_count: u32,
    pub device_info: Option<DeviceInfo>,
    pub nat_ports: Vec<u16>,
    pub device_info_ack_sent: bool,
    pub random_video_socket: Option<Arc<tokio::net::UdpSocket>>, // Random port socket for video streaming
    pub random_video_port: Option<u16>, // Random port number for video streaming
    pub probe_state: ProbeState, // Track Code 50/51 probe exchange
    pub first_retransmission_sent: bool, // Track if first empty retransmission was sent
    pub retransmission_bucket: Vec<u32>, // Bucket to collect package IDs between end frames
    pub token: Option<String>,
    pub camera_nat_port: Option<u16>,
    pub code51_count: u32,
    pub pending_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeState {
    NotStarted,
    InProgress { count: u32 },
    Completed,
}

impl CameraConnection {
    pub fn new(device_id: String, ip: IpAddr, addr: SocketAddr) -> Self {
        Self {
            device_id,
            ip,
            addr,
            state: ProtocolState::Disconnected,
            protocol_state: ProtocolState::Disconnected,
            tcp_conn: None,
            stream_buffer: StreamBuffer::new(100), // Keep 100 frames
            received_packages: Vec::new(),
            last_retransmission_time: chrono::Utc::now(),
            udp_ports: HashMap::new(),
            viewers: HashMap::new(),
            last_heartbeat: chrono::Utc::now(),
            last_keepalive: chrono::Utc::now(),
            retry_count: 0,
            device_info: None,
            nat_ports: Vec::new(),
            device_info_ack_sent: false,
            random_video_socket: None,
            random_video_port: None,
            probe_state: ProbeState::NotStarted,
            first_retransmission_sent: false,
            retransmission_bucket: Vec::new(),
            token: None,
            camera_nat_port: None,
            code51_count: 0,
            pending_command: None,
        }
    }

    pub fn add_viewer(&mut self, viewer_id: String) {
        let now = chrono::Utc::now();
        self.viewers.insert(viewer_id.clone(), ViewerInfo {
            id: viewer_id,
            connected_at: now,
            last_activity: now,
        });
    }

    pub fn remove_viewer(&mut self, viewer_id: &str) {
        self.viewers.remove(viewer_id);
    }

    pub fn viewer_count(&self) -> usize {
        self.viewers.len()
    }

    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = chrono::Utc::now();
    }

    pub fn is_healthy(&self, timeout_seconds: u64) -> bool {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(self.last_heartbeat);
        duration.num_seconds() < timeout_seconds as i64
    }

    pub fn is_connected(&self) -> bool {
        self.state == ProtocolState::Idle || self.state == ProtocolState::Streaming
    }

    pub fn is_connection_healthy(&self) -> bool {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(self.last_keepalive);
        duration.num_seconds() < 30 // Consider unhealthy after 30 seconds without keepalive
    }

    pub fn update_keepalive(&mut self) {
        self.last_keepalive = chrono::Utc::now();
    }

    pub fn add_video_frame(&mut self, data: &[u8]) {
        // Add video frame to stream buffer
        self.stream_buffer.add_complete_frame(data.to_vec());
    }

    pub fn take_pending_command(&mut self) -> Option<String> {
        self.pending_command.take()
    }

    pub fn add_received_package(&mut self, pkg_id: u32) {
        if !self.received_packages.contains(&pkg_id) {
            self.received_packages.push(pkg_id);
        }
    }

    pub fn get_and_clear_received_packages(&mut self) -> Vec<u32> {
        let packages = self.received_packages.clone();
        self.received_packages.clear();
        self.last_retransmission_time = chrono::Utc::now();
        packages
    }

    pub fn should_send_retransmission(&self) -> bool {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(self.last_retransmission_time);
        duration.num_milliseconds() >= 100 // Send every 100ms
    }

    // Retransmission bucket methods
    pub fn add_to_retransmission_bucket(&mut self, pkg_id: u32) {
        if !self.retransmission_bucket.contains(&pkg_id) {
            self.retransmission_bucket.push(pkg_id);
        }
    }

    pub fn get_and_clear_retransmission_bucket(&mut self) -> Vec<u32> {
        let packages = self.retransmission_bucket.clone();
        self.retransmission_bucket.clear();
        packages
    }

    pub fn is_retransmission_bucket_empty(&self) -> bool {
        self.retransmission_bucket.is_empty()
    }
}

/// Camera manager for handling multiple cameras
#[derive(Debug)]
pub struct CameraManager {
    pub cameras: HashMap<IpAddr, Arc<RwLock<CameraConnection>>>,
    pub config: crate::config::AppConfig,
}

impl CameraManager {
    pub fn new(config: crate::config::AppConfig) -> Self {
        Self {
            cameras: HashMap::new(),
            config,
        }
    }

    pub async fn get_or_create_camera(&mut self, ip: IpAddr) -> Arc<RwLock<CameraConnection>> {
        if let Some(camera) = self.cameras.get(&ip) {
            camera.clone()
        } else {
            let device_id = format!("cam{}", ip.to_string().split('.').last().unwrap_or("0"));
            let addr = SocketAddr::new(ip, 6123);
            let camera = Arc::new(RwLock::new(CameraConnection::new(device_id, ip, addr)));
            self.cameras.insert(ip, camera.clone());
            camera
        }
    }

    pub async fn remove_camera(&mut self, ip: IpAddr) {
        self.cameras.remove(&ip);
    }

    pub async fn get_camera(&self, ip: IpAddr) -> Option<Arc<RwLock<CameraConnection>>> {
        self.cameras.get(&ip).cloned()
    }

    pub async fn list_cameras(&self) -> Vec<IpAddr> {
        self.cameras.keys().cloned().collect()
    }
}
