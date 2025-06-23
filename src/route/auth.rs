use crate::db;
use crate::mail;
use crate::{State, auth::create_jwt};
use actix_web::http::header;
use actix_web::{
    Error, HttpResponse, cookie::Cookie, cookie::time::Duration as CookieDuration, error, web,
};
use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use log::warn;
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use rand::Rng;
use rand::distr::Alphanumeric;
use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio::time::{self, Duration as TokioDuration};

// Authentication

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

async fn login(form: web::Form<Login>, state: State) -> Result<HttpResponse, Error> {
    let result = db::login(&state.pool, &form.username, &form.password).await;

    if let Some(user) = result {
        let token = create_jwt(user.id, form.username.clone());

        Ok(HttpResponse::Ok()
            .cookie(
                Cookie::build("session", &token)
                    .path("/")
                    .http_only(true)
                    .max_age(CookieDuration::days(1))
                    .finish(),
            )
            .json(user))
    } else {
        Err(error::ErrorUnauthorized("Username or password is wrong."))
    }
}

static EMAIL_OTP_MAP: Lazy<Mutex<HashMap<String, (Register, DateTime<Utc>)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
struct Register {
    username: String,
    name: String,
    password: String,
    email: String,
}

async fn register(form: web::Form<Register>, state: State) -> Result<HttpResponse, Error> {
    match db::has_registered(&state.pool, &form.username, &form.email).await {
        Ok(true) => {
            return Err(error::ErrorConflict("This email already in use"));
        }
        Err(e) => {
            return Err(error::ErrorInternalServerError(e));
        }
        _ => {}
    }

    let otp = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();

    mail::send_email(
        &form.email,
        "ThisCrow Email Verification",
        format!(
            r#"<a href="https://thiscrow.vate.world/verify_email?email={}&otp={}">Verify your registration</a>"#,
            form.email, otp
        ),
    ).await?;

    EMAIL_OTP_MAP
        .lock()
        .await
        .insert(otp, (form.into_inner(), Utc::now()));
    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
struct VerifyEmail {
    otp: String,
    email: String,
}

async fn verify_email(state: State, query: web::Query<VerifyEmail>) -> Result<HttpResponse, Error> {
    let mut otp_map = EMAIL_OTP_MAP.lock().await;

    let (user, exp) = otp_map
        .remove(&query.otp)
        .ok_or(error::ErrorUnauthorized("Invalid OTP"))?;

    if user.email != query.email {
        return Err(error::ErrorUnauthorized("Invalid OTP"));
    }

    if Utc::now() > exp + Duration::minutes(5) {
        return Err(error::ErrorUnauthorized("Expired token"));
    }

    let id = db::register(
        &state.pool,
        &user.username,
        &user.name,
        &user.email,
        &user.password,
    )
    .await
    .map_err(|e| {
        warn!("Error while register user: {}", e);
        error::ErrorInternalServerError("Error while registering")
    })?
    .id;

    let token = create_jwt(id, user.username.clone());

    Ok(HttpResponse::Found()
        .append_header((header::LOCATION, "/"))
        .cookie(
            Cookie::build("session", &token)
                .path("/")
                .http_only(true)
                .max_age(CookieDuration::days(1))
                .finish(),
        )
        .finish())
}

pub fn clear_otp_schedular() {
    static CLEAR_OTPS_ONCE: OnceCell<()> = OnceCell::new();

    if CLEAR_OTPS_ONCE.set(()).is_ok() {
        tokio::spawn(async {
            let mut interval = time::interval(TokioDuration::from_secs(300));

            loop {
                interval.tick().await;

                EMAIL_OTP_MAP
                    .lock()
                    .await
                    .retain(|_otp, (_user, exp)| *exp + Duration::minutes(5) > Utc::now());
            }
        });
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register))
            .route("/verify_email", web::get().to(verify_email)),
    );
}
