use actix_web::HttpMessage;
use actix_web::error::ErrorPayloadTooLarge;
use actix_web::{
    Error, FromRequest, HttpRequest, HttpResponse, Responder, dev::Payload, error::ErrorBadRequest,
    http::header::CONTENT_TYPE, web,
};
use futures_util::StreamExt;
use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;

#[macro_export]
macro_rules! msgpack {
    ($val:expr) => {
        rmp_serde::to_vec(&$val).unwrap()
    };
}

pub struct MsgPack<T>(pub T);

impl<T> MsgPack<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> FromRequest for MsgPack<T>
where
    T: DeserializeOwned + 'static,
{
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        if req.content_type() != "application/msgpack" {
            return Box::pin(async {
                Err(ErrorBadRequest("Content-Type must be application/msgpack"))
            });
        }

        let mut payload = payload.take();

        const MAX_SIZE: usize = 1024 * 1024;

        Box::pin(async move {
            let mut body = web::BytesMut::new();

            while let Some(chunk) = payload.next().await {
                let chunk = chunk?;
                body.extend_from_slice(&chunk);

                if body.len() > MAX_SIZE {
                    return Err(ErrorPayloadTooLarge("Payload too large"));
                }
            }

            let value: T = rmp_serde::from_slice(&body).map_err(ErrorBadRequest)?;

            Ok(MsgPack(value))
        })
    }
}

impl<T> Responder for MsgPack<T>
where
    T: Serialize,
{
    type Body = actix_web::body::BoxBody;

    fn respond_to(self, _req: &HttpRequest) -> HttpResponse {
        match rmp_serde::to_vec(&self.0) {
            Ok(body) => HttpResponse::Ok()
                .insert_header((CONTENT_TYPE, "application/msgpack"))
                .body(body),
            Err(_) => HttpResponse::InternalServerError().finish(),
        }
    }
}

impl<T> Deref for MsgPack<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
