//! End-to-end test of the Phase 2 multi-source session backend:
//! migrations → attach files (with real text extraction) → save notes →
//! record audio metadata → assemble the combined session context.

use std::io::Write;
use std::path::{Path, PathBuf};

use app_lib::database::repositories::attachment::AttachmentsRepository;
use app_lib::database::repositories::meeting_notes::MeetingNotesRepository;
use app_lib::database::repositories::note_audio::NoteAudioRepository;
use app_lib::sources::assembler::assemble_context;
use app_lib::sources::extraction::{detect_file_type, extract_text};
use sqlx::SqlitePool;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn insert_meeting(pool: &SqlitePool, id: &str) {
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'Test session', '2026-07-07T10:00:00Z', '2026-07-07T10:00:00Z')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

fn write_docx(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = Default::default();
    z.start_file("[Content_Types].xml", opts).unwrap();
    z.write_all(b"<Types/>").unwrap();
    z.start_file("word/document.xml", opts).unwrap();
    z.write_all(
        br#"<w:document><w:body>
            <w:p><w:r><w:t>Lecture handout: dynamic programming.</w:t></w:r></w:p>
            <w:p><w:r><w:t>Homework due Friday.</w:t></w:r></w:p>
        </w:body></w:document>"#,
    )
    .unwrap();
    z.finish().unwrap();
}

fn write_pptx(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = Default::default();
    z.start_file("[Content_Types].xml", opts).unwrap();
    z.write_all(b"<Types/>").unwrap();
    z.start_file("ppt/slides/slide1.xml", opts).unwrap();
    z.write_all(br#"<p:sld><a:p><a:r><a:t>Slide one title</a:t></a:r></a:p></p:sld>"#)
        .unwrap();
    z.start_file("ppt/slides/slide2.xml", opts).unwrap();
    z.write_all(br#"<p:sld><a:p><a:r><a:t>Slide two content</a:t></a:r></a:p></p:sld>"#)
        .unwrap();
    z.finish().unwrap();
}

/// Extract + insert, mirroring what the api_attach_* commands do.
async fn attach(pool: &SqlitePool, meeting_id: &str, path: &Path) {
    let file_type = detect_file_type(path);
    let extracted = extract_text(path, &file_type);
    AttachmentsRepository::insert(
        pool,
        meeting_id,
        &path.file_name().unwrap().to_string_lossy(),
        &file_type,
        &path.to_string_lossy(),
        extracted.as_deref(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn multi_source_session_end_to_end() {
    let pool = test_pool().await;
    let meeting_id = "meeting-e2e-test";
    insert_meeting(&pool, meeting_id).await;

    let tmp = tempdir();

    // --- transcript segments (what recording/import produces) ---
    for (i, (text, speaker)) in [
        ("Welcome to the lecture on dynamic programming.", "system"),
        ("What is memoization exactly?", "mic"),
    ]
    .iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("transcript-{}", i))
        .bind(meeting_id)
        .bind(text)
        .bind("2026-07-07T10:00:00Z")
        .bind(speaker)
        .bind(i as f64 * 10.0)
        .execute(&pool)
        .await
        .unwrap();
    }

    // --- attachments: pdf, docx, pptx, txt, image ---
    let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixture.pdf");
    attach(&pool, meeting_id, &pdf).await;

    let docx = tmp.join("handout.docx");
    write_docx(&docx);
    attach(&pool, meeting_id, &docx).await;

    let pptx = tmp.join("slides.pptx");
    write_pptx(&pptx);
    attach(&pool, meeting_id, &pptx).await;

    let txt = tmp.join("syllabus.txt");
    std::fs::write(&txt, "Course syllabus: greedy algorithms, DP, graphs.").unwrap();
    attach(&pool, meeting_id, &txt).await;

    let img = tmp.join("whiteboard.png");
    std::fs::write(&img, [0x89, 0x50, 0x4E, 0x47]).unwrap(); // header only; no OCR expected
    attach(&pool, meeting_id, &img).await;

    // --- user notes ---
    MeetingNotesRepository::upsert(
        &pool,
        meeting_id,
        Some("My own takeaway: **review recurrence relations**"),
        Some(r#"[{"type":"paragraph"}]"#),
    )
    .await
    .unwrap();

    // --- audio metadata ---
    let audio = tmp.join("audio.mp4");
    std::fs::write(&audio, vec![0u8; 2048]).unwrap();
    NoteAudioRepository::upsert_from_file(&pool, meeting_id, &audio, "recorded", None)
        .await
        .unwrap();

    // --- listing shows all five files, extraction status correct ---
    let listed = AttachmentsRepository::list_for_meeting(&pool, meeting_id)
        .await
        .unwrap();
    assert_eq!(listed.len(), 5);
    let by_name = |n: &str| listed.iter().find(|a| a.file_name == n).unwrap();
    assert!(by_name("fixture.pdf").has_extracted_text, "PDF text should extract");
    assert!(by_name("handout.docx").has_extracted_text);
    assert!(by_name("slides.pptx").has_extracted_text);
    assert!(by_name("syllabus.txt").has_extracted_text);
    assert!(!by_name("whiteboard.png").has_extracted_text, "images have no OCR yet");

    // --- audio metadata round-trip ---
    let audio_meta = NoteAudioRepository::get(&pool, meeting_id).await.unwrap().unwrap();
    assert_eq!(audio_meta.origin, "recorded");
    assert_eq!(audio_meta.current_size_bytes, Some(2048));
    assert!(audio_meta.retained);
    assert_eq!(audio_meta.format.as_deref(), Some("mp4"));

    // --- the assembled context contains every source, labeled ---
    let ctx = assemble_context(&pool, meeting_id).await.unwrap();
    assert_eq!(ctx.sources_used, vec!["transcript", "attachments", "user_notes"]);

    let transcript = ctx.transcript_text.as_deref().unwrap();
    assert!(transcript.contains("[system] Welcome to the lecture"));
    assert!(transcript.contains("[mic] What is memoization"));

    assert_eq!(ctx.attachments.len(), 4, "image without text is excluded from context");
    let prompt = ctx.to_prompt_text();
    assert!(prompt.contains("=== RECORDING TRANSCRIPT ==="));
    assert!(prompt.contains("=== UPLOADED FILE: fixture.pdf (pdf) ==="));
    assert!(prompt.contains("mitochondria"), "PDF body text should appear in prompt");
    assert!(prompt.contains("Homework due Friday"), "DOCX text should appear in prompt");
    assert!(prompt.contains("[Slide 2]\nSlide two content"), "PPTX slides labeled in order");
    assert!(prompt.contains("=== USER'S OWN NOTES ==="));
    assert!(prompt.contains("review recurrence relations"));

    // --- notes round-trip ---
    let notes = MeetingNotesRepository::get(&pool, meeting_id).await.unwrap().unwrap();
    assert_eq!(notes.notes_json.as_deref(), Some(r#"[{"type":"paragraph"}]"#));

    // --- delete an attachment ---
    let target = by_name("syllabus.txt").id.clone();
    assert!(AttachmentsRepository::delete(&pool, &target).await.unwrap());
    let after = AttachmentsRepository::list_for_meeting(&pool, meeting_id).await.unwrap();
    assert_eq!(after.len(), 4);
}

#[test]
fn all_bundled_templates_are_valid() {
    use app_lib::summary::templates::validate_and_parse_template;

    let templates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    let mut checked = 0;
    for entry in std::fs::read_dir(&templates_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let content = std::fs::read_to_string(&path).unwrap();
            validate_and_parse_template(&content).unwrap_or_else(|e| {
                panic!("Bundled template {:?} is invalid: {}", path.file_name().unwrap(), e)
            });
            checked += 1;
        }
    }
    // The three Synth context templates plus upstream's six
    assert!(checked >= 9, "expected at least 9 bundled templates, found {}", checked);

    for id in ["lecture", "discussion", "coffee_chat"] {
        assert!(
            templates_dir.join(format!("{}.json", id)).exists(),
            "missing context template '{}'",
            id
        );
    }
}

#[tokio::test]
async fn summary_input_includes_all_sources() {
    use app_lib::sources::assembler::augment_transcript_text;

    let pool = test_pool().await;
    let meeting_id = "meeting-augment-test";
    insert_meeting(&pool, meeting_id).await;

    let tmp = tempdir();
    let txt = tmp.join("agenda.txt");
    std::fs::write(&txt, "Agenda: budget review, hiring plan.").unwrap();
    attach(&pool, meeting_id, &txt).await;

    MeetingNotesRepository::upsert(&pool, meeting_id, Some("Remember to ask about Q3"), None)
        .await
        .unwrap();

    let ctx = assemble_context(&pool, meeting_id).await.unwrap();
    let live_transcript = "Let's start with the budget.";
    let (augmented, sources) = augment_transcript_text(live_transcript, &ctx);

    assert_eq!(sources, vec!["transcript", "attachments", "user_notes"]);
    assert!(augmented.starts_with(live_transcript), "transcript must come first");
    assert!(augmented.contains("=== SUPPLEMENTARY MATERIAL"));
    assert!(augmented.contains("=== UPLOADED FILE: agenda.txt (txt) ==="));
    assert!(augmented.contains("hiring plan"));
    assert!(augmented.contains("=== USER'S OWN NOTES ==="));
    assert!(augmented.contains("Remember to ask about Q3"));

    // A bare transcript with no other sources passes through untouched
    let empty_ctx = assemble_context(&pool, "meeting-augment-test-nonexistent")
        .await
        .unwrap();
    let (bare, bare_sources) = augment_transcript_text(live_transcript, &empty_ctx);
    assert_eq!(bare, live_transcript);
    assert_eq!(bare_sources, vec!["transcript"]);
}

#[tokio::test]
async fn empty_session_has_no_sources() {
    let pool = test_pool().await;
    insert_meeting(&pool, "meeting-empty").await;

    let ctx = assemble_context(&pool, "meeting-empty").await.unwrap();
    assert!(ctx.transcript_text.is_none());
    assert!(ctx.attachments.is_empty());
    assert!(ctx.user_notes_markdown.is_none());
    assert!(ctx.sources_used.is_empty());
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth-sources-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
