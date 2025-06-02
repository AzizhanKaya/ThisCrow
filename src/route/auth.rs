use crate::db;
use crate::{State, auth::create_jwt};
use actix_web::{Error, HttpResponse, error, web};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct Register {
    username: String,
    name: String,
    password: String,
    email: String,
}

// Authentication

async fn login(form: web::Form<Login>, state: State) -> Result<HttpResponse, Error> {
    let result = db::login(&state.pool, &form.username, &form.password).await;

    if let Some(user) = result {
        let token = create_jwt(user.id, form.username.clone());

        Ok(HttpResponse::Ok()
            .cookie(
                actix_web::cookie::Cookie::build("session", &token)
                    .path("/")
                    .http_only(true)
                    .finish(),
            )
            .json(json!(user)))
    } else {
        Err(error::ErrorUnauthorized("Username or password is wrong."))
    }
}

async fn register(form: web::Form<Register>, state: State) -> Result<HttpResponse, Error> {
    let user = db::register(
        &state.pool,
        &form.username,
        &form.name,
        &form.email,
        &form.password,
    )
    .await
    .map_err(|e| error::ErrorInternalServerError(e))?;

    let token = create_jwt(user.id, form.username.clone());

    Ok(HttpResponse::Ok()
        .cookie(
            actix_web::cookie::Cookie::build("session", &token)
                .path("/")
                .http_only(true)
                .finish(),
        )
        .json(json!(user)))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register)),
    );
}
