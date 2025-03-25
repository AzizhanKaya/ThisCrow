use actix_web::{web, App, HttpServer};
use actix_web::middleware::Logger;
use sqlx::PgPool;
use serde::{Deserialize, Serialize};
use chrono::Utc;
use dotenv::dotenv;
use std::env;
use actix_ws;
use std::collections::HashMap;
use dashmap::DashMap;

mod models;
use models::*;

async fn db_connection() -> Result<PgPool, sqlx::Error> {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPool::connect(&database_url).await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    
    env_logger::init();

    let pool = db_connection().await.expect("Failed to connect to database");

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(pool.clone()))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
