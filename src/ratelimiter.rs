use crate::middleware::JwtUser;
use actix_governor::KeyExtractor;
use actix_web::{HttpMessage, ResponseError, dev::ServiceRequest};

#[derive(Debug)]
pub struct ExtractionError {
    message: &'static str,
}

impl std::fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "User extraction failed: {}", self.message)
    }
}

impl ResponseError for ExtractionError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::BAD_REQUEST
    }
}

#[derive(Clone)]
pub struct UserKeyExtractor;

impl KeyExtractor for UserKeyExtractor {
    type Key = String;
    type KeyExtractionError = ExtractionError;

    fn extract(&self, req: &ServiceRequest) -> Result<Self::Key, Self::KeyExtractionError> {
        if let Some(user) = req.extensions().get::<JwtUser>() {
            return Ok(user.id.to_string());
        }

        if let Some(ip) = req
            .headers()
            .get("cf-connecting-ip")
            .and_then(|h| h.to_str().ok())
            .map(|ip| ip.trim().to_string())
        {
            return Ok(ip);
        }

        req.peer_addr()
            .map(|addr| addr.ip().to_string())
            .ok_or(ExtractionError {
                message: "Peer address not found",
            })
    }
}
