//! A real click-through pass, done at the backend layer since a browser
//! preview can't drive Tauri's invoke() bridge and no OS-level UI automation
//! is available in this environment (window.__TAURI_INTERNALS__ only exists
//! inside the native webview).
//!
//! This chains one continuous session through every phase's real business
//! logic in the order a user would actually hit it -- not isolated unit
//! tests, but one narrative: create a folder, record a lecture, attach a
//! slide deck, write notes, get auto-classified, generate a REAL summary
//! via local Ollama, identify speakers on real synthesized audio, organize
//! and tag it, export to all three formats, then run it through Storage
//! Manager. Every assertion checks real state (DB rows, real files, actual
//! LLM output, actual PDF/DOCX bytes) rather than "didn't crash."
//!
//! Ignored by default -- needs Ollama running locally with llama3.1, plus
//! macOS `say` and ffmpeg for the diarization leg (same deps as the
//! existing per-phase real-audio tests). Run explicitly:
//!   cargo test --test full_session_e2e -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::process::Command;

use app_lib::database::repositories::attachment::AttachmentsRepository;
use app_lib::database::repositories::folder::FoldersRepository;
use app_lib::database::repositories::meeting_notes::MeetingNotesRepository;
use app_lib::database::repositories::note_audio::NoteAudioRepository;
use app_lib::diarization::pipeline::{assign_speaker_labels, diarize_file};
use app_lib::organization::action_item_extraction::extract_action_items;
use app_lib::organization::action_items::{list_action_items, replace_undone_action_items, ActionItemFilter};
use app_lib::organization::export::fetch_export_data;
use app_lib::organization::export_docx::render_docx;
use app_lib::organization::export_pdf::render_pdf;
use app_lib::organization::storage::{compute_stats, list_session_audio};
use app_lib::sources::assembler::{assemble_context, augment_transcript_text};
use app_lib::sources::classify::suggest_context_type;
use app_lib::sources::extraction::{detect_file_type, extract_text};
use app_lib::summary::llm_client::LLMProvider;
use app_lib::summary::processor::generate_meeting_summary;
use app_lib::summary::templates;
use sqlx::SqlitePool;

async fn fresh_pool() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

/// Mirrors what lib.rs's Tauri setup hook does at real app startup
/// (`resource_dir().join("templates")`) -- outside a running Tauri app
/// there's no resource dir to resolve, so point at the same source files
/// directly. Without this, get_template() only sees the two built-ins
/// compiled into the binary and none of the bundled JSON templates
/// (lecture/discussion/coffee_chat/etc.), which is exactly the gap this
/// click-through test caught on its first run.
fn wire_bundled_templates_dir() {
    templates::set_bundled_templates_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates"));
}

fn tmpdir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth-e2e-{}-{}", label, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
#[ignore = "needs Ollama (llama3.1) running locally; macOS say + ffmpeg for the diarization leg"]
async fn full_session_click_through() {
    wire_bundled_templates_dir();
    let pool = fresh_pool().await;
    let tmp = tmpdir("main");

    // ---- 1. Organize: create a course folder, like a student would ----
    let folder = FoldersRepository::create(&pool, None, "CS33 - Distributed Systems", Some("📚"))
        .await
        .expect("folder creation");

    // ---- 2. Record: a lecture gets transcribed (simulate transcription
    // completing by writing segments directly, same as the real pipeline
    // does after Whisper finishes) ----
    let meeting_id = "meeting-e2e-lecture".to_string();
    let now = chrono::Utc::now();
    sqlx::query(
        "INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&meeting_id)
    .bind("Lecture 12: Consensus Algorithms")
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("meeting insert");

    let transcript_segments = [
        (0.0, "Good morning everyone, welcome to lecture twelve. Today we're covering consensus algorithms, specifically Raft."),
        (12.0, "This will absolutely be on the midterm, so make sure you understand leader election."),
        (30.0, "Remember the homework problem set six is due Friday, covering the material from today."),
        (45.0, "Let's work through an example. Suppose we have five nodes and node one becomes a candidate."),
        (70.0, "Any questions before we move to the next topic? Office hours are Tuesday if you need more help."),
    ];
    for (i, (start, text)) in transcript_segments.iter().enumerate() {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("t{}", i))
        .bind(&meeting_id)
        .bind(text)
        .bind(now.to_rfc3339())
        .bind(*start)
        .bind(*start + 10.0)
        .execute(&pool)
        .await
        .unwrap();
    }

    // ---- 3. Auto-classification runs on this transcript, same as the
    // real recording-save path does ----
    let full_text: String = transcript_segments.iter().map(|(_, t)| *t).collect::<Vec<_>>().join(" ");
    let suggested = suggest_context_type(&full_text);
    assert_eq!(suggested, Some("lecture"), "a lecture transcript should auto-classify as lecture");
    sqlx::query("UPDATE meetings SET context_type = ? WHERE id = ?")
        .bind(suggested.unwrap())
        .bind(&meeting_id)
        .execute(&pool)
        .await
        .unwrap();

    // ---- 4. Multi-source: attach a real PDF (slides), extract its text ----
    let slides_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixture.pdf");
    let slides_dest = tmp.join("slides.pdf");
    std::fs::copy(&slides_src, &slides_dest).unwrap();
    let file_type = detect_file_type(&slides_dest);
    let extracted = extract_text(&slides_dest, &file_type);
    assert!(extracted.is_some(), "PDF text extraction must actually produce text, not just not-crash");
    AttachmentsRepository::insert(
        &pool,
        &meeting_id,
        "slides.pdf",
        &file_type,
        &slides_dest.to_string_lossy(),
        extracted.as_deref(),
    )
    .await
    .unwrap();

    // ---- 5. Multi-source: the student's own written notes ----
    MeetingNotesRepository::upsert(
        &pool,
        &meeting_id,
        Some("Ask about the difference between Raft and Paxos next class"),
        Some(r#"[{"type":"paragraph"}]"#),
    )
    .await
    .unwrap();

    // ---- 6. Organize: file it into the course folder, tag it ----
    sqlx::query("UPDATE meetings SET folder_id = ?, tags = ? WHERE id = ?")
        .bind(&folder.id)
        .bind(r#"["exam-relevant","raft"]"#)
        .bind(&meeting_id)
        .execute(&pool)
        .await
        .unwrap();

    // ---- 7. Assemble every source and generate a REAL summary against
    // local Ollama -- not a stub, an actual model call ----
    let ctx = assemble_context(&pool, &meeting_id).await.unwrap();
    assert_eq!(ctx.sources_used, vec!["transcript", "attachments", "user_notes"]);
    let (augmented_text, sources_used) = augment_transcript_text(&full_text, &ctx);
    assert_eq!(sources_used.len(), 3, "summary input should draw on all three source types");

    // The real api_process_transcript command writes a transcript_chunks row
    // before generating a summary; fetch_export_data's summary lookup joins
    // against it (SummaryProcessesRepository::get_summary_data_for_meeting),
    // so this step must be mirrored here too or the export step later won't
    // see the summary we're about to generate.
    sqlx::query(
        "INSERT INTO transcript_chunks (meeting_id, meeting_name, transcript_text, model, model_name, chunk_size, overlap, created_at)
         VALUES (?, ?, ?, 'ollama', 'llama3.1:latest', 4000, 100, ?)",
    )
    .bind(&meeting_id)
    .bind("Lecture 12: Consensus Algorithms")
    .bind(&full_text)
    .bind(now.to_rfc3339())
    .execute(&pool)
    .await
    .unwrap();

    let template = templates::get_template("lecture").expect("lecture template must exist");
    let client = reqwest::Client::new();
    let (final_markdown, _english_markdown, num_chunks) = generate_meeting_summary(
        &client,
        &LLMProvider::Ollama,
        "llama3.1:latest",
        "",
        &augmented_text,
        "",
        "lecture",
        &template,
        4000,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("Ollama summary generation should succeed -- is `ollama serve` running with llama3.1 pulled?");

    assert!(num_chunks >= 1);
    assert!(!final_markdown.trim().is_empty(), "a real LLM call must produce real, non-empty output");
    eprintln!("--- Generated summary ---\n{}\n---", final_markdown);

    let result_json = serde_json::json!({
        "markdown": final_markdown,
        "sources_used": sources_used,
    });
    sqlx::query(
        "INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, result)
         VALUES (?, 'completed', ?, ?, ?)",
    )
    .bind(&meeting_id)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(result_json.to_string())
    .execute(&pool)
    .await
    .unwrap();

    // ---- 8. Cross-note action items: extract from the REAL generated
    // summary (the transcript explicitly mentions "problem set six is due
    // Friday", and the lecture template's Homework & Assignments section
    // instructs the model to capture exactly this), same call the real
    // summary-completion path now makes. ----
    let extracted = extract_action_items(&final_markdown);
    eprintln!("--- Extracted action items ---\n{:#?}\n---", extracted);
    assert!(
        !extracted.is_empty(),
        "expected the real model to surface the homework mentioned in the transcript \
         (\"problem set six is due Friday\") under the lecture template's Homework & \
         Assignments section; got no extractable items from:\n{}",
        final_markdown
    );

    let saved_count = replace_undone_action_items(&pool, &meeting_id, &extracted).await.unwrap();
    assert_eq!(saved_count, extracted.len());

    let items_after = list_action_items(
        &pool,
        &ActionItemFilter { folder_id: Some(folder.id.clone()), ..Default::default() },
    )
    .await
    .unwrap();
    assert_eq!(items_after.len(), extracted.len(), "cross-note view must reflect exactly what was extracted");
    assert_eq!(items_after[0].meeting_title, "Lecture 12: Consensus Algorithms");
    assert_eq!(items_after[0].context_type, "lecture");

    // ---- 9. Speaker diarization on a REAL synthesized two-voice
    // recording (professor + student asking a question) ----
    let (seg_model, emb_model) = ensure_diarization_models();
    let audio_path = synthesize_lecture_audio(&tmp);

    NoteAudioRepository::upsert_from_file(&pool, &meeting_id, &audio_path, "recorded", None)
        .await
        .unwrap();

    let spans = diarize_file(&audio_path, &seg_model, &emb_model, 8, |_p| {}).expect("diarization must succeed");
    assert!(!spans.is_empty(), "should detect speech in a real two-voice recording");

    let rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT id, audio_start_time, audio_end_time FROM transcripts WHERE meeting_id = ?",
    )
    .bind(&meeting_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let assignments = assign_speaker_labels(&rows, &spans);
    let labeled_count = assignments.iter().filter(|(_, label)| label.is_some()).count();
    assert!(labeled_count > 0, "at least some transcript rows should get a speaker label");
    for (id, label) in &assignments {
        if let Some(label) = label {
            sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ?")
                .bind(label)
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }
    }

    // ---- 10. Export to all three formats, with real content assertions ----
    let (title, context_type, created_at): (String, String, String) =
        sqlx::query_as("SELECT title, context_type, created_at FROM meetings WHERE id = ?")
            .bind(&meeting_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(context_type, "lecture");

    let export_data = fetch_export_data(&pool, &meeting_id, &title, &context_type, &created_at)
        .await
        .unwrap();
    assert_eq!(export_data.attachments.len(), 1);
    assert!(export_data.summary_markdown.is_some());

    let pdf_path = tmp.join("export.pdf");
    render_pdf(&export_data, &pdf_path).expect("PDF export of a real generated summary");
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();
    assert!(pdf_bytes.starts_with(b"%PDF-"));
    assert!(pdf_bytes.len() > 2000, "a real multi-section export should be a substantial PDF");

    let docx_path = tmp.join("export.docx");
    render_docx(&export_data, &docx_path).expect("DOCX export of a real generated summary");
    let docx_file = std::fs::File::open(&docx_path).unwrap();
    let mut archive = zip::ZipArchive::new(docx_file).unwrap();
    use std::io::Read;
    let mut doc_xml = String::new();
    archive.by_name("word/document.xml").unwrap().read_to_string(&mut doc_xml).unwrap();
    assert!(doc_xml.contains("Consensus") || doc_xml.contains("Raft"), "DOCX must contain real session content, not placeholder text");

    // ---- 11. Storage Manager: verify it sees the audio we just recorded ----
    let sessions = list_session_audio(&pool).await.unwrap();
    let this_session = sessions.iter().find(|s| s.meeting_id == meeting_id).expect("session should appear in storage list");
    assert_eq!(this_session.context_type, "lecture");
    assert!(this_session.current_size_bytes.unwrap_or(0) > 0);

    let stats = compute_stats(&pool, &tmp).await.unwrap();
    assert_eq!(stats.session_count, 1);
    assert_eq!(stats.sessions_with_retained_audio, 1);

    eprintln!("\n=== Full session click-through passed ===");
    eprintln!("Folder: {} | Session: {} ({})", folder.name, title, context_type);
    eprintln!("Sources used: {:?}", sources_used);
    eprintln!("Summary: {} chars across {} chunk(s)", final_markdown.len(), num_chunks);
    eprintln!("Speakers labeled: {}/{} transcript rows", labeled_count, rows.len());
    eprintln!("Action items extracted: {}", items_after.len());
    eprintln!("Exports: PDF {} bytes, DOCX valid zip with real content", pdf_bytes.len());

    std::fs::remove_dir_all(&tmp).ok();
}

const MODELS: &[(&str, &str)] = &[
    ("segmentation-3.0.onnx", "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx"),
    ("wespeaker_en_voxceleb_CAM++.onnx", "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx"),
];

fn ensure_diarization_models() -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/diarization-test-models");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, url) in MODELS {
        let dest = dir.join(name);
        if dest.exists() && std::fs::metadata(&dest).unwrap().len() > 1_000_000 {
            continue;
        }
        let status = Command::new("curl").args(["-L", "--fail", "-o"]).arg(&dest).arg(*url).status().unwrap();
        assert!(status.success(), "failed to download {}", name);
    }
    (dir.join(MODELS[0].0), dir.join(MODELS[1].0))
}

/// A professor lecturing, then a student asking a question -- two distinct
/// macOS voices, concatenated with brief silence, resampled to 16kHz mono.
fn synthesize_lecture_audio(dir: &Path) -> PathBuf {
    let turns: &[(&str, &str)] = &[
        ("Daniel", "Today we're covering consensus algorithms, specifically the Raft protocol, and this will be on your midterm exam."),
        ("Samantha", "Sorry, quick question — how is Raft different from Paxos in terms of leader election?"),
    ];

    let mut converted = Vec::new();
    for (i, (voice, text)) in turns.iter().enumerate() {
        let aiff = dir.join(format!("turn{}.aiff", i));
        let status = Command::new("say").args(["-v", voice, "-o"]).arg(&aiff).arg(text).status().unwrap();
        assert!(status.success(), "`say` must be available (macOS only)");
        let wav = dir.join(format!("turn{}.wav", i));
        Command::new("ffmpeg").args(["-y", "-i"]).arg(&aiff).args(["-ar", "16000", "-ac", "1"]).arg(&wav).status().unwrap();
        converted.push(wav);
    }

    let silence = dir.join("silence.wav");
    Command::new("ffmpeg").args(["-y", "-f", "lavfi", "-i", "anullsrc=r=16000:cl=mono", "-t", "0.6"]).arg(&silence).status().unwrap();

    let list = dir.join("concat.txt");
    let mut content = String::new();
    for wav in &converted {
        content.push_str(&format!("file '{}'\n", wav.display()));
        content.push_str(&format!("file '{}'\n", silence.display()));
    }
    std::fs::write(&list, content).unwrap();

    let output = dir.join("lecture_audio.wav");
    let status = Command::new("ffmpeg")
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy"])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    output
}
