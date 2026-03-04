#![allow(unused_must_use)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::middleware::Logger;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, middleware::Compress, web};
use dashmap::DashMap;
use dotenv::dotenv;
use sqlx::PgPool;
use state::app::AppState;
use std::env;

use crate::lockmap::LockMap;
use crate::message::Snowflake;
use crate::message::service::MessageService;

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
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    dotenv().ok();

    route::upload::init();
    route::auth::clear_otp_schedular();

    let pool = db_connection()
        .await
        .expect("Failed to connect to database");

    let message_store = db::message::MessageStore::open("data/messages")
        .expect("Failed to open RocksDB message store");

    let messages = MessageService::new(message_store);

    let hasher = ahash::RandomState::new();

    let state = web::Data::new(AppState {
        users: DashMap::with_hasher_and_shard_amount(hasher.clone(), 512),
        groups: DashMap::with_hasher_and_shard_amount(hasher, 128),
        user_locks: LockMap::new(),
        group_locks: LockMap::new(),
        pool,
        snowflake: Snowflake::new(1),
        messages,
    });

    let _governor = GovernorConfigBuilder::default()
        .seconds_per_request(10)
        .burst_size(20)
        .key_extractor(ratelimiter::UserKeyExtractor)
        .finish()
        .unwrap();

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
            .app_data(state.clone())
            .service(
                web::scope("/api")
                    // .wrap(Governor::new(&governor))
                    .configure(route::auth::configure)
                    .service(ping)
                    .service(
                        web::scope("")
                            .wrap(middleware::AuthMiddleware)
                            .configure(route::upload::configure)
                            .route("/ws", web::get().to(route::ws::ws))
                            .configure(route::state::configure)
                            .configure(route::info::configure),
                    ),
            )
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
