use crate::config::{ActionDistributions, SimulationConfig};
use crate::state::AppState;
use futures::{SinkExt, StreamExt};
use log::{debug, error, info};
use rand::prelude::*;
use rand_distr::Distribution;
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

// Placeholder WS URL
const WS_BASE_URL: &str = "ws://localhost:3000/ws";

pub async fn run(
    state: Arc<AppState>,
    config: &SimulationConfig,
    dist: &ActionDistributions,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting Phase 2: 100,000 WebSocket Connections...");

    let mut tasks = Vec::new();

    let user_ids: Vec<u64> = state.users.iter().map(|kv| *kv.key()).collect();

    let chunk_size = 500;

    for chunk in user_ids.chunks(chunk_size) {
        for &user_id in chunk {
            let state = state.clone();
            let config = config.clone();
            let dist = dist.clone();

            // Spawn user task
            let handle = tokio::spawn(async move {
                user_behavior_loop(user_id, state, config, dist).await;
            });
            tasks.push(handle);
        }

        sleep(Duration::from_millis(50)).await;
    }

    info!("All user tasks spawned. Waiting for simulation...");

    tokio::signal::ctrl_c().await?;
    info!("Received Ctrl+C, shutting down...");

    Ok(())
}

async fn user_behavior_loop(
    user_id: u64,
    state: Arc<AppState>,
    config: SimulationConfig,
    dist: ActionDistributions,
) {
    let cookie = if let Some(u) = state.users.get(&user_id) {
        u.cookie.clone()
    } else {
        return;
    };

    // Prepare Request with Cookie
    let mut request = WS_BASE_URL.into_client_request().unwrap();
    let headers = request.headers_mut();
    headers.insert("Cookie", HeaderValue::from_str(&cookie).unwrap());

    // Connect WS
    let (ws_stream, _) = match connect_async(request).await {
        Ok(s) => s,
        Err(e) => {
            debug!("User {} failed to connect: {}", user_id, e);
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();

    // Handle incoming messages (ignore or log)
    tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            // Just drain
        }
    });

    let mut rng = rand::rng();

    // Initial delays
    let mut next_dm_delay = Duration::from_secs_f64(dist.dm_interval.sample(&mut rng));
    let mut next_group_delay = Duration::from_secs_f64(dist.group_msg_interval.sample(&mut rng));

    loop {
        tokio::select! {
            _ = sleep(next_dm_delay) => {
                // Send DM
                send_dm(user_id, &state, &dist, &mut rng, &mut write).await;
                // Schedule next
                next_dm_delay = Duration::from_secs_f64(dist.dm_interval.sample(&mut rng));
            }
            _ = sleep(next_group_delay) => {
                // Send Group Msg
                send_group_msg(user_id, &state, &dist, &mut rng, &mut write).await;
                // Schedule next
                next_group_delay = Duration::from_secs_f64(dist.group_msg_interval.sample(&mut rng));
            }
        }
    }
}

async fn send_dm<W>(
    user_id: u64,
    state: &Arc<AppState>,
    dist: &ActionDistributions,
    rng: &mut impl Rng,
    write: &mut W,
) where
    W: SinkExt<Message> + Unpin,
{
    // Pick a friend based on affinity
    // 1. Get friends
    // We shouldn't hold the reference to DashMap return value across await points.
    let friends_list = if let Some(friends) = state.friends.get(&user_id) {
        friends.clone() // Clone vector: (FriendID, Affinity)
    } else {
        return;
    };

    if friends_list.is_empty() {
        return;
    }

    // Weighted Choice
    // dist.friend_affinity is just distribution used for generation.
    // The actual weights are stored in friends_list definition (u64, f64).

    let total_weight: f64 = friends_list.iter().map(|(_, w)| *w).sum();
    // Use WeightedIndex? Or manual.
    // WeightedIndex requires creating it which might be slow in loop?
    // Implementation: pick random 0..total_weight
    let target = rng.random::<f64>() * total_weight;
    let mut current = 0.0;
    let mut selected_friend = friends_list[0].0;

    for (fid, w) in &friends_list {
        current += *w;
        if current >= target {
            selected_friend = *fid;
            break;
        }
    }

    // Compose Message
    let msg_json = serde_json::json!({
        "type": "DM",
        "to": selected_friend,
        "content": "Hello friend!"
    })
    .to_string();

    let _ = write.send(Message::Text(msg_json.into())).await;
}

async fn send_group_msg<W>(
    user_id: u64,
    state: &Arc<AppState>,
    dist: &ActionDistributions,
    rng: &mut impl Rng,
    write: &mut W,
) where
    W: SinkExt<Message> + Unpin,
{
    // Pick a group
    let groups = if let Some(g) = state.user_groups.get(&user_id) {
        g.clone()
    } else {
        return;
    };

    if groups.is_empty() {
        return;
    }

    let group_id = groups.choose(rng).unwrap();

    // Pick a channel in group
    // Need to look up group to get channels
    // Since we don't store channel struct in group deeply?
    // In state.rs: Group { channel_ids: Vec<u64> }

    let channels = if let Some(g) = state.groups.get(group_id) {
        g.channel_ids.clone()
    } else {
        return;
    };

    if channels.is_empty() {
        return;
    }

    let channel_id = channels.choose(rng).unwrap();

    let msg_json = serde_json::json!({
        "type": "GroupMsg",
        "channel_id": channel_id,
        "content": "Hello group!"
    })
    .to_string();

    let _ = write.send(Message::Text(msg_json.into())).await;
}
