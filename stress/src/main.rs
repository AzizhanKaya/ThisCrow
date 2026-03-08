mod api;
mod config;
mod ws;

use api::ApiClient;
use config::Config;
use futures::{StreamExt, stream};
use indicatif::{ProgressBar, ProgressStyle};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config = Arc::new(Config::default());

    let local_ips: Vec<IpAddr> = config
        .local_ips
        .iter()
        .map(|s| IpAddr::from_str(s).expect("Invalid IP address in config"))
        .collect();

    let clients = local_ips
        .iter()
        .map(|ip| ApiClient::new(config.api_url.clone(), *ip).unwrap())
        .collect::<Vec<ApiClient>>();

    println!(
        "Starting stress test: {} users with {} parallel requests",
        config.users_to_create, config.concurrency
    );

    let remote_addr = config.addr.parse::<SocketAddr>().unwrap();

    let tasks = (0..config.users_to_create).map(|i| {
        let client = clients[i % clients.len()].clone();

        let local_ip = local_ips[i % local_ips.len()];

        async move {
            match client.register_user(i).await {
                Ok(Some(token)) => {
                    tokio::spawn(async move {
                        if let Err(e) = ws::connect_and_listen(local_ip, remote_addr, token).await {
                            log::warn!("{}", e);
                        }
                    });
                }
                Ok(None) => {
                    log::warn!("User {} registered but no session cookie received", i);
                }
                Err(e) => {
                    log::warn!("{}", e);
                }
            }
        }
    });

    let pb = ProgressBar::new(config.users_to_create as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap()
        .progress_chars("##-"),
    );

    let mut stream = stream::iter(tasks).buffer_unordered(config.concurrency);

    while let Some(_) = stream.next().await {
        pb.inc(1);
    }
    pb.finish_with_message("Registration Complete");

    println!("All users processed. Keeping WS connections open. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    Ok(())
}
