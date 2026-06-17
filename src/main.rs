#![allow(unused_must_use)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::lockmap::LockMap;
use crate::message::service::MessageService;
use crate::message::snowflake::SnowflakeGenerator;
use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use dashmap::DashMap;
use dotenv::dotenv;
use google_cloud_storage::client::{Client, ClientConfig};
use nohash_hasher::BuildNoHashHasher;
use once_cell::sync::Lazy;
use sqlx::PgPool;
use state::app::AppState;
use std::env;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub type State = web::Data<AppState>;

mod db;
mod id;
mod lockmap;
mod mail;
mod message;
mod middleware;
mod msgpack;
mod ratelimiter;
mod route;
mod state;

#[get("/ping")]
async fn ping() -> impl Responder {
    HttpResponse::Ok().body("PONG")
}

async fn db_connection() -> Result<PgPool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Migration error");

    Ok(pool)
}

pub static DOMAIN: Lazy<String> = Lazy::new(|| env::var("DOMAIN").expect("DOMAIN must be set"));

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    #[cfg(feature = "mail")]
    tokio::spawn(route::auth::clear_otp_schedular());

    let pool = db_connection()
        .await
        .expect("Failed to connect to database");

    let message_store = db::message::MessageStore::open("data/messages")
        .expect("Failed to open RocksDB message store");

    let messages = MessageService::new(message_store);

    let config = ClientConfig::default().with_auth().await.unwrap();
    let gcs_client = web::Data::new(Client::new(config));

    let hasher = BuildNoHashHasher::<id::id>::default();

    let shutdown = CancellationToken::new();
    let tracker = tokio_util::task::TaskTracker::new();

    let state = web::Data::new(AppState {
        users: DashMap::with_hasher_and_shard_amount(hasher.clone(), 8),
        groups: DashMap::with_hasher_and_shard_amount(hasher.clone(), 8),
        voice_direct: DashMap::with_hasher_and_shard_amount(hasher.clone(), 8),
        user_locks: LockMap::new(),
        group_locks: LockMap::new(),
        pool,
        snowflake: SnowflakeGenerator::new(1),
        messages,
        shutdown: shutdown.clone(),
        tracker: tracker.clone(),
    });

    let state_ws = state.clone();
    tokio::spawn(async move {
        log::info!("WebSocket server listening on {}", 8081);
        if let Err(e) = route::ws::listen(8081, state_ws).await {
            log::error!("WebSocket server error: {:?}", e);
        }
    });

    let governor = GovernorConfigBuilder::default()
        .requests_per_second(50)
        .burst_size(200)
        .key_extractor(ratelimiter::UserKeyExtractor)
        .finish()
        .unwrap();

    let governor_upload_fast = GovernorConfigBuilder::default()
        .seconds_per_request(60 / 20)
        .burst_size(20)
        .key_extractor(ratelimiter::UserKeyExtractor)
        .finish()
        .unwrap();

    let governor_upload_slow = GovernorConfigBuilder::default()
        .seconds_per_request(60 * 60 / 100)
        .burst_size(100)
        .key_extractor(ratelimiter::UserKeyExtractor)
        .finish()
        .unwrap();

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin_fn(|origin, _req_head| {
                matches!(
                    origin.as_bytes(),
                    b"http://localhost:5173"
                        | b"https://tauri.localhost"
                        | b"http://tauri.localhost"
                        | b"https://thiscrow.net"
                        | b"https://www.thiscrow.net"
                        | b"tauri://localhost"
                )
            })
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE"])
            .allowed_headers(vec![
                actix_web::http::header::AUTHORIZATION,
                actix_web::http::header::ACCEPT,
                actix_web::http::header::CONTENT_TYPE,
            ])
            .supports_credentials()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(state.clone())
            .app_data(gcs_client.clone())
            .service(
                web::scope("/api")
                    .configure(route::auth::configure)
                    .service(ping)
                    .service(
                        web::scope("")
                            .wrap(middleware::AuthMiddleware)
                            .service(
                                web::scope("/upload")
                                    .wrap(Governor::new(&governor_upload_fast))
                                    .wrap(Governor::new(&governor_upload_slow))
                                    .configure(route::upload::configure),
                            )
                            .configure(route::state::configure)
                            .configure(route::info::configure)
                            .configure(route::message::configure)
                            .configure(route::invitation::configure)
                            .configure(route::group::configure),
                    )
                    .wrap(Governor::new(&governor)),
            )
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await?;

    log::info!("HTTP server stopped, draining WebSocket sessions");
    shutdown.cancel();
    tracker.close();

    tokio::select! {
        _ = tracker.wait() => log::info!("All sessions drained"),
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            log::warn!("Drain timeout, forcing exit");
        }
    }

    Ok(())
}
