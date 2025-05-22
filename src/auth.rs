use crate::models::{State, User, id};
use actix_web::HttpMessage;
use actix_web::dev::{Service, ServiceResponse, Transform};
use actix_web::error::ErrorUnauthorized;
use actix_web::{Error, dev::ServiceRequest};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

// Authorization

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwtUser {
    pub id: id,
    pub username: String,
    pub exp: usize,
}

static JWT_SECRET: Lazy<Vec<u8>> = Lazy::new(|| {
    env::var("JWT_SECRET")
        .expect("JWT_SECRET must be set")
        .into_bytes()
});

static ENCODING_KEY: Lazy<EncodingKey> = Lazy::new(|| EncodingKey::from_secret(&JWT_SECRET));
static DECODING_KEY: Lazy<DecodingKey> = Lazy::new(|| DecodingKey::from_secret(&JWT_SECRET));

pub fn create_jwt(user_id: id, username: String) -> String {
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + 24 * 60 * 60;

    let jwt_user = JwtUser {
        id: user_id,
        username,
        exp: expiration,
    };

    encode(&Header::default(), &jwt_user, &ENCODING_KEY).unwrap()
}

pub fn verify_jwt(token: &str) -> Option<JwtUser> {
    decode(token, &DECODING_KEY, &Validation::default())
        .ok()
        .map(|data| data.claims)
}

pub struct AuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = AuthMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddlewareService { service }))
    }
}

pub struct AuthMiddlewareService<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        if let Some(cookie) = req.cookie("session") {
            if let Some(claims) = verify_jwt(cookie.value()) {
                req.extensions_mut().insert(claims);
                let fut = self.service.call(req);
                return Box::pin(async move { fut.await });
            }
        }
        Box::pin(async move { Err(ErrorUnauthorized("Invalid token")) })
    }
}
