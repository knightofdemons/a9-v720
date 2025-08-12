use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use crate::types::CameraSession;

/// Web server state
#[derive(Clone)]
pub struct WebState {
    pub camera_sessions: Arc<RwLock<HashMap<String, CameraSession>>>,
}

/// Camera information for web display
#[derive(Debug, Clone, Serialize)]
pub struct CameraInfo {
    pub device_id: String,
    pub last_keepalive: String,
    pub status: String,
    pub streaming: bool,
    pub stream_data_size: usize,
}

/// Snapshot request
#[derive(Debug, Deserialize)]
pub struct SnapshotRequest {
    pub device_id: String,
}

/// Livestream request
#[derive(Debug, Deserialize)]
pub struct LivestreamRequest {
    pub device_id: String,
}

/// Stream data request
#[derive(Debug, Deserialize)]
pub struct StreamDataRequest {
    #[allow(dead_code)]
    pub device_id: String,
}

/// Create web routes
pub fn create_web_routes(state: WebState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/api/cameras", get(cameras_handler))
        .route("/api/snapshot", post(snapshot_handler))
        .route("/api/livestream", post(livestream_handler))
        .route("/api/stream-data", get(stream_data_handler))
        .route("/api/health", get(health_handler))
        .route("/api/send-code11", get(send_code11_handler))
        .with_state(state)
}

/// Main index page
async fn index_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>A9 V720 Camera Server</title>
    <style>
        body {
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            margin: 0;
            padding: 20px;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
            background: white;
            border-radius: 15px;
            box-shadow: 0 20px 40px rgba(0,0,0,0.1);
            overflow: hidden;
        }
        .header {
            background: linear-gradient(135deg, #2c3e50 0%, #34495e 100%);
            color: white;
            padding: 30px;
            text-align: center;
        }
        .header h1 {
            margin: 0;
            font-size: 2.5em;
            font-weight: 300;
        }
        .header p {
            margin: 10px 0 0 0;
            opacity: 0.8;
            font-size: 1.1em;
        }
        .content {
            padding: 30px;
        }
        .cameras-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
            gap: 20px;
            margin-top: 20px;
        }
        .camera-card {
            background: #f8f9fa;
            border-radius: 10px;
            padding: 20px;
            border: 1px solid #e9ecef;
            transition: transform 0.2s, box-shadow 0.2s;
        }
        .camera-card:hover {
            transform: translateY(-2px);
            box-shadow: 0 10px 25px rgba(0,0,0,0.1);
        }
        .camera-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 15px;
        }
        .camera-id {
            font-size: 1.2em;
            font-weight: 600;
            color: #2c3e50;
        }
        .status-indicator {
            width: 12px;
            height: 12px;
            border-radius: 50%;
            background: #28a745;
        }
        .status-indicator.offline {
            background: #dc3545;
        }
        .camera-info {
            margin-bottom: 15px;
        }
        .info-row {
            display: flex;
            justify-content: space-between;
            margin-bottom: 5px;
            font-size: 0.9em;
        }
        .info-label {
            color: #6c757d;
            font-weight: 500;
        }
        .info-value {
            color: #495057;
        }
        .camera-actions {
            display: flex;
            gap: 10px;
        }
        .btn {
            padding: 8px 16px;
            border: none;
            border-radius: 5px;
            cursor: pointer;
            font-size: 0.9em;
            font-weight: 500;
            transition: all 0.2s;
            text-decoration: none;
            display: inline-block;
            text-align: center;
        }
        .btn-primary {
            background: #007bff;
            color: white;
        }
        .btn-primary:hover {
            background: #0056b3;
        }
        .btn-success {
            background: #28a745;
            color: white;
        }
        .btn-success:hover {
            background: #1e7e34;
        }
        .btn-warning {
            background: #ffc107;
            color: #212529;
        }
        .btn-warning:hover {
            background: #e0a800;
        }
        .refresh-btn {
            background: #6c757d;
            color: white;
            padding: 10px 20px;
            border: none;
            border-radius: 5px;
            cursor: pointer;
            font-size: 1em;
            margin-bottom: 20px;
        }
        .refresh-btn:hover {
            background: #545b62;
        }
        .no-cameras {
            text-align: center;
            padding: 40px;
            color: #6c757d;
            font-size: 1.1em;
        }
        .loading {
            text-align: center;
            padding: 40px;
            color: #6c757d;
        }
        .spinner {
            border: 3px solid #f3f3f3;
            border-top: 3px solid #007bff;
            border-radius: 50%;
            width: 30px;
            height: 30px;
            animation: spin 1s linear infinite;
            margin: 0 auto 10px;
        }
        @keyframes spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
        }
    </style>
        </head>
        <body>
            <div class="container">
                <div class="header">
            <h1>🎥 A9 V720 Camera Server</h1>
                    <p>Real-time camera monitoring and control</p>
                </div>
        <div class="content">
            <button class="refresh-btn" onclick="loadCameras()">🔄 Refresh Cameras</button>
            <div id="cameras-container">
                <div class="loading">
                    <div class="spinner"></div>
                    Loading cameras...
                </div>
            </div>
                </div>
            </div>
            
            <script>
        // Load cameras on page load
        document.addEventListener('DOMContentLoaded', loadCameras);

                // Auto-refresh every 30 seconds
        setInterval(loadCameras, 30000);

        async function loadCameras() {
            const container = document.getElementById('cameras-container');
            container.innerHTML = '<div class="loading"><div class="spinner"></div>Loading cameras...</div>';

            try {
                const response = await fetch('/api/cameras');
                const cameras = await response.json();
                
                if (cameras.length === 0) {
                    container.innerHTML = '<div class="no-cameras">No cameras connected</div>';
                    return;
                }

                container.innerHTML = '<div class="cameras-grid">' + 
                    cameras.map(camera => createCameraCard(camera)).join('') + 
                    '</div>';
            } catch (error) {
                console.error('Error loading cameras:', error);
                container.innerHTML = '<div class="no-cameras">Error loading cameras</div>';
            }
        }

        function createCameraCard(camera) {
            const isOnline = camera.status === 'online';
            const lastSeen = new Date(camera.last_keepalive).toLocaleString();
            
            return `
                <div class="camera-card">
                    <div class="camera-header">
                        <div class="camera-id">📹 ${camera.device_id}</div>
                        <div class="status-indicator ${isOnline ? '' : 'offline'}"></div>
                    </div>
                    <div class="camera-info">
                        <div class="info-row">
                            <span class="info-label">Status:</span>
                            <span class="info-value">${camera.status}</span>
                        </div>
                        <div class="info-row">
                            <span class="info-label">Last Seen:</span>
                            <span class="info-value">${lastSeen}</span>
                        </div>
                        <div class="info-row">
                            <span class="info-label">Streaming:</span>
                            <span class="info-value">${camera.streaming ? 'Yes' : 'No'}</span>
                        </div>
                        <div class="info-row">
                            <span class="info-label">Stream Data:</span>
                            <span class="info-value">${(camera.stream_data_size / 1024).toFixed(1)} KB</span>
                </div>
                    </div>
                    <div class="camera-actions">
                        <button class="btn btn-primary" onclick="startLivestream('${camera.device_id}')">
                            ${camera.streaming ? '📺 Stop Stream' : '📺 Start Stream'}
                        </button>
                        <button class="btn btn-success" onclick="takeSnapshot('${camera.device_id}')">
                            📸 Snapshot
                        </button>
                        ${camera.streaming ? '<button class="btn btn-warning" onclick="viewStream(\'' + camera.device_id + '\')">👁️ View Stream</button>' : ''}
                    </div>
                </div>
            `;
        }

        async function takeSnapshot(deviceId) {
            try {
                const response = await fetch('/api/snapshot', {
                            method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({ device_id: deviceId })
                });
                
                const result = await response.json();
                if (result.success) {
                    alert(`Snapshot taken for camera ${deviceId}`);
                } else {
                    alert(`Error taking snapshot: ${result.message}`);
                }
            } catch (error) {
                console.error('Error taking snapshot:', error);
                alert('Error taking snapshot');
            }
        }

        async function startLivestream(deviceId) {
            try {
                const response = await fetch('/api/livestream', {
                            method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({ device_id: deviceId })
                });
                
                        const result = await response.json();
                if (result.success) {
                    alert(`Livestream ${result.message.includes('started') ? 'started' : 'stopped'} for camera ${deviceId}`);
                    // Refresh the camera list to update streaming status
                    loadCameras();
                } else {
                    alert(`Error: ${result.message}`);
                }
            } catch (error) {
                console.error('Error with livestream:', error);
                alert('Error with livestream');
            }
        }

        function viewStream(deviceId) {
            // Open stream data in a new window/tab
            const streamUrl = `/api/stream-data?device_id=${deviceId}`;
            window.open(streamUrl, '_blank', 'width=800,height=600');
        }
            </script>
        </body>
        </html>
    "#)
}

/// Get list of connected cameras
async fn cameras_handler(
    State(state): State<WebState>,
) -> Json<Vec<CameraInfo>> {
    let sessions = state.camera_sessions.read().await;
    let mut cameras = Vec::new();
    
    for (device_id, session) in sessions.iter() {
        let now = std::time::Instant::now();
        let time_since_keepalive = now.duration_since(session.last_keepalive);
        
        // Consider camera offline if no keepalive for more than 60 seconds
        let status = if time_since_keepalive.as_secs() < 60 {
            "online"
        } else {
            "offline"
        };
        
        // Convert Instant to SystemTime for proper timestamp formatting
        let system_time = std::time::SystemTime::now() - time_since_keepalive;
        let datetime = chrono::DateTime::<chrono::Utc>::from(system_time);
        
        cameras.push(CameraInfo {
            device_id: device_id.clone(),
            last_keepalive: datetime.to_rfc3339(),
            status: status.to_string(),
            streaming: session.streaming,
            stream_data_size: session.stream_buffer.len(),
        });
    }
    
    Json(cameras)
}

/// Take snapshot from camera (placeholder)
async fn snapshot_handler(
    State(_state): State<WebState>,
    Json(request): Json<SnapshotRequest>,
) -> Json<serde_json::Value> {
    info!("📸 Web request: Take snapshot from camera {}", request.device_id);
    
    // TODO: Implement actual snapshot functionality
    // This is a placeholder that will be integrated later
    
    Json(serde_json::json!({
        "success": true,
        "message": "Snapshot functionality will be implemented later",
        "device_id": request.device_id,
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Start livestream from camera
async fn livestream_handler(
    State(state): State<WebState>,
    Json(request): Json<LivestreamRequest>,
) -> Json<serde_json::Value> {
    info!("📺 Web request: Start livestream from camera {}", request.device_id);
    
    // Find the camera session
    let mut sessions = state.camera_sessions.write().await;
    if let Some(session) = sessions.get_mut(&request.device_id) {
        if session.streaming {
            // Stop streaming
            session.stop_streaming();
            info!("📺 Stopped streaming for camera {}", request.device_id);
            
            Json(serde_json::json!({
                "success": true,
                "message": "Streaming stopped",
                "device_id": request.device_id,
                "streaming": false,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        } else {
            // Queue code 11 command to start the complete streaming protocol sequence
            info!("📺 Queueing code 11 command to start streaming protocol for camera {}", request.device_id);
            session.pending_command = Some("code11".to_string());
            
            info!("📺 Code 11 command queued for camera {} - will trigger complete protocol sequence", request.device_id);
            
            Json(serde_json::json!({
                "success": true,
                "message": "Code 11 command queued - will start complete streaming protocol sequence",
                "device_id": request.device_id,
                "streaming": false,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    } else {
        Json(serde_json::json!({
            "success": false,
            "message": "Camera not found",
            "device_id": request.device_id,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }
}

/// Get stream data for a camera
async fn stream_data_handler(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let device_id = params.get("device_id").unwrap_or(&"unknown".to_string()).clone();
    info!("📹 Web request: Get stream data for camera {}", device_id);
    
    let sessions = state.camera_sessions.read().await;
    if let Some(session) = sessions.get(&device_id) {
        if session.streaming && !session.stream_buffer.is_empty() {
            // Return the stream data as binary
            let data = session.get_stream_data();
            axum::response::Response::builder()
                .header("Content-Type", "application/octet-stream")
                .header("Cache-Control", "no-cache")
                .body(axum::body::Body::from(data.to_vec()))
                .unwrap()
        } else {
            // No stream data available
            axum::response::Response::builder()
                .status(404)
                .body(axum::body::Body::from("No stream data available"))
                .unwrap()
        }
    } else {
        // Camera not found
        axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("Camera not found"))
            .unwrap()
    }
}

/// Health check endpoint
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "a9-v720-web-server",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Send code 11 command to camera
async fn send_code11_handler(
    State(state): State<WebState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let device_id = params.get("device_id").unwrap_or(&"0800c001XPTN".to_string()).clone();
    info!("🎬 Manual code 11 request for camera {}", device_id);
    
    // Queue the code 11 command for processing through the bounded queue system
    let result = {
        let mut sessions = state.camera_sessions.write().await;
        if let Some(session) = sessions.get_mut(&device_id) {
            let camera_ip = session.addr.ip();
            let camera_port = session.addr.port();
            info!("🎬 Found camera {} at IP {}:{}", device_id, camera_ip, camera_port);
            
            debug!("🎬 About to queue code 11 command for camera {} (current pending: {:?})", device_id, session.pending_command);
            session.pending_command = Some("code11".to_string());
            debug!("🎬 After queuing code 11 command for camera {} (new pending: {:?})", device_id, session.pending_command);
            info!("🎬 Code 11 command queued for camera {} (IP: {})", device_id, camera_ip);
            
            Json(serde_json::json!({
                "success": true,
                "message": "Code 11 command queued successfully",
                "device_id": device_id,
                "camera_port": camera_port,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        } else {
            warn!("🎬 Camera {} not found", device_id);
            Json(serde_json::json!({
                "success": false,
                "message": "Camera not found",
                "device_id": device_id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    };
    result
}

/// Start the web server
pub async fn start_web_server(
    port: u16,
    camera_sessions: Arc<RwLock<HashMap<String, CameraSession>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🌐 Starting web server on port {}", port);
    let state = WebState {
        camera_sessions,
    };
    let app = create_web_routes(state);
    let addr = format!("0.0.0.0:{}", port);
    info!("🌐 Attempting to bind web server to {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("🌐 Web server listening on {}", addr);
    axum::serve(listener, app).await?;
    
    Ok(())
}