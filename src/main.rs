#![allow(unused_must_use)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::lockmap::LockMap;
use crate::message::service::MessageService;
use crate::message::snowflake::SnowflakeGenerator;
use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::middleware::Logger;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, middleware::Compress, web};
use dashmap::DashMap;
use dotenv::dotenv;
use once_cell::sync::Lazy;
use sqlx::PgPool;
use state::app::AppState;
use std::env;
use tokio::runtime::{Builder, Runtime};

pub type State = web::Data<AppState>;

mod db;
mod id;
mod lockmap;
#[cfg(feature = "mail")]
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
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
}

pub static TOKIO_RT: Lazy<Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Tokio runtime error")
});

fn main() -> std::io::Result<()> {
    env_logger::init();
    dotenv().ok();

    route::upload::init();
    route::auth::clear_otp_schedular();

    let pool = TOKIO_RT
        .block_on(db_connection())
        .expect("Failed to connect to database");

    let message_store = db::message::MessageStore::open("data/messages")
        .expect("Failed to open RocksDB message store");

    let messages = MessageService::new(message_store);

    let hasher = ahash::RandomState::new();

    let state = web::Data::new(AppState {
        users: DashMap::with_hasher_and_shard_amount(hasher.clone(), 8),
        groups: DashMap::with_hasher_and_shard_amount(hasher, 8),
        user_locks: LockMap::new(),
        group_locks: LockMap::new(),
        pool,
        snowflake: SnowflakeGenerator::new(1),
        messages,
    });

    for _ in 0..4 {
        let state = state.clone();

        std::thread::spawn(move || {
            let mut rt = monoio::RuntimeBuilder::<monoio::IoUringDriver>::new()
                .enable_all()
                .build()
                .expect("Monoio runtime error");

            rt.block_on(route::ws::listen(8081, state));
        });
    }

    let governor = GovernorConfigBuilder::default()
        .seconds_per_request(10)
        .burst_size(50)
        .key_extractor(ratelimiter::UserKeyExtractor)
        .finish()
        .unwrap();

    TOKIO_RT.block_on(
        HttpServer::new(move || {
            let cors = Cors::default()
                .allowed_origin("http://localhost:5173")
                .allowed_methods(vec!["GET", "POST", "PUT", "DELETE"])
                .allowed_headers(vec![
                    actix_web::http::header::AUTHORIZATION,
                    actix_web::http::header::ACCEPT,
                    actix_web::http::header::CONTENT_TYPE,
                ])
                .supports_credentials()
                .max_age(3600);

            App::new()
                .wrap(Compress::default())
                .wrap(Logger::default())
                .wrap(cors)
                .app_data(web::PayloadConfig::new(1024))
                .app_data(state.clone())
                .service(
                    web::scope("/api")
                        .configure(route::auth::configure)
                        .service(ping)
                        .service(
                            web::scope("")
                                .wrap(middleware::AuthMiddleware)
                                .wrap(Governor::new(&governor))
                                .configure(route::upload::configure)
                                .configure(route::state::configure)
                                .configure(route::info::configure),
                        ),
                )
        })
        .bind("0.0.0.0:8080")?
        .run(),
    )
}
