/// Speaker diarization pipeline (PROJECT_BRIEF.md §7, Phase 4).
///
/// Runs pyannote's segmentation-3.0 model plus a WeSpeaker embedding model
/// through ONNX Runtime (the same runtime the app already ships for
/// Parakeet), then clusters embeddings by cosine similarity. CPU-only by
/// design — the models are small enough for real-time-ish laptop use.
use anyhow::{anyhow, Result};
use pyannote_rs::{EmbeddingExtractor, EmbeddingManager};
use std::path::Path;

use crate::audio::decoder::decode_audio_file;

pub const DIARIZATION_SAMPLE_RATE: u32 = 16000;
/// Cosine-similarity threshold for treating an embedding as a known speaker.
/// pyannote-rs convention: higher = stricter (more speakers detected).
pub const SPEAKER_MATCH_THRESHOLD: f32 = 0.5;
/// Segments shorter than this rarely embed reliably; they inherit a label
/// via get_best_speaker_match instead of minting a new speaker.
const MIN_EMBED_SEGMENT_SECS: f64 = 0.4;

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerSpan {
    pub start: f64,
    pub end: f64,
    /// 1-based speaker index → displayed as "Speaker 1", "Speaker 2", …
    pub speaker: usize,
}

/// Diarize an audio file into speaker-labeled time spans.
///
/// `progress` receives 0–100 as the file is processed.
pub fn diarize_file(
    audio_path: &Path,
    segmentation_model: &Path,
    embedding_model: &Path,
    max_speakers: usize,
    mut progress: impl FnMut(u8),
) -> Result<Vec<SpeakerSpan>> {
    // Decode + resample to 16kHz mono (same path Whisper uses), then to i16
    let decoded = decode_audio_file(audio_path)?;
    let samples_f32 = decoded.to_whisper_format();
    let total_secs = samples_f32.len() as f64 / DIARIZATION_SAMPLE_RATE as f64;
    if total_secs < 1.0 {
        return Err(anyhow!("Audio too short to diarize"));
    }
    let samples: Vec<i16> = samples_f32
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    drop(samples_f32);
    progress(10);

    let segments = pyannote_rs::get_segments(&samples, DIARIZATION_SAMPLE_RATE, segmentation_model)
        .map_err(|e| anyhow!("Segmentation failed: {}", e))?;

    let mut extractor = EmbeddingExtractor::new(embedding_model)
        .map_err(|e| anyhow!("Failed to load embedding model: {}", e))?;
    let mut manager = EmbeddingManager::new(max_speakers);

    let mut spans = Vec::new();
    for segment in segments {
        let segment = segment.map_err(|e| anyhow!("Segmentation error: {}", e))?;

        let embedding: Vec<f32> = match extractor.compute(&segment.samples) {
            Ok(iter) => iter.collect(),
            Err(e) => {
                log::warn!(
                    "Embedding failed for segment {:.1}-{:.1}s: {}; skipping",
                    segment.start,
                    segment.end,
                    e
                );
                continue;
            }
        };

        let speaker = if segment.end - segment.start < MIN_EMBED_SEGMENT_SECS {
            manager.get_best_speaker_match(embedding).ok()
        } else {
            manager
                .search_speaker(embedding.clone(), SPEAKER_MATCH_THRESHOLD)
                .or_else(|| manager.get_best_speaker_match(embedding).ok())
        };

        if let Some(speaker) = speaker {
            spans.push(SpeakerSpan {
                start: segment.start,
                end: segment.end,
                speaker,
            });
        }

        let pct = 10.0 + (segment.end / total_secs) * 85.0;
        progress(pct.clamp(0.0, 95.0) as u8);
    }

    progress(95);
    Ok(spans)
}

/// Number of distinct speakers in a span list.
pub fn distinct_speakers(spans: &[SpeakerSpan]) -> usize {
    let mut ids: Vec<usize> = spans.iter().map(|s| s.speaker).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

/// Map speaker spans onto transcript rows by maximum temporal overlap.
///
/// Input rows are (transcript_id, audio_start_time, audio_end_time); rows
/// without timing information get None (their existing label is kept).
pub fn assign_speaker_labels(
    rows: &[(String, Option<f64>, Option<f64>)],
    spans: &[SpeakerSpan],
) -> Vec<(String, Option<String>)> {
    rows.iter()
        .map(|(id, start, end)| {
            let label = match (start, end) {
                (Some(start), Some(end)) if end > start => {
                    best_speaker_for_range(*start, *end, spans)
                        .map(|speaker| format!("Speaker {}", speaker))
                }
                _ => None,
            };
            (id.clone(), label)
        })
        .collect()
}

fn best_speaker_for_range(start: f64, end: f64, spans: &[SpeakerSpan]) -> Option<usize> {
    let mut overlap_by_speaker: std::collections::HashMap<usize, f64> =
        std::collections::HashMap::new();

    for span in spans {
        let overlap = (span.end.min(end) - span.start.max(start)).max(0.0);
        if overlap > 0.0 {
            *overlap_by_speaker.entry(span.speaker).or_insert(0.0) += overlap;
        }
    }

    overlap_by_speaker
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(speaker, _)| speaker)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: f64, end: f64, speaker: usize) -> SpeakerSpan {
        SpeakerSpan { start, end, speaker }
    }

    #[test]
    fn assigns_by_max_overlap() {
        let spans = vec![span(0.0, 10.0, 1), span(10.0, 20.0, 2)];
        let rows = vec![
            ("t1".into(), Some(1.0), Some(4.0)),   // fully inside speaker 1
            ("t2".into(), Some(8.0), Some(14.0)),  // 2s with 1, 4s with 2
            ("t3".into(), Some(15.0), Some(19.0)), // fully inside speaker 2
        ];
        let out = assign_speaker_labels(&rows, &spans);
        assert_eq!(out[0].1.as_deref(), Some("Speaker 1"));
        assert_eq!(out[1].1.as_deref(), Some("Speaker 2"));
        assert_eq!(out[2].1.as_deref(), Some("Speaker 2"));
    }

    #[test]
    fn rows_without_timing_or_overlap_get_none() {
        let spans = vec![span(0.0, 5.0, 1)];
        let rows = vec![
            ("no-timing".into(), None, None),
            ("no-overlap".into(), Some(20.0), Some(25.0)),
            ("degenerate".into(), Some(3.0), Some(3.0)),
        ];
        let out = assign_speaker_labels(&rows, &spans);
        assert!(out.iter().all(|(_, label)| label.is_none()));
    }

    #[test]
    fn counts_distinct_speakers() {
        let spans = vec![span(0.0, 1.0, 2), span(1.0, 2.0, 1), span(2.0, 3.0, 2)];
        assert_eq!(distinct_speakers(&spans), 2);
    }
}
