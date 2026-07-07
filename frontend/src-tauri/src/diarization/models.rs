/// Diarization model management: two small ONNX models downloaded on first
/// use into the app's models directory (mirroring the Whisper model flow).
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

pub struct DiarizationModels {
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
}

/// (filename, url, minimum plausible size in bytes)
const MODELS: &[(&str, &str, u64)] = &[
    (
        "segmentation-3.0.onnx",
        "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx",
        1_000_000,
    ),
    (
        "wespeaker_en_voxceleb_CAM++.onnx",
        "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx",
        10_000_000,
    ),
];

pub fn models_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow!("Failed to resolve app data dir: {}", e))?
        .join("models")
        .join("diarization");
    Ok(dir)
}

fn is_present(path: &Path, min_size: u64) -> bool {
    std::fs::metadata(path).map(|m| m.len() >= min_size).unwrap_or(false)
}

pub fn models_present<R: Runtime>(app: &AppHandle<R>) -> Result<bool> {
    let dir = models_dir(app)?;
    Ok(MODELS
        .iter()
        .all(|(name, _, min)| is_present(&dir.join(name), *min)))
}

/// Ensure both models are on disk, downloading any that are missing.
/// `progress` receives (model_name, downloaded_bytes, total_bytes).
pub async fn ensure_models<R: Runtime>(
    app: &AppHandle<R>,
    mut progress: impl FnMut(&str, u64, Option<u64>),
) -> Result<DiarizationModels> {
    let dir = models_dir(app)?;
    std::fs::create_dir_all(&dir)?;

    for (name, url, min_size) in MODELS {
        let dest = dir.join(name);
        if is_present(&dest, *min_size) {
            continue;
        }

        log::info!("Downloading diarization model '{}' from {}", name, url);
        let response = reqwest::get(*url)
            .await
            .map_err(|e| anyhow!("Failed to download {}: {}", name, e))?
            .error_for_status()
            .map_err(|e| anyhow!("Failed to download {}: {}", name, e))?;

        let total = response.content_length();
        let mut downloaded: u64 = 0;

        // Download to a temp path, rename only on success
        let tmp = dir.join(format!("{}.part", name));
        let mut file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| anyhow!("Failed to create {}: {}", tmp.display(), e))?;

        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| anyhow!("Download error for {}: {}", name, e))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| anyhow!("Write error for {}: {}", name, e))?;
            downloaded += chunk.len() as u64;
            progress(name, downloaded, total);
        }
        file.flush().await.ok();
        drop(file);

        if !is_present(&tmp, *min_size) {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow!(
                "Downloaded {} looks truncated ({} bytes)",
                name,
                downloaded
            ));
        }
        std::fs::rename(&tmp, &dest)
            .map_err(|e| anyhow!("Failed to finalize {}: {}", name, e))?;
        log::info!("Diarization model '{}' ready ({} bytes)", name, downloaded);
    }

    Ok(DiarizationModels {
        segmentation: dir.join(MODELS[0].0),
        embedding: dir.join(MODELS[1].0),
    })
}
