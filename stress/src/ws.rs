use futures_util::StreamExt;
use http::HeaderValue;
use once_cell::sync::Lazy;
use std::net::{IpAddr, SocketAddr};
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

pub async fn connect_and_listen(
    local_ip: IpAddr,
    remote_addr: SocketAddr,
    token: String,
) -> anyhow::Result<()> {
    let socket = TcpSocket::new_v4()?;
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

    while let Some(message_result) = ws_stream.next().await {
        match message_result {
            Ok(msg) => {
                if let WsMessage::Close(c) = msg {
                    log::info!("{:?}", c);
                    break;
                }
            }
            Err(e) => {
                log::warn!("{}", e);
                break;
            }
        }
    }

    log::info!("disconnected");

    Ok(())
}
