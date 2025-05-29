#![allow(unused_must_use)]
#![allow(non_camel_case_types)]
use actix_files::NamedFile;
use actix_web::middleware::Logger;
use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use chrono::Utc;
use dashmap::DashMap;
use dotenv::dotenv;
use models::AppState;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::env;
use std::sync::Arc;

pub type State = web::Data<AppState>;

mod models;
use models::*;

mod auth;

mod db;
use db::init_db;

mod route;
use route::message::ws;
use route::webrtc::init_webrtc_api;

async fn index() -> actix_web::Result<NamedFile> {
    Ok(NamedFile::open("./index.html")?)
}

async fn db_connection() -> Result<PgPool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPool::connect(&database_url).await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    dotenv().ok();

    let pool = db_connection()
        .await
        .expect("Failed to connect to database");

    init_db(pool.clone()).await.expect("Failed to init db");

    let state = web::Data::new(AppState {
        users: DashMap::new(),
        chats: DashMap::new(),
        pool,
        rtc_api: init_webrtc_api(),
    });

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(state.clone())
            .route("/", web::get().to(index))
            .configure(route::auth::configure)
            .service(
                web::scope("")
                    .wrap(auth::AuthMiddleware)
                    .route("/ws", web::get().to(ws))
                    .configure(route::upload::configure)
                    .configure(route::webrtc::configure),
            )
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
