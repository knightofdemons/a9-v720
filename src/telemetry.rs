use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

/// Initialize telemetry with debug logging
pub fn init_telemetry() {
    // Configure tracing with debug level and systemd journal output
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,a9_v720_server=debug"));
    
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_max_level(Level::DEBUG)
        .init();
    
    info!("📊 Telemetry initialized with debug logging");
}
