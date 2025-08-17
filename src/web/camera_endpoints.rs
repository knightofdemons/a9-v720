use crate::types::CameraManager;
use crate::protocol::{ProtocolHeader, ForwardCommand};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::net::IpAddr;
use tokio::io::AsyncWriteExt;

pub async fn list_cameras(
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> impl IntoResponse {
    let manager = camera_manager.read().await;
    let mut cameras = Vec::new();
    
    for camera in manager.cameras.values() {
        let camera_guard = camera.read().await;
        if let Some(device_id) = &camera_guard.device_id {
            cameras.push(device_id.clone());
        }
    }
    
    Json(json!({
        "code": 200,
        "message": "OK",
        "data": {
            "cameras": cameras,
            "count": cameras.len()
        }
    }))
}

pub async fn get_camera_info(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> Response {
    let manager = camera_manager.read().await;
    
    // Find camera by device ID
    let mut target_camera = None;
    for camera in manager.cameras.values() {
        let camera_guard = camera.read().await;
        if let Some(id) = &camera_guard.device_id {
            if id == &device_id {
                target_camera = Some((
                    camera_guard.ip,
                    camera_guard.is_connected(),
                    camera_guard.last_heartbeat,
                    camera_guard.udp_ports.keys().cloned().collect::<Vec<_>>(),
                    camera_guard.nat_ports.clone(),
                    camera_guard.state.clone()
                ));
                break;
            }
        }
    }
    
    if let Some((ip, connected, last_heartbeat, udp_ports, nat_ports, state)) = target_camera {
        // Get buffer information
        let buffer_info = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.cameras.get(&ip) {
                if let Ok(camera_guard) = camera.try_read() {
                    let stream_buffer = &camera_guard.stream_buffer;
                    json!({
                        "frame_count": stream_buffer.frame_count(),
                        "max_frames": stream_buffer.max_frames,
                        "total_bytes": stream_buffer.get_all_frames().iter().map(|frame| frame.len()).sum::<usize>()
                    })
                } else {
                    json!({
                        "frame_count": 0,
                        "max_frames": 100,
                        "total_bytes": 0
                    })
                }
            } else {
                json!({
                    "frame_count": 0,
                    "max_frames": 100,
                    "total_bytes": 0
                })
            }
        };

        Json(json!({
            "code": 200,
            "message": "OK",
            "data": {
                "device_id": device_id,
                "ip": ip.to_string(),
                "connected": connected,
                "last_heartbeat": last_heartbeat.to_rfc3339(),
                "udp_ports": udp_ports,
                "nat_ports": nat_ports,
                "streaming": state == crate::types::ProtocolState::Streaming,
                "stream_buffer": buffer_info
            }
        })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({
            "code": 404,
            "message": "Camera not found",
            "data": null
        }))).into_response()
    }
}

pub async fn start_streaming(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> Response {
    // First, quickly check if camera exists and get its IP without holding locks for long
    let target_ip = {
        let manager = camera_manager.read().await;
        let mut found_ip = None;
        for camera in manager.cameras.values() {
            if let Ok(camera_guard) = camera.try_read() {
                if let Some(id) = &camera_guard.device_id {
                    if id == &device_id {
                        found_ip = Some(camera_guard.ip);
                        break;
                    }
                }
            }
        }
        found_ip
    };
    
    if let Some(ip_addr) = target_ip {
        // Get the camera's TCP connection without blocking
        let tcp_conn = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.cameras.get(&ip_addr) {
                if let Ok(camera_guard) = camera.try_read() {
                    camera_guard.tcp_conn.clone()
                } else {
                    None
                }
            } else {
                None
            }
        };
        
        if let Some(tcp_conn) = tcp_conn {
            // Spawn the streaming control in a separate task to avoid blocking the response
            let tcp_conn_clone = tcp_conn.clone();
            let camera_manager_clone = camera_manager.clone();
            let device_id_clone = device_id.clone();
            
            tokio::spawn(async move {
                match crate::router::tcp::TcpRouter::start_streaming_for_camera(ip_addr, &camera_manager_clone).await {
                    Ok(_) => {
                        tracing::info!("Successfully started streaming for camera {}", ip_addr);
                    }
                    Err(e) => {
                        tracing::error!("Failed to start streaming for camera {}: {}", device_id_clone, e);
                    }
                }
            });
            
            // Return immediately with success response
            Json(json!({
                "code": 200,
                "message": "Streaming start command sent successfully",
                "data": {
                    "device_id": device_id,
                    "ip": ip_addr.to_string(),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "note": "Streaming control commands are being sent in background"
                }
            })).into_response()
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "code": 500,
                "message": "No TCP connection found for camera",
                "data": null
            }))).into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, Json(json!({
            "code": 404,
            "message": "Camera not found",
            "data": null
        }))).into_response()
    }
}

pub async fn stop_streaming(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> Response {
    // First, quickly check if camera exists and get its IP without holding locks for long
    let target_ip = {
        let manager = camera_manager.read().await;
        let mut found_ip = None;
        for camera in manager.cameras.values() {
            if let Ok(camera_guard) = camera.try_read() {
                if let Some(id) = &camera_guard.device_id {
                    if id == &device_id {
                        found_ip = Some(camera_guard.ip);
                        break;
                    }
                }
            }
        }
        found_ip
    };
    
    if let Some(ip_addr) = target_ip {
        // Spawn the stop streaming logic in a separate task to avoid blocking the response
        let camera_manager_clone = camera_manager.clone();
        let device_id_clone = device_id.clone();
        
        tokio::spawn(async move {
            // Close all UDP connections for this camera
            if let Ok(mut manager) = camera_manager_clone.try_write() {
                if let Some(camera) = manager.cameras.get(&ip_addr) {
                    if let Ok(mut camera_guard) = camera.try_write() {
                        // Close all UDP sockets
                        camera_guard.udp_ports.clear();
                        tracing::info!("Closed {} UDP connections for camera {}", 
                            camera_guard.udp_ports.len(), ip_addr);
                        
                        // Clear video buffer
                        camera_guard.stream_buffer.clear();
                        tracing::info!("Cleared video buffer for camera {}", ip_addr);
                        
                        // Set state back to Idle
                        camera_guard.state = crate::types::ProtocolState::Idle;
                        tracing::info!("Camera {} state set to Idle", ip_addr);
                    }
                }
            }
            
            // Optionally send a stop streaming command to the camera
            let tcp_conn = {
                if let Ok(manager) = camera_manager_clone.try_read() {
                    if let Some(camera) = manager.cameras.get(&ip_addr) {
                        if let Ok(camera_guard) = camera.try_read() {
                            camera_guard.tcp_conn.clone()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            
            if let Some(tcp_conn) = tcp_conn {
                // Send stop streaming command (Code 301/0)
                let stop_streaming = serde_json::json!({
                    "code": 301,
                    "target": "00112233445566778899aabbccddeeff",
                    "content": {
                        "code": 0  // Stop streaming
                    }
                });
                
                match send_tcp_message(&tcp_conn, &stop_streaming).await {
                    Ok(_) => {
                        tracing::info!("Stop streaming command sent to {}", ip_addr);
                    }
                    Err(e) => {
                        tracing::error!("Failed to send stop streaming command to {}: {}", ip_addr, e);
                    }
                }
            }
        });
        
        // Return immediately with success response
        Json(json!({
            "code": 200,
            "message": "Streaming stop command sent successfully",
            "data": {
                "device_id": device_id,
                "ip": ip_addr.to_string(),
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "note": "Streaming stop commands are being sent in background"
            }
        })).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({
            "code": 404,
            "message": "Camera not found",
            "data": null
        }))).into_response()
    }
}

pub async fn trigger_snapshot(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> Response {
    // First, quickly check if camera exists and get its IP without holding locks for long
    let target_ip = {
        let manager = camera_manager.read().await;
        let mut found_ip = None;
        for camera in manager.cameras.values() {
            if let Ok(camera_guard) = camera.try_read() {
                if let Some(id) = &camera_guard.device_id {
                    if id == &device_id {
                        found_ip = Some(camera_guard.ip);
                        break;
                    }
                }
            }
        }
        found_ip
    };
    
    if let Some(ip_addr) = target_ip {
        // Get the camera's TCP connection without blocking
        let tcp_conn = {
            let manager = camera_manager.read().await;
            if let Some(camera) = manager.cameras.get(&ip_addr) {
                if let Ok(camera_guard) = camera.try_read() {
                    camera_guard.tcp_conn.clone()
                } else {
                    None
                }
            } else {
                None
            }
        };
        
        if let Some(tcp_conn) = tcp_conn {
            // Spawn the snapshot command in a separate task to avoid blocking the response
            let tcp_conn_clone = tcp_conn.clone();
            let device_id_clone = device_id.clone();
            
            tokio::spawn(async move {
                // Send snapshot command (Code 301/5)
                let snapshot_command = serde_json::json!({
                    "code": 301,
                    "target": "00112233445566778899aabbccddeeff",
                    "content": {
                        "code": 5  // Snapshot
                    }
                });
                
                match send_tcp_message(&tcp_conn_clone, &snapshot_command).await {
                    Ok(_) => {
                        tracing::info!("Snapshot command sent to {}", ip_addr);
                    }
                    Err(e) => {
                        tracing::error!("Failed to trigger snapshot for camera {}: {}", device_id_clone, e);
                    }
                }
            });
            
            // Return immediately with success response
            Json(json!({
                "code": 200,
                "message": "Snapshot command sent successfully",
                "data": {
                    "device_id": device_id,
                    "ip": ip_addr.to_string(),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "note": "Snapshot command is being sent in background"
                }
            })).into_response()
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "code": 500,
                "message": "No TCP connection found for camera",
                "data": null
            }))).into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, Json(json!({
            "code": 404,
            "message": "Camera not found",
            "data": null
        }))).into_response()
    }
}

pub async fn get_video_stream(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> impl IntoResponse {
    let manager = camera_manager.read().await;
    
    // Find camera by device ID
    let mut target_camera = None;
    for camera in manager.cameras.values() {
        let camera_guard = camera.read().await;
        if let Some(id) = &camera_guard.device_id {
            if id == &device_id {
                target_camera = Some(camera_guard.stream_buffer.get_latest_frame().map(|f| f.to_vec()));
                break;
            }
        }
    }
    
    if let Some(latest_frame) = target_camera {
        if let Some(frame) = latest_frame {
            Response::builder()
                .status(200)
                .header("Content-Type", "image/jpeg")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .body(axum::body::Body::from(frame))
                .unwrap()
        } else {
            (StatusCode::NO_CONTENT, "No video frames available").into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "Camera not found").into_response()
    }
}

pub async fn get_mjpeg_stream(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> impl IntoResponse {
    let manager = camera_manager.read().await;
    
    // Find camera by device ID
    let mut target_camera = None;
    for camera in manager.cameras.values() {
        let camera_guard = camera.read().await;
        if let Some(id) = &camera_guard.device_id {
            if id == &device_id {
                target_camera = Some(camera_guard.stream_buffer.get_latest_frame().map(|f| f.to_vec()));
                break;
            }
        }
    }
    
    if let Some(latest_frame) = target_camera {
        if let Some(frame) = latest_frame {
            // Create a simple MJPEG stream with just the latest frame
            let mut mjpeg_data = Vec::new();
            mjpeg_data.extend_from_slice(b"--frame\r\n");
            mjpeg_data.extend_from_slice(b"Content-Type: image/jpeg\r\n");
            mjpeg_data.extend_from_slice(format!("Content-Length: {}\r\n", frame.len()).as_bytes());
            mjpeg_data.extend_from_slice(b"\r\n");
            mjpeg_data.extend_from_slice(&frame);
            mjpeg_data.extend_from_slice(b"\r\n");
            
            Response::builder()
                .status(200)
                .header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .body(axum::body::Body::from(mjpeg_data))
                .unwrap()
        } else {
            (StatusCode::NO_CONTENT, "No video frames available").into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "Camera not found").into_response()
    }
}

// Debug endpoint to examine buffer contents
pub async fn debug_buffer(
    Path(device_id): Path<String>,
    State(camera_manager): State<Arc<RwLock<CameraManager>>>,
) -> Response {
    let manager = camera_manager.read().await;
    
    // Find camera by device ID
    let mut target_camera = None;
    for camera in manager.cameras.values() {
        let camera_guard = camera.read().await;
        if let Some(id) = &camera_guard.device_id {
            if id == &device_id {
                let buffer = &camera_guard.stream_buffer;
                let frames = buffer.get_all_frames();
                let latest_frame = buffer.get_latest_frame();
                
                target_camera = Some(json!({
                    "frame_count": frames.len(),
                    "max_frames": buffer.max_frames,
                    "latest_frame_size": latest_frame.map(|f| f.len()),
                    "latest_frame_hex": latest_frame.map(|f| {
                        f.iter().take(16).map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
                    }),
                    "latest_frame_ascii": latest_frame.map(|f| {
                        String::from_utf8_lossy(&f.iter().take(32).map(|&b| if b >= 32 && b <= 126 { b } else { b'.' }).collect::<Vec<_>>()).to_string()
                    }),
                    "all_frame_sizes": frames.iter().map(|f| f.len()).collect::<Vec<_>>()
                }));
                break;
            }
        }
    }
    
    if let Some(buffer_info) = target_camera {
        Json(json!({
            "code": 200,
            "message": "Buffer debug info",
            "data": buffer_info
        })).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Camera not found").into_response()
    }
}

// Helper function for TCP communication (used by snapshot and stop streaming)
async fn send_tcp_message(
    tcp_conn: &Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    json_data: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json_str = serde_json::to_string(json_data)?;
    let header = ProtocolHeader::json(0, json_str.len());
    let mut message = Vec::new();
    message.extend_from_slice(&header.to_bytes());
    message.extend_from_slice(json_str.as_bytes());
    
    let mut socket_guard = tcp_conn.lock().await;
    socket_guard.write_all(&message).await?;
    
    Ok(())
}


