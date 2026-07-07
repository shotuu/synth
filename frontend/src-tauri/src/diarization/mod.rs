/// Speaker diarization (PROJECT_BRIEF.md §7, Phase 4): on-demand
/// "who spoke when" via pyannote segmentation + WeSpeaker embeddings on
/// ONNX Runtime. CPU-only, fully local, models fetched on first use.
pub mod commands;
pub mod models;
pub mod pipeline;
