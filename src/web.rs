use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::{config::AppConfig, CameraSessions};

#[derive(Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub camera_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub success: bool,
    pub message: String,
    pub image_data: Option<String>, // Base64 encoded image
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CameraInfo {
    pub device_id: String,
    pub ip_address: String,
    pub status: String,
    pub last_seen: String,
    pub protocol_state: String,
}

#[derive(Serialize, Deserialize)]
pub struct CameraListResponse {
    pub cameras: Vec<CameraInfo>,
    pub total_count: usize,
}

pub struct WebServerState {
    pub camera_sessions: CameraSessions,
    #[allow(dead_code)]
    pub config: AppConfig,
}

/// Create the web server router with all endpoints
pub fn create_web_router(state: WebServerState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/api/cameras", get(list_cameras))
        .route("/api/camera/:camera_id/snapshot", get(get_snapshot))
        .route("/api/camera/:camera_id/snapshot", post(request_snapshot))
        .route("/api/camera/:camera_id/stream", get(get_stream))
        .route("/camera/:camera_id", get(camera_detail_page))
        .route("/static/style.css", get(serve_css))
        .layer(CorsLayer::permissive())
        .with_state(Arc::new(state))
}

/// Main index page showing camera list
async fn index_handler(State(state): State<Arc<WebServerState>>) -> impl IntoResponse {
    let sessions = state.camera_sessions.read().await;
    let cameras: Vec<CameraInfo> = sessions
        .iter()
        .map(|(device_id, session)| CameraInfo {
            device_id: device_id.clone(),
            ip_address: session.ip_address.to_string(),
            status: if session.is_connected() { "Connected".to_string() } else { "Disconnected".to_string() },
            last_seen: session.last_seen.format("%Y-%m-%d %H:%M:%S").to_string(),
            protocol_state: format!("{:?}", session.protocol_state),
        })
        .collect();

    let html = generate_index_html(&cameras);
    Html(html)
}

/// Camera detail page with snapshot and stream options
async fn camera_detail_page(
    Path(camera_id): Path<String>,
    State(state): State<Arc<WebServerState>>,
) -> impl IntoResponse {
    let sessions = state.camera_sessions.read().await;
    
    if let Some(session) = sessions.get(&camera_id) {
        let camera_info = CameraInfo {
            device_id: camera_id.clone(),
            ip_address: session.ip_address.to_string(),
            status: if session.is_connected() { "Connected".to_string() } else { "Disconnected".to_string() },
            last_seen: session.last_seen.format("%Y-%m-%d %H:%M:%S").to_string(),
            protocol_state: format!("{:?}", session.protocol_state),
        };
        
        let html = generate_camera_detail_html(&camera_info);
        Html(html)
    } else {
        Html(format!("<h1>Camera {} not found</h1>", camera_id))
    }
}

/// API endpoint to list all cameras
async fn list_cameras(State(state): State<Arc<WebServerState>>) -> impl IntoResponse {
    let sessions = state.camera_sessions.read().await;
    let cameras: Vec<CameraInfo> = sessions
        .iter()
        .map(|(device_id, session)| CameraInfo {
            device_id: device_id.clone(),
            ip_address: session.ip_address.to_string(),
            status: if session.is_connected() { "Connected".to_string() } else { "Disconnected".to_string() },
            last_seen: session.last_seen.format("%Y-%m-%d %H:%M:%S").to_string(),
            protocol_state: format!("{:?}", session.protocol_state),
        })
        .collect();

    let response = CameraListResponse {
        total_count: cameras.len(),
        cameras,
    };

    Json(response)
}

/// API endpoint to request a snapshot from a camera
async fn request_snapshot(
    Path(camera_id): Path<String>,
    State(state): State<Arc<WebServerState>>,
) -> impl IntoResponse {
    info!("📸 Snapshot request for camera: {}", camera_id);
    
    let sessions = state.camera_sessions.read().await;
    
    if let Some(_session) = sessions.get(&camera_id) {
        // TODO: Implement actual snapshot capture from camera
        // For now, return a placeholder response
        let response = SnapshotResponse {
            success: false,
            message: "Snapshot functionality not yet implemented".to_string(),
            image_data: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        
        Json(response)
    } else {
        let response = SnapshotResponse {
            success: false,
            message: format!("Camera {} not found", camera_id),
            image_data: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        
        Json(response)
    }
}

/// API endpoint to get the latest snapshot from a camera
async fn get_snapshot(
    Path(camera_id): Path<String>,
    State(state): State<Arc<WebServerState>>,
) -> impl IntoResponse {
    info!("📷 Get snapshot for camera: {}", camera_id);
    
    let sessions = state.camera_sessions.read().await;
    
    if let Some(_session) = sessions.get(&camera_id) {
        // TODO: Return actual snapshot image
        // For now, return a placeholder response
        let response = SnapshotResponse {
            success: false,
            message: "Snapshot retrieval not yet implemented".to_string(),
            image_data: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        
        Json(response)
    } else {
        let response = SnapshotResponse {
            success: false,
            message: format!("Camera {} not found", camera_id),
            image_data: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        
        Json(response)
    }
}

/// API endpoint for live stream (placeholder)
async fn get_stream(
    Path(camera_id): Path<String>,
    State(_state): State<Arc<WebServerState>>,
) -> impl IntoResponse {
    warn!("🎥 Live stream request for camera: {} (not implemented)", camera_id);
    
    // TODO: Implement MJPEG stream
    (
        StatusCode::NOT_IMPLEMENTED,
        "Live streaming not yet implemented"
    )
}

/// Serve CSS styles
async fn serve_css() -> impl IntoResponse {
    let css = r#"
/* A9 V720 Camera Management - Centered Full Width CSS */
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #f5f5f5; }
.container { max-width: 100%; margin: 0 auto; padding: 20px 40px; }
.header { background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 30px 0; text-align: center; margin-bottom: 30px; }
.camera-overview { background: white; border-radius: 12px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); padding: 30px; }
.camera-table { width: 100%; border-collapse: collapse; margin-top: 20px; }
.camera-table th, .camera-table td { padding: 15px; text-align: left; border-bottom: 1px solid #e1e5e9; }
.camera-table th { background: #f8f9fa; font-weight: 600; color: #374151; }
.camera-table tr:hover { background: #f8f9fa; }
.status { padding: 6px 12px; border-radius: 20px; font-size: 12px; font-weight: 600; }
.status.connected { background: #c6f6d5; color: #22543d; }
.status.disconnected { background: #fed7d7; color: #742a2a; }
.btn { background: #667eea; color: white; border: none; padding: 8px 16px; border-radius: 6px; cursor: pointer; margin: 2px; text-decoration: none; display: inline-block; font-size: 14px; }
.btn:hover { background: #5a67d8; }
.btn-secondary { background: #48bb78; }
.btn-secondary:hover { background: #38a169; }
.actions { white-space: nowrap; }
h1 { margin-bottom: 0; }
h2 { color: #374151; margin-bottom: 20px; }
"#;
    (
        [(header::CONTENT_TYPE, "text/css")],
        css
    )
}

/// Generate HTML for the main index page
fn generate_index_html(cameras: &[CameraInfo]) -> String {
    // Sort cameras by IP address
    let mut sorted_cameras = cameras.to_vec();
    sorted_cameras.sort_by(|a, b| {
        // Parse IP addresses for proper numeric sorting
        let parse_ip = |ip: &str| -> Vec<u32> {
            ip.split('.').map(|s| s.parse::<u32>().unwrap_or(0)).collect()
        };
        parse_ip(&a.ip_address).cmp(&parse_ip(&b.ip_address))
    });

    let camera_rows: String = sorted_cameras
        .iter()
        .map(|camera| {
            format!(
                r#"
                <tr>
                    <td>{}</td>
                    <td>{}</td>
                    <td><span class="status {}">{}</span></td>
                    <td>{}</td>
                    <td>{}</td>
                    <td class="actions">
                        <a href="/api/camera/{}/snapshot" class="btn btn-secondary">📸 Snapshot</a>
                        <a href="/api/camera/{}/stream" class="btn">📹 Live Stream</a>
                    </td>
                </tr>
                "#,
                camera.device_id,
                camera.ip_address,
                if camera.status == "Connected" { "connected" } else { "disconnected" },
                camera.status,
                camera.last_seen,
                camera.protocol_state,
                camera.device_id,
                camera.device_id
            )
        })
        .collect();

    format!(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>A9 V720 Camera Management</title>
    <link rel="stylesheet" href="/static/style.css">
</head>
<body>
    <div class="header">
        <div class="container">
            <h1>🎥 A9 V720 Camera Management</h1>
            <p>Monitor and control your A9 V720 cameras</p>
        </div>
    </div>
    
    <div class="container">
        <div class="camera-overview">
            <h2>Connected Cameras ({} total)</h2>
            {}
                <table class="camera-table">
                    <thead>
                        <tr>
                            <th>Camera ID</th>
                            <th>IP Address</th>
                            <th>Status</th>
                            <th>Last Seen</th>
                            <th>Protocol State</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
            
        </div>
    </div>
    
    <script>
        // Auto-refresh every 30 seconds
        setTimeout(() => {{
            window.location.reload();
        }}, 30000);
    </script>
</body>
</html>
        "#,
        cameras.len(),
        if cameras.is_empty() { 
            "<p>No cameras currently connected. Make sure your cameras are powered on and connected to the network.</p>" 
        } else { "" },
        camera_rows
    )
}

/// Generate HTML for camera detail page
fn generate_camera_detail_html(camera: &CameraInfo) -> String {
    format!(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Camera {} - A9 V720 Management</title>
    <link rel="stylesheet" href="/static/style.css">
</head>
<body>
    <div class="container">
        <header>
            <h1>📹 Camera: {}</h1>
            <a href="/" class="button">← Back to Camera List</a>
        </header>
        
        <main>
            <div class="camera-details">
                <div class="detail-card">
                    <h3>Camera Information</h3>
                    <table>
                        <tr><td><strong>Device ID:</strong></td><td>{}</td></tr>
                        <tr><td><strong>IP Address:</strong></td><td>{}</td></tr>
                        <tr><td><strong>Status:</strong></td><td><span class="status {}">{}</span></td></tr>
                        <tr><td><strong>Last Seen:</strong></td><td>{}</td></tr>
                        <tr><td><strong>Protocol State:</strong></td><td>{}</td></tr>
                    </table>
                </div>
                
                <div class="action-card">
                    <h3>Camera Actions</h3>
                    <div class="action-buttons">
                        <button onclick="requestSnapshot()" class="button primary">📸 Take Snapshot</button>
                        <button onclick="viewStream()" class="button">🎥 View Live Stream</button>
                        <button onclick="refreshInfo()" class="button">🔄 Refresh Info</button>
                    </div>
                </div>
                
                <div class="snapshot-preview" id="snapshotPreview" style="display: none;">
                    <h3>Latest Snapshot</h3>
                    <img id="snapshotImage" src="" alt="Camera Snapshot" style="max-width: 100%; height: auto;">
                    <p id="snapshotInfo"></p>
                </div>
            </div>
        </main>
        
        <footer>
            <p>A9 V720 Rust Server - Camera: {}</p>
        </footer>
    </div>
    
    <script>
        async function requestSnapshot() {{
            const response = await fetch('/api/camera/{}/snapshot', {{
                method: 'POST',
                headers: {{ 'Content-Type': 'application/json' }}
            }});
            
            const result = await response.json();
            
            if (result.success && result.image_data) {{
                document.getElementById('snapshotImage').src = 'data:image/jpeg;base64,' + result.image_data;
                document.getElementById('snapshotInfo').textContent = 'Snapshot taken at: ' + result.timestamp;
                document.getElementById('snapshotPreview').style.display = 'block';
            }} else {{
                alert('Snapshot failed: ' + result.message);
            }}
        }}
        
        function viewStream() {{
            window.open('/api/camera/{}/stream', '_blank');
        }}
        
        function refreshInfo() {{
            window.location.reload();
        }}
    </script>
</body>
</html>
        "#,
        camera.device_id,
        camera.device_id,
        camera.device_id,
        camera.ip_address,
        if camera.status == "Connected" { "connected" } else { "disconnected" },
        camera.status,
        camera.last_seen,
        camera.protocol_state,
        camera.device_id,
        camera.device_id,
        camera.device_id
    )
}
