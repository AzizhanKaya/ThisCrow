use actix_multipart::Multipart;
use actix_web::{Error, HttpResponse, error, web};
use futures_util::StreamExt as _;
use rand::distr::Alphanumeric;
use rand::{Rng, rng};
use serde::Serialize;
use sha256::digest;
use std::{fs, io::Write, path::PathBuf};

const UPLOAD_DIRS: &[(&str, &str)] = &[
    ("img", "./uploads/images"),
    ("pp", "./uploads/profile_pictures"),
    ("video", "./uploads/videos"),
    ("file", "./uploads/files"),
];

pub fn init() {
    for (_, path) in UPLOAD_DIRS {
        fs::create_dir_all(path).expect("Could not create the upload directories");
    }
}

#[derive(Serialize)]
struct UploadInfo {
    filename: String,
    saved_filename: String,
    r#type: String,
}

async fn save_files(mut payload: Multipart) -> Result<Vec<UploadInfo>, Error> {
    let mut saved_files = Vec::new();

    while let Some(item) = payload.next().await {
        let mut field = item?;

        let Some(content_disposition) = field.content_disposition() else {
            continue;
        };

        let Some(field_name) = field.name() else {
            continue;
        };

        let Some(filename) = content_disposition.get_filename().map(|s| s.to_string()) else {
            continue;
        };

        if filename.trim().is_empty() {
            continue;
        }

        let extension = filename.rsplit('.').next().unwrap_or("").to_lowercase();

        let dir_key = match field_name {
            "pp" => "pp",
            "img" => "img",
            "video" => "video",
            _ => "file",
        };

        let dir_path = UPLOAD_DIRS
            .iter()
            .find(|(key, _)| *key == dir_key)
            .map(|(_, path)| PathBuf::from(path))
            .ok_or_else(|| error::ErrorBadRequest("Invalid file type"))?;

        let rand: String = rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();

        let hex = digest(format!("{}{}", filename, rand));

        let saved_filename = format!("{}.{}", hex, extension);
        let filepath = dir_path.join(&saved_filename);

        let mut file = fs::File::create(filepath)?;

        while let Some(chunk) = field.next().await {
            let data = chunk?;
            file.write_all(&data)?;
        }

        saved_files.push(UploadInfo {
            filename,
            saved_filename,
            r#type: dir_key.to_string(),
        });
    }

    Ok(saved_files)
}

async fn upload(payload: Multipart) -> Result<HttpResponse, Error> {
    let saved_files = save_files(payload).await?;

    Ok(HttpResponse::Ok().json(saved_files))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/upload", web::post().to(upload));
}
