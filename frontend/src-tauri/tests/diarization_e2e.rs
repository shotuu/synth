//! Real-audio diarization test: synthesizes a two-voice conversation with
//! macOS `say`, downloads the ONNX models (once, cached in target/), runs
//! the full pipeline, and checks that two speakers come out.
//!
//! Ignored by default (needs macOS `say`, ffmpeg, network for first run):
//!   cargo test --test diarization_e2e -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::process::Command;

use app_lib::diarization::pipeline::{assign_speaker_labels, diarize_file, distinct_speakers};

const MODELS: &[(&str, &str)] = &[
    (
        "segmentation-3.0.onnx",
        "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx",
    ),
    (
        "wespeaker_en_voxceleb_CAM++.onnx",
        "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx",
    ),
];

fn models_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/diarization-test-models")
}

fn ensure_models() -> (PathBuf, PathBuf) {
    let dir = models_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for (name, url) in MODELS {
        let dest = dir.join(name);
        if dest.exists() && std::fs::metadata(&dest).unwrap().len() > 1_000_000 {
            continue;
        }
        eprintln!("Downloading {} …", name);
        let status = Command::new("curl")
            .args(["-L", "--fail", "-o"])
            .arg(&dest)
            .arg(*url)
            .status()
            .expect("curl not available");
        assert!(status.success(), "failed to download {}", name);
    }
    (dir.join(MODELS[0].0), dir.join(MODELS[1].0))
}

/// Build a ~40s two-speaker conversation wav using two very different
/// macOS voices, with short silences between turns.
fn synthesize_conversation(dir: &Path) -> PathBuf {
    let turns: &[(&str, &str)] = &[
        ("Samantha", "Good morning everyone, let's get started with the budget review. We have a lot of ground to cover today and I want to make sure we get through all of it together."),
        ("Lee", "Thanks for setting this up. I looked through the numbers last night and I have a few concerns about the marketing spend in the third quarter."),
        ("Samantha", "That's a fair point. The marketing budget did grow faster than we projected, mostly because of the conference sponsorships we added in June."),
        ("Lee", "Right, and I think we should consider scaling those back next year unless we can clearly measure the return on that investment."),
    ];

    let mut turn_wavs = Vec::new();
    for (i, (voice, text)) in turns.iter().enumerate() {
        let aiff = dir.join(format!("turn{}.aiff", i));
        let status = Command::new("say")
            .args(["-v", voice, "-o"])
            .arg(&aiff)
            .arg(text)
            .status()
            .expect("`say` not available (macOS only test)");
        assert!(status.success(), "say failed for voice {}", voice);
        turn_wavs.push(aiff);
    }

    // Concatenate with 700ms silences, output 16kHz mono wav
    let silence = dir.join("silence.wav");
    Command::new("ffmpeg")
        .args(["-y", "-f", "lavfi", "-i", "anullsrc=r=16000:cl=mono", "-t", "0.7"])
        .arg(&silence)
        .status()
        .expect("ffmpeg not available");

    let list = dir.join("concat.txt");
    let mut list_content = String::new();
    for (i, wav) in turn_wavs.iter().enumerate() {
        // Convert each aiff to 16k mono wav first
        let converted = dir.join(format!("turn{}.wav", i));
        Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(wav)
            .args(["-ar", "16000", "-ac", "1"])
            .arg(&converted)
            .status()
            .unwrap();
        list_content.push_str(&format!("file '{}'\n", converted.display()));
        list_content.push_str(&format!("file '{}'\n", silence.display()));
    }
    std::fs::write(&list, list_content).unwrap();

    let output = dir.join("conversation.wav");
    let status = Command::new("ffmpeg")
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy"])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success(), "ffmpeg concat failed");
    output
}

#[test]
#[ignore = "needs macOS say + ffmpeg + one-time model download"]
fn two_voice_conversation_yields_two_speakers() {
    let tmp = std::env::temp_dir().join(format!("synth-diarize-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let (seg_model, emb_model) = ensure_models();
    let audio = synthesize_conversation(&tmp);

    let mut last_progress = 0u8;
    let spans = diarize_file(&audio, &seg_model, &emb_model, 8, |p| {
        last_progress = p;
    })
    .expect("diarization failed");

    eprintln!("Got {} spans:", spans.len());
    for s in &spans {
        eprintln!("  {:.1}-{:.1}s → Speaker {}", s.start, s.end, s.speaker);
    }

    assert!(!spans.is_empty(), "no speech segments detected");
    assert!(last_progress >= 90, "progress callback should reach the end");

    let speakers = distinct_speakers(&spans);
    assert!(
        (2..=3).contains(&speakers),
        "expected ~2 speakers for a two-voice conversation, got {}",
        speakers
    );

    // The first and second turns are different voices, so the dominant
    // speaker of the first ~8s should differ from the following turn.
    let rows = vec![
        ("turn1".to_string(), Some(0.5), Some(7.5)),
        ("turn2".to_string(), Some(10.0), Some(16.0)),
    ];
    let labels = assign_speaker_labels(&rows, &spans);
    let (l1, l2) = (labels[0].1.clone(), labels[1].1.clone());
    eprintln!("turn1 → {:?}, turn2 → {:?}", l1, l2);
    assert!(l1.is_some() && l2.is_some(), "both turns should get labels");
    assert_ne!(l1, l2, "different voices should get different speaker labels");
}
