use actix_web::{Error, HttpResponse, error, web};
use google_cloud_storage::client::Client;
use google_cloud_storage::sign::{SignedURLMethod, SignedURLOptions};
use rand::distr::Alphanumeric;
use rand::{Rng, rng};
use serde::{Deserialize, Serialize};
use sha256::digest;
use std::time::Duration;

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum StorageType {
    Image,
    Video,
    File,
    Avatar,
    Icon,
}

impl StorageType {
    pub fn bucket_name(&self) -> &'static str {
        match self {
            StorageType::Image => "thiscrow-media-images",
            StorageType::Video => "thiscrow-media-videos",
            StorageType::File => "thiscrow-media-files",
            StorageType::Avatar => "thiscrow-user-avatars",
            StorageType::Icon => "thiscrow-server-icons",
        }
    }
}

#[derive(Deserialize)]
pub struct UploadRequest {
    pub filename: String,
    pub content_type: String,
    pub storage_type: StorageType,
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub original_filename: String,
    pub saved_filename: String,
    pub signed_url: String,
    pub public_url: String,
}

pub async fn get_upload_signature(
    payload: web::Json<UploadRequest>,
    gcs_client: web::Data<Client>,
) -> Result<HttpResponse, Error> {
    let req = payload.into_inner();

    let extension = req
        .filename
        .rsplit('.')
        .next()
        .unwrap_or("bin")
        .to_lowercase();

    let rand_str: String = rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();

    let hash_name = digest(format!(
        "{}{}{}",
        req.filename,
        rand_str,
        chrono::Utc::now()
    ));

    let saved_filename = format!("{}.{}", hash_name, extension);

    let bucket = req.storage_type.bucket_name();

    let opts = SignedURLOptions {
        method: SignedURLMethod::PUT,
        expires: Duration::from_secs(3600),
        content_type: Some(req.content_type.clone()),
        ..Default::default()
    };

    let signed_url = gcs_client
        .signed_url(bucket, &saved_filename, None, None, opts)
        .await
        .map_err(|e| error::ErrorInternalServerError(e.to_string()))?;

    let response = UploadResponse {
        original_filename: req.filename,
        saved_filename: saved_filename.clone(),
        signed_url,
        public_url: format!(
            "https://storage.googleapis.com/{}/{}",
            bucket, saved_filename
        ),
    };

    Ok(HttpResponse::Ok().json(response))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/upload", web::put().to(get_upload_signature));
}
