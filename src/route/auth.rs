use crate::db;
use crate::msgpack::MsgPack;
use crate::{State, middleware::create_jwt};
use actix_web::{
    Error, HttpResponse, cookie::Cookie, cookie::time::Duration as CookieDuration, error, web,
};

#[cfg(feature = "mail")]
use dashmap::DashMap;
use log::warn;
use serde::Deserialize;

#[cfg(feature = "mail")]
use {
    crate::DOMAIN,
    crate::mail,
    actix_web::http::header,
    chrono::{DateTime, Duration, Utc},
    once_cell::sync::Lazy,
    rand::{Rng, distr::Alphanumeric},
    tokio::time::{self, Duration as TokioDuration},
};

// Authentication

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

async fn login(login: MsgPack<Login>, state: State) -> Result<HttpResponse, Error> {
    let result = db::user::login(&state.pool, &login.username, &login.password).await;

    if let Some(user) = result {
        let token = create_jwt(user.id);

        Ok(HttpResponse::Ok()
            .cookie(
                Cookie::build("session", &token)
                    .path("/")
                    .http_only(true)
                    .max_age(CookieDuration::days(1))
                    .finish(),
            )
            .finish())
    } else {
        Err(error::ErrorUnauthorized("Username or password is wrong."))
    }
}

#[cfg(feature = "mail")]
static EMAIL_OTP_MAP: Lazy<DashMap<String, (Register, DateTime<Utc>)>> =
    Lazy::new(|| DashMap::new());

#[derive(Deserialize)]
struct Register {
    username: String,
    name: String,
    password: String,
    email: String,
    public_key: Vec<u8>,
}

async fn register(register: MsgPack<Register>, state: State) -> Result<HttpResponse, Error> {
    #[cfg(not(feature = "mail"))]
    {
        let id = db::user::register(
            &state.pool,
            &register.username,
            &register.name,
            &register.email,
            &register.password,
            &register.public_key,
        )
        .await
        .map_err(|e| {
            warn!("Error while register user: {}", e);
            error::ErrorInternalServerError("Error while registering")
        })?;

        let token = create_jwt(id);

        Ok(HttpResponse::Ok()
            .cookie(
                Cookie::build("session", &token)
                    .path("/")
                    .http_only(true)
                    .max_age(CookieDuration::days(1))
                    .finish(),
            )
            .finish())
    }

    #[cfg(feature = "mail")]
    {
        let otp: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();

        mail::send_email(
                &register.email,
                "ThisCrow Email Verification",
                format!(
                    r#"<a href="https://{}/api/auth/verify_email?email={}&otp={}">Verify your registration</a>"#,
                    *DOMAIN, register.email, otp)
            ).await.map_err(|_| error::ErrorInternalServerError("Can not send email"))?;

        EMAIL_OTP_MAP.insert(otp, (register.0, Utc::now()));
        Ok(HttpResponse::Ok().finish())
    }
}

#[cfg(feature = "mail")]
#[derive(Deserialize)]
struct VerifyEmail {
    otp: String,
    email: String,
}

#[cfg(feature = "mail")]
async fn verify_email(state: State, query: web::Query<VerifyEmail>) -> Result<HttpResponse, Error> {
    let (_otp, (user, exp)) = EMAIL_OTP_MAP
        .remove(&query.otp)
        .ok_or(error::ErrorUnauthorized("Invalid OTP"))?;

    if user.email != query.email {
        return Err(error::ErrorUnauthorized("Invalid OTP"));
    }

    if Utc::now() > exp + Duration::minutes(5) {
        return Err(error::ErrorUnauthorized("Expired token"));
    }

    let id = db::user::register(
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
    })?;

    let token = create_jwt(id);

    Ok(HttpResponse::Found()
        .append_header((header::LOCATION, "http://localhost:5173/"))
        .cookie(
            Cookie::build("session", &token)
                .path("/")
                .http_only(true)
                .max_age(CookieDuration::days(1))
                .finish(),
        )
        .finish())
}

#[cfg(feature = "mail")]
pub async fn clear_otp_schedular() {
    let mut interval = time::interval(TokioDuration::from_secs(300));

    loop {
        interval.tick().await;

        EMAIL_OTP_MAP.retain(|_otp, (_user, exp)| *exp + Duration::minutes(5) > Utc::now());
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    #[cfg(feature = "mail")]
    cfg.service(
        web::scope("/auth")
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register))
            .route("/verify_email", web::get().to(verify_email)),
    );

    #[cfg(not(feature = "mail"))]
    cfg.service(
        web::scope("/auth")
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register)),
    );
}
