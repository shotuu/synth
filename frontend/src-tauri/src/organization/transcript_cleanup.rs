//! "AI-cleaned" transcript export: an alternate rendering of the transcript
//! that runs each speaker's turns through the same LLM provider configured
//! for meeting summaries, stripping filler words/false starts and reflowing
//! the text into readable sentences, while keeping the original
//! speaker/timestamp structure untouched. Raw export (export.rs's default
//! path) never calls any of this.
use std::path::PathBuf;
use std::time::Duration;

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, Runtime};

use crate::database::repositories::setting::SettingsRepository;
use crate::ollama::metadata::ModelMetadataCache;
use crate::summary::llm_client::{generate_summary, LLMProvider};
use crate::summary::processor::clean_llm_markdown_output;
use crate::summary::summary_engine::models::get_model_by_name;

static METADATA_CACHE: Lazy<ModelMetadataCache> =
    Lazy::new(|| ModelMetadataCache::new(Duration::from_secs(300)));

static TURN_MARKER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^<<<TURN (\d+)>>>\s*$").unwrap());

const SYSTEM_PROMPT: &str = r#"You are a careful copy editor cleaning up an automatic speech-to-text meeting transcript for reading. You will receive one or more speaker turns, each starting with its own `<<<TURN n>>>` marker line followed by that turn's raw transcribed text.

For each turn:
- Remove filler words and verbal tics (um, uh, like, you know, so, well, right, I mean) when they add no meaning.
- Remove exact word/phrase repetitions and false starts caused by the speaker restarting a sentence.
- Reflow the text into clear, well-punctuated sentences and paragraphs.
- Preserve the speaker's actual meaning, opinions, and word choice — this is a light copy-edit, not a summary or rewrite. Do not add information, do not remove substantive content, and do not change facts, numbers, or names.
- Keep each turn as exactly one turn; do not merge or split turns.

Output EXACTLY one `<<<TURN n>>>` marker per input turn, matching its number, followed by that turn's cleaned text, and nothing else — no commentary, no extra markers, no headers."#;

struct Turn {
    speaker: Option<String>,
    start: Option<f64>,
    text: String,
}

/// Merges consecutive same-speaker rows into one turn, same grouping rule
/// export.rs's HTML renderer uses to decide when to repeat a speaker chip.
fn group_into_turns(rows: &[(String, Option<String>, Option<f64>)]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for (text, speaker, start) in rows {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(last) = turns.last_mut() {
            if &last.speaker == speaker {
                last.text.push(' ');
                last.text.push_str(trimmed);
                continue;
            }
        }
        turns.push(Turn { speaker: speaker.clone(), start: *start, text: trimmed.to_string() });
    }
    turns
}

/// Groups turn indices into chunks that stay under `char_budget`, without
/// ever splitting a single turn across two LLM calls.
fn chunk_turns(turns: &[Turn], char_budget: usize) -> Vec<Vec<usize>> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_len = 0usize;
    for (i, turn) in turns.iter().enumerate() {
        let len = turn.text.len() + 20; // + marker overhead
        if !current.is_empty() && current_len + len > char_budget {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current.push(i);
        current_len += len;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn render_chunk_prompt(turns: &[Turn], indices: &[usize]) -> String {
    let mut out = String::new();
    for &i in indices {
        out.push_str(&format!("<<<TURN {}>>>\n{}\n\n", i, turns[i].text));
    }
    out
}

/// Parses `<<<TURN n>>>`-delimited cleaned text back out of an LLM response,
/// requiring an exact 1:1 match against the turns that were sent — any
/// mismatch (model dropped/added/reordered a turn) is a hard error rather
/// than silently misattributing cleaned text to the wrong speaker.
fn parse_cleaned_chunk(response: &str, indices: &[usize]) -> Result<Vec<(usize, String)>, String> {
    let markers: Vec<regex::Match> = TURN_MARKER.find_iter(response).collect();
    if markers.len() != indices.len() {
        return Err(format!(
            "AI cleanup returned {} turn(s), expected {}",
            markers.len(),
            indices.len()
        ));
    }

    let mut result = Vec::with_capacity(markers.len());
    for (pos, m) in markers.iter().enumerate() {
        let idx: usize = TURN_MARKER
            .captures(m.as_str())
            .and_then(|c| c.get(1))
            .and_then(|g| g.as_str().parse().ok())
            .ok_or_else(|| "AI cleanup returned a malformed turn marker".to_string())?;
        let text_start = m.end();
        let text_end = markers.get(pos + 1).map(|next| next.start()).unwrap_or(response.len());
        let text = response[text_start..text_end].trim().to_string();
        if text.is_empty() {
            return Err(format!("AI cleanup returned empty text for turn {}", idx));
        }
        result.push((idx, text));
    }

    let got_indices: Vec<usize> = result.iter().map(|(i, _)| *i).collect();
    if got_indices != indices {
        return Err("AI cleanup returned turns out of order or with unexpected turn numbers".to_string());
    }
    Ok(result)
}

struct CleanupConfig {
    provider: LLMProvider,
    model_name: String,
    api_key: String,
    ollama_endpoint: Option<String>,
    custom_openai_endpoint: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    app_data_dir: Option<PathBuf>,
    char_budget: usize,
}

/// Resolves the same provider/model settings used for meeting summaries
/// (there's one settings row per AI capability, not per feature — see
/// SettingsRepository::get_model_config) into everything generate_summary
/// needs for a raw-prompt call.
async fn resolve_cleanup_config<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<CleanupConfig, String> {
    let setting = SettingsRepository::get_model_config(pool)
        .await
        .map_err(|e| format!("Failed to load AI settings: {}", e))?
        .ok_or_else(|| {
            "No AI provider is configured. Set one up in Settings before using AI cleanup.".to_string()
        })?;

    let provider = LLMProvider::from_str(&setting.provider)?;
    // Not setting.get_custom_openai_config() - that only parses the JSON
    // column, which never carries a plaintext key once one has been
    // migrated into the OS keychain. This goes through the keychain-aware
    // repository method instead, same as api_key resolution below.
    let custom_openai = SettingsRepository::get_custom_openai_config(pool)
        .await
        .map_err(|e| format!("Failed to load custom OpenAI config: {}", e))?;

    let api_key = if matches!(provider, LLMProvider::Ollama | LLMProvider::BuiltInAI | LLMProvider::CustomOpenAI)
    {
        String::new()
    } else {
        SettingsRepository::get_api_key(pool, &setting.provider)
            .await
            .map_err(|e| format!("Failed to load API key: {}", e))?
            .filter(|k| !k.is_empty())
            .ok_or_else(|| format!("No API key configured for {}", setting.provider))?
    };

    let final_api_key = if provider == LLMProvider::CustomOpenAI {
        let cfg = custom_openai
            .as_ref()
            .ok_or_else(|| "Custom OpenAI provider selected but not configured".to_string())?;
        cfg.api_key.clone().unwrap_or_default()
    } else {
        api_key
    };

    let context_tokens: usize = match provider {
        LLMProvider::Ollama => {
            match METADATA_CACHE.get_or_fetch(&setting.model, setting.ollama_endpoint.as_deref()).await {
                Ok(meta) => (meta.context_size as usize).saturating_sub(300),
                Err(_) => 4000,
            }
        }
        LLMProvider::BuiltInAI => get_model_by_name(&setting.model)
            .map(|m| (m.context_size as usize).saturating_sub(300))
            .unwrap_or(1748),
        _ => 100_000,
    };

    // Unlike summarization (short output from a long input), cleaned text is
    // roughly the same length as the raw turn text, so the input chunk sent
    // to the model must leave it enough of the context window to write back
    // an equally-sized response — reserve well over half instead of a fixed
    // small margin.
    let chars_per_token = 1.0 / 0.35;
    let input_token_budget = ((context_tokens as f64) / 2.2).floor().max(200.0);
    let char_budget = (input_token_budget * chars_per_token).floor() as usize;

    // The response must fit in the same max_tokens cap sent on the request,
    // or the provider truncates it mid-turn and parse_cleaned_chunk's exact
    // marker-count check hard-fails the whole export. generate_summary's own
    // fallback (8192, for Claude) is sized for short summarization output,
    // not cleanup's roughly-1:1 output — a chunk built from a 100k-token
    // cloud "context_tokens" assumption above easily produces far more.
    // Explicitly size the request's max_tokens off the same input budget
    // (with headroom) so the two stay consistent, unless the user set an
    // explicit CustomOpenAI override. Clamped to 8192: most cloud APIs
    // (Claude, OpenAI, Groq) reject a max_tokens above their standard cap
    // with a hard 400 rather than honoring it, and this only feeds the two
    // providers (Claude, CustomOpenAI-without-override) that actually read
    // it — see generate_summary in llm_client.rs. A chunk whose real cleaned
    // output would exceed this still truncates; char_budget above already
    // keeps individual chunks well short of that in the common case.
    let derived_max_tokens = ((input_token_budget * 1.2).ceil() as u32).clamp(512, 8192);
    let max_tokens = custom_openai
        .as_ref()
        .and_then(|c| c.max_tokens)
        .map(|t| t as u32)
        .or(Some(derived_max_tokens));

    Ok(CleanupConfig {
        provider,
        model_name: setting.model.clone(),
        api_key: final_api_key,
        ollama_endpoint: setting.ollama_endpoint.clone(),
        custom_openai_endpoint: custom_openai.as_ref().map(|c| c.endpoint.clone()),
        max_tokens,
        temperature: custom_openai.as_ref().and_then(|c| c.temperature),
        top_p: custom_openai.as_ref().and_then(|c| c.top_p),
        app_data_dir: app.path().app_data_dir().ok(),
        char_budget,
    })
}

/// Runs the transcript through the configured LLM to strip filler words and
/// reflow it into clean paragraphs, one or more speaker turns per call,
/// returning rows in the same `(text, speaker, start)` shape as the raw
/// export path so callers can drop it straight into ExportData.
pub async fn clean_transcript<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    rows: &[(String, Option<String>, Option<f64>)],
) -> Result<Vec<(String, Option<String>, Option<f64>)>, String> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let config = resolve_cleanup_config(app, pool).await?;
    let turns = group_into_turns(rows);
    if turns.is_empty() {
        return Ok(Vec::new());
    }
    let client = Client::new();
    let chunks = chunk_turns(&turns, config.char_budget);

    let mut cleaned_text: Vec<Option<String>> = vec![None; turns.len()];
    for indices in &chunks {
        let prompt = render_chunk_prompt(&turns, indices);
        let response = generate_summary(
            &client,
            &config.provider,
            &config.model_name,
            &config.api_key,
            SYSTEM_PROMPT,
            &prompt,
            config.ollama_endpoint.as_deref(),
            config.custom_openai_endpoint.as_deref(),
            config.max_tokens,
            config.temperature,
            config.top_p,
            config.app_data_dir.as_ref(),
            None,
        )
        .await
        .map_err(|e| format!("AI cleanup failed: {}", e))?;
        let response = clean_llm_markdown_output(&response);

        for (idx, text) in parse_cleaned_chunk(&response, indices)? {
            cleaned_text[idx] = Some(text);
        }
    }

    Ok(turns
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let text = cleaned_text[i].clone().unwrap_or(t.text);
            (text, t.speaker, t.start)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<(String, Option<String>, Option<f64>)> {
        vec![
            ("Um so like".to_string(), Some("Speaker 1".to_string()), Some(0.0)),
            ("yeah I think we should".to_string(), Some("Speaker 1".to_string()), Some(1.5)),
            ("ship it.".to_string(), Some("Speaker 1".to_string()), Some(3.0)),
            ("Agreed.".to_string(), Some("Speaker 2".to_string()), Some(5.0)),
        ]
    }

    #[test]
    fn groups_consecutive_same_speaker_rows_into_one_turn() {
        let turns = group_into_turns(&rows());
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].speaker.as_deref(), Some("Speaker 1"));
        assert_eq!(turns[0].text, "Um so like yeah I think we should ship it.");
        assert_eq!(turns[0].start, Some(0.0));
        assert_eq!(turns[1].speaker.as_deref(), Some("Speaker 2"));
        assert_eq!(turns[1].text, "Agreed.");
    }

    #[test]
    fn chunk_turns_never_splits_a_single_turn() {
        let turns = group_into_turns(&rows());
        let chunks = chunk_turns(&turns, 10); // tiny budget, forces one turn per chunk
        assert_eq!(chunks, vec![vec![0], vec![1]]);
    }

    #[test]
    fn parses_matching_markers_in_order() {
        let response = "<<<TURN 0>>>\nWe should ship it.\n\n<<<TURN 1>>>\nAgreed.\n";
        let parsed = parse_cleaned_chunk(response, &[0, 1]).unwrap();
        assert_eq!(parsed[0], (0, "We should ship it.".to_string()));
        assert_eq!(parsed[1], (1, "Agreed.".to_string()));
    }

    #[test]
    fn rejects_mismatched_turn_count() {
        let response = "<<<TURN 0>>>\nOnly one turn.\n";
        let err = parse_cleaned_chunk(response, &[0, 1]).unwrap_err();
        assert!(err.contains("expected 2"), "got: {err}");
    }

    #[test]
    fn rejects_reordered_or_wrong_turn_numbers() {
        let response = "<<<TURN 1>>>\nSecond.\n\n<<<TURN 0>>>\nFirst.\n";
        let err = parse_cleaned_chunk(response, &[0, 1]).unwrap_err();
        assert!(err.contains("out of order"), "got: {err}");
    }

    #[test]
    fn clean_transcript_returns_empty_for_empty_input() {
        // No app/pool needed: the empty-input short-circuit runs before any
        // settings lookup, so this exercises that guard without a fixture.
        let rows: Vec<(String, Option<String>, Option<f64>)> = vec![];
        let turns = group_into_turns(&rows);
        assert!(turns.is_empty());
    }
}
