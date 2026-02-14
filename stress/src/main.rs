mod config;
mod phase1;
mod phase2;
mod state;

use config::{ActionDistributions, SimulationConfig};
use log::{error, info};
use state::AppState;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("Starting Stress Tester...");

    // 1. Load Configuration
    let config = SimulationConfig::default();
    let distributions = ActionDistributions::default();
    info!("Configuration loaded. Target users: {}", config.user_count);

    // 2. Initialize State
    let state = Arc::new(AppState::new());

    // 3. Phase 1: Preparation (HTTP)
    info!("Starting Phase 1: Preparation...");
    if let Err(e) = phase1::run(state.clone(), &config, &distributions).await {
        error!("Phase 1 failed: {}", e);
        return Err(e);
    }
    info!("Phase 1 Complete.");

    // 4. Phase 2: Execution (WebSocket)
    info!("Starting Phase 2: Execution...");
    if let Err(e) = phase2::run(state.clone(), &config, &distributions).await {
        error!("Phase 2 failed: {}", e);
        return Err(e);
    }
    info!("Phase 2 Complete.");

    Ok(())
}
