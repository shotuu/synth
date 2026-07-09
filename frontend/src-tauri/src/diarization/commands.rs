/// Tauri commands for on-demand speaker diarization (Phase 4).
use log::{error as log_error, info as log_info};
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::models::{ensure_models, models_present};
use super::pipeline::{assign_speaker_labels, diarize_file, distinct_speakers};
use crate::state::AppState;

/// Meetings with a diarization run currently in flight (double-click guard)
static RUNNING: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// RAII guard removing a meeting_id from RUNNING when dropped. Rust runs
/// `Drop` impls during unwind too, so this clears the guard even if the
/// pipeline panics instead of returning an `Err` — a plain "remove after
/// the match" (the previous approach) skips on panic, permanently wedging
/// that meeting at "Speaker identification is already running for this
/// session" for the rest of the app's lifetime, with no way to retry short
/// of a restart.
struct RunningGuard(String);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        if let Ok(mut running) = RUNNING.lock() {
            running.remove(&self.0);
        }
    }
}

const MAX_SPEAKERS: usize = 8;

#[derive(Clone, Serialize)]
struct DiarizationProgress<'a> {
    meeting_id: &'a str,
    stage: &'a str,
    progress: u8,
}

#[derive(Clone, Serialize)]
struct DiarizationComplete<'a> {
    meeting_id: &'a str,
    speakers: usize,
    segments_labeled: usize,
}

#[derive(Clone, Serialize)]
struct DiarizationError<'a> {
    meeting_id: &'a str,
    message: &'a str,
}

fn emit_progress<R: Runtime>(app: &AppHandle<R>, meeting_id: &str, stage: &str, progress: u8) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress { meeting_id, stage, progress },
    );
}

/// Resolve the audio file for a meeting: note_audio first, then the legacy
/// folder_path scan used by retranscription.
async fn resolve_audio_path(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<PathBuf, String> {
    if let Ok(Some(audio)) =
        crate::database::repositories::note_audio::NoteAudioRepository::get(pool, meeting_id).await
    {
        if let Some(path) = audio.storage_path {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    let folder: Option<String> =
        sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Failed to look up meeting: {}", e))?
            .flatten();

    let folder = folder.ok_or_else(|| {
        "No audio found for this session — the recording may have been deleted".to_string()
    })?;

    crate::audio::retranscription::find_audio_file(std::path::Path::new(&folder))
        .map_err(|e| format!("No audio file found in session folder: {}", e))
}

#[tauri::command]
pub async fn api_diarization_models_present<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    models_present(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_identify_speakers<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<(), String> {
    {
        let mut running = RUNNING.lock().map_err(|e| e.to_string())?;
        if !running.insert(meeting_id.clone()) {
            return Err("Speaker identification is already running for this session".to_string());
        }
    }

    let pool = state.db_manager.pool().clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let _guard = RunningGuard(meeting_id.clone());
        let result = run_identify_speakers(&app_handle, &pool, &meeting_id).await;

        match result {
            Ok((speakers, segments_labeled)) => {
                log_info!(
                    "Diarization complete for {}: {} speakers across {} segments",
                    meeting_id,
                    speakers,
                    segments_labeled
                );
                let _ = app_handle.emit(
                    "diarization-complete",
                    DiarizationComplete {
                        meeting_id: &meeting_id,
                        speakers,
                        segments_labeled,
                    },
                );
            }
            Err(message) => {
                log_error!("Diarization failed for {}: {}", meeting_id, message);
                let _ = app_handle.emit(
                    "diarization-error",
                    DiarizationError { meeting_id: &meeting_id, message: &message },
                );
            }
        }
    });

    Ok(())
}

async fn run_identify_speakers<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Result<(usize, usize), String> {
    let audio_path = resolve_audio_path(pool, meeting_id).await?;

    // 1. Models (download on first use)
    emit_progress(app, meeting_id, "downloading-models", 0);
    let models = {
        let app_for_progress = app.clone();
        let meeting_for_progress = meeting_id.to_string();
        ensure_models(app, move |_name, done, total| {
            let pct = total
                .filter(|t| *t > 0)
                .map(|t| ((done as f64 / t as f64) * 100.0) as u8)
                .unwrap_or(0);
            emit_progress(&app_for_progress, &meeting_for_progress, "downloading-models", pct);
        })
        .await
        .map_err(|e| format!("Model download failed: {}", e))?
    };

    // 2. Heavy pipeline on a blocking thread, streaming progress events
    emit_progress(app, meeting_id, "identifying-speakers", 0);
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let progress_task = {
        let app = app.clone();
        let meeting_id = meeting_id.to_string();
        tauri::async_runtime::spawn(async move {
            while let Some(pct) = progress_rx.recv().await {
                emit_progress(&app, &meeting_id, "identifying-speakers", pct);
            }
        })
    };

    let spans = tokio::task::spawn_blocking(move || {
        diarize_file(
            &audio_path,
            &models.segmentation,
            &models.embedding,
            MAX_SPEAKERS,
            |pct| {
                let _ = progress_tx.send(pct);
            },
        )
    })
    .await
    .map_err(|e| format!("Diarization task failed: {}", e))?
    .map_err(|e| format!("Diarization failed: {}", e))?;
    progress_task.abort();

    if spans.is_empty() {
        // A real, observed failure mode: the bundled speaker-segmentation
        // model can fail to detect any speech in some recordings even
        // though transcription (a separate model) clearly found speech in
        // the same audio — confirmed not caused by length, level, or clipping
        // (tested with length variation and multiple gain-normalization
        // passes, all reproduced 0 segments). This is a real limitation of
        // that model on some audio profiles, not evidence the recording is
        // silent — say so plainly rather than implying user error.
        return Err(
            "Speaker detection couldn't identify distinct voices in this recording. \
             This can happen with some recordings even when transcription worked fine — \
             it's a limitation of the speaker-detection model on certain audio, not a \
             sign the recording is empty. The transcript itself is unaffected."
                .to_string(),
        );
    }

    // 3. Map spans onto transcript rows
    emit_progress(app, meeting_id, "labeling-transcript", 96);
    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT id, audio_start_time, audio_end_time FROM transcripts WHERE meeting_id = ?",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to load transcript rows: {}", e))?;

    let assignments = assign_speaker_labels(&rows, &spans);

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Failed to start transaction: {}", e))?;
    let mut labeled = 0usize;
    for (transcript_id, label) in &assignments {
        if let Some(label) = label {
            sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ?")
                .bind(label)
                .bind(transcript_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("Failed to label transcript: {}", e))?;
            labeled += 1;
        }
    }
    tx.commit()
        .await
        .map_err(|e| format!("Failed to commit labels: {}", e))?;

    emit_progress(app, meeting_id, "labeling-transcript", 100);
    Ok((distinct_speakers(&spans), labeled))
}

#[derive(Serialize)]
pub struct SpeakerInfo {
    pub label: String,
    pub segment_count: i64,
}

#[tauri::command]
pub async fn api_get_speakers(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<SpeakerInfo>, String> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT speaker, COUNT(*) FROM transcripts
         WHERE meeting_id = ? AND speaker IS NOT NULL AND speaker NOT IN ('mic','system')
         GROUP BY speaker ORDER BY COUNT(*) DESC",
    )
    .bind(&meeting_id)
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| format!("Failed to list speakers: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(label, segment_count)| SpeakerInfo { label, segment_count })
        .collect())
}

#[tauri::command]
pub async fn api_rename_speaker(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    from_label: String,
    to_label: String,
) -> Result<u64, String> {
    let to_label = to_label.trim();
    if to_label.is_empty() {
        return Err("Speaker name cannot be empty".to_string());
    }

    let result = sqlx::query("UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?")
        .bind(to_label)
        .bind(&meeting_id)
        .bind(&from_label)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| format!("Failed to rename speaker: {}", e))?;

    log_info!(
        "Renamed speaker '{}' → '{}' on {} ({} segments)",
        from_label,
        to_label,
        meeting_id,
        result.rows_affected()
    );
    Ok(result.rows_affected())
}
