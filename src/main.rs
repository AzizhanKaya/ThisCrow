#![allow(unused_must_use)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use actix_cors::Cors;
use actix_web::middleware::Logger;
use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use chrono::Utc;
use dashmap::DashMap;
use dotenv::dotenv;
use models::AppState;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::env;

pub type State = web::Data<AppState>;

mod models;

mod auth;

mod db;

mod route;

mod mail;

#[get("/ping")]
async fn ping() -> impl Responder {
    HttpResponse::Ok().body("PONG")
}

async fn db_connection() -> Result<PgPool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPool::connect(&database_url).await
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

    db::init_db(pool.clone()).await.expect("Failed to init db");

    let state = web::Data::new(AppState {
        users: DashMap::new(),
        chat_users: DashMap::new(),
        pool,
    });

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
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(state.clone())
            .service(
                web::scope("/api")
                    .configure(route::auth::configure)
                    .service(ping)
                    .configure(route::upload::configure)
                    .service(
                        web::scope("")
                            .wrap(auth::AuthMiddleware)
                            .route("/ws", web::get().to(route::message::ws))
                            .configure(route::state::configure)
                            .configure(route::event::configure),
                    ),
            )
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
