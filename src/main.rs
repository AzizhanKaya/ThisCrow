#![allow(unused_must_use)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::lockmap::LockMap;
use crate::message::service::MessageService;
use crate::message::snowflake::SnowflakeGenerator;
use actix_cors::Cors;
use actix_governor::GovernorConfigBuilder;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use dashmap::DashMap;
use dotenv::dotenv;
use google_cloud_storage::client::{Client, ClientConfig};
use nohash_hasher::BuildNoHashHasher;
use once_cell::sync::Lazy;
use sqlx::PgPool;
use state::app::AppState;
use std::env;

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
    env_logger::init();
    dotenv().ok();

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

    let state = web::Data::new(AppState {
        users: DashMap::with_hasher_and_shard_amount(hasher.clone(), 8),
        groups: DashMap::with_hasher_and_shard_amount(hasher.clone(), 8),
        user_locks: LockMap::new(),
        group_locks: LockMap::new(),
        pool,
        snowflake: SnowflakeGenerator::new(1),
        messages,
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

    let governor_upload = GovernorConfigBuilder::default()
        .seconds_per_request(10)
        .burst_size(10)
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
                            //.wrap(Governor::new(&governor))
                            .configure(route::upload::configure)
                            .configure(route::state::configure)
                            .configure(route::info::configure)
                            .configure(route::message::configure)
                            .configure(route::invitation::configure),
                    ),
            )
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
