use crate::state::SharedState;
use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use once_cell::sync::Lazy;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::net::TcpSocket;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

static BASE_REQ: Lazy<Request<()>> = Lazy::new(|| {
    Request::builder()
        .method("GET")
        .uri("ws://127.0.0.1/api/ws")
        .header("Host", "127.0.0.1")
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .unwrap()
});

static WEBSOCKET_CONFIG: Lazy<WebSocketConfig> = Lazy::new(|| {
    WebSocketConfig::default()
        .max_frame_size(Some(256))
        .max_message_size(Some(256))
        .read_buffer_size(256)
        .write_buffer_size(128)
        .accept_unmasked_frames(true)
});

static BASE_TIME: Lazy<Instant> = Lazy::new(Instant::now);

pub async fn connect_and_listen(
    local_ip: IpAddr,
    remote_addr: SocketAddr,
    token: String,
    state: Arc<SharedState>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let socket = TcpSocket::new_v4()?;
    socket.set_recv_buffer_size(1024)?;
    socket.set_send_buffer_size(1024)?;

    socket.bind(SocketAddr::new(local_ip, 0))?;

    let stream = socket.connect(remote_addr).await?;
    let _ = stream.set_nodelay(true);

    let mut request = BASE_REQ.clone();

    request
        .headers_mut()
        .insert("Sec-WebSocket-Key", HeaderValue::from_str(&generate_key())?);

    request
        .headers_mut()
        .insert("Cookie", HeaderValue::from_str(&token)?);

    let (mut ws_stream, response) =
        tokio_tungstenite::client_async_with_config(request, stream, Some(*WEBSOCKET_CONFIG))
            .await?;

    drop(response);

    let mut ping_interval = tokio::time::interval(Duration::from_secs(10));

    state.active_users.fetch_add(1, Ordering::Relaxed);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                let _ = ws_stream.send(WsMessage::Close(None)).await;
                let _ = ws_stream.close(None).await;
                break;
            }
            _ = ping_interval.tick() => {
                let now = BASE_TIME.elapsed().as_micros() as u64;
                let payload = now.to_le_bytes().to_vec();
                if let Err(e) = ws_stream.send(WsMessage::Ping(payload.into())).await {
                    log::warn!("ping error: {}", e);
                    break;
                }
                state.total_pings_sent.fetch_add(1, Ordering::Relaxed);
            }
            msg = ws_stream.next() => {
                let Some(message_result) = msg else {
                    break;
                };
                match message_result {
                    Ok(msg) => {
                        match msg {
                            WsMessage::Close(c) => {
                                log::info!("{:?}", c);
                                break;
                            }
                            WsMessage::Pong(payload) => {
                                if payload.len() == 8 {
                                    let mut arr = [0u8; 8];
                                    arr.copy_from_slice(&payload);
                                    let sent_time = u64::from_le_bytes(arr);
                                    let now = BASE_TIME.elapsed().as_micros() as u64;
                                    if now > sent_time {
                                        let latency_ms = (now - sent_time) / 1000;
                                        state.add_latency(latency_ms);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        log::warn!("{}", e);
                        break;
                    }
                }
            }
        }
    }

    log::info!("disconnected");
    state.active_users.fetch_sub(1, Ordering::Relaxed);

    Ok(())
}
