use crate::db;
use crate::{State, auth::create_jwt};
use actix_web::{Error, HttpResponse, web};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct Register {
    username: String,
    password: String,
    email: String,
}

// Authentication

async fn login(form: web::Form<Login>, state: State) -> Result<HttpResponse, Error> {
    let result = db::login(&state.pool, &form.username, &form.password).await;

    if let Some(user_id) = result {
        let token = create_jwt(user_id, form.username.clone());

        Ok(HttpResponse::Ok()
            .cookie(
                actix_web::cookie::Cookie::build("session", &token)
                    .path("/")
                    .http_only(true)
                    .finish(),
            )
            .finish())
    } else {
        Ok(HttpResponse::Unauthorized().finish())
    }
}

async fn register(form: web::Form<Register>, state: State) -> Result<HttpResponse, Error> {
    let result = db::register(&state.pool, &form.username, &form.email, &form.password).await;

    if let Ok(user_id) = result {
        let token = create_jwt(user_id, form.username.clone());

        Ok(HttpResponse::Ok()
            .cookie(
                actix_web::cookie::Cookie::build("session", &token)
                    .path("/")
                    .http_only(true)
                    .finish(),
            )
            .finish())
    } else {
        Ok(HttpResponse::InternalServerError().finish())
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/login", web::post().to(login))
        .route("/register", web::post().to(register));
}
