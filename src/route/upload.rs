use actix_multipart::Multipart;
use actix_web::{Error, HttpResponse, web};
use futures_util::StreamExt as _;
use sanitize_filename::sanitize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const UPLOAD_PATHS: &[(&str, &str)] = &[
    ("img", "./uploads/images"),
    ("pp", "./uploads/profile_pictures"),
    ("video", "./uploads/videos"),
];

pub async fn upload(
    path_info: web::Path<String>,
    payload: Multipart,
) -> Result<HttpResponse, Error> {
    let path_segment = path_info.into_inner();

    let upload_dir = UPLOAD_PATHS
        .iter()
        .find(|(key, _)| *key == path_segment)
        .map(|(_, dir)| PathBuf::from(dir))
        .ok_or_else(|| {
            log::error!("Geçersiz yükleme yolu segmenti: {}", path_segment);
            actix_web::error::ErrorNotFound("Bu tür için yükleme uç noktası bulunamadı.")
        })?;

    if !upload_dir.exists() {
        fs::create_dir_all(&upload_dir).map_err(|e| {
            log::error!("Dizin oluşturulamadı {:?}: {}", upload_dir, e);
            actix_web::error::ErrorInternalServerError("Yükleme dizini oluşturulamadı.")
        })?;
    }

    save_files(payload, upload_dir).await?;

    Ok(HttpResponse::Ok().body("Yükleme tamamlandı"))
}

async fn save_files(mut payload: Multipart, upload_dir: PathBuf) -> Result<(), Error> {
    while let Some(field_result) = payload.next().await {
        let mut field = field_result.map_err(|e| {
            log::error!("Multipart error: {}", e);
            actix_web::error::ErrorBadRequest("Error processing multipart field.")
        })?;

        let content_disposition = field.content_disposition().ok_or_else(|| {
            log::warn!("Content-Disposition header missing");
            actix_web::error::ErrorBadRequest("Content-Disposition header missing.")
        })?;

        let filename = content_disposition
            .get_filename()
            .map(sanitize)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                log::warn!("Filename missing or invalid in Content-Disposition");
                actix_web::error::ErrorBadRequest("Filename missing or invalid.")
            })?;

        let file_path = upload_dir.join(&filename);

        if file_path.exists() {
            log::warn!("File already exists: {:?}", file_path);
            return Err(actix_web::error::ErrorConflict(format!(
                "Bu isimde dosya zaten mevcut: {}",
                filename
            )));
        }

        let mut file = web::block(move || std::fs::File::create(file_path))
            .await?
            .map_err(|_| actix_web::error::ErrorInternalServerError("Could not create file."))?;

        while let Some(chunk_result) = field.next().await {
            let data = chunk_result.map_err(|e| {
                log::error!("Error reading chunk for {:?}: {}", filename, e);
                actix_web::error::ErrorInternalServerError("Error reading file chunk.")
            })?;
            file = web::block(move || file.write_all(&data).map(|_| file))
                .await?
                .map_err(|e| {
                    log::error!("Failed to write chunk to file {:?}: {}", filename, e);
                    actix_web::error::ErrorInternalServerError("Could not write to file.")
                })?;
        }
    }
    Ok(())
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/upload").route("/{type}", web::post().to(upload)));
}
