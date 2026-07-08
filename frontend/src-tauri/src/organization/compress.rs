/// Audio compression for the Storage Manager (PROJECT_BRIEF.md §4): re-encode
/// retained audio to a low-bitrate mono Opus file. Opus at ~24kbps mono is
/// effectively transparent for speech and dramatically smaller than the
/// original recording (AAC 192kbps or raw PCM).
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::audio::ffmpeg::find_ffmpeg_path;

pub const TARGET_BITRATE_KBPS: i64 = 24;

/// Re-encode `source` to Opus at TARGET_BITRATE_KBPS, writing next to it
/// with a `.opus` extension. Returns the new file's path and size.
/// The source is left in place; the caller swaps it in only after this
/// succeeds, so a failed compression never loses the original audio.
pub fn compress_to_opus(source: &Path) -> Result<(PathBuf, u64)> {
    if !source.is_file() {
        return Err(anyhow!("Source audio not found: {}", source.display()));
    }

    let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| anyhow!("FFmpeg not found"))?;
    let dest = source.with_extension("compressing.opus");

    let mut command = Command::new(ffmpeg_path);
    command
        .args([
            "-y",
            "-i",
        ])
        .arg(source)
        .args([
            "-c:a",
            "libopus",
            "-b:a",
            &format!("{}k", TARGET_BITRATE_KBPS),
            "-ac",
            "1",
            "-vn",
        ])
        .arg(&dest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().map_err(|e| anyhow!("Failed to run ffmpeg: {}", e))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&dest);
        return Err(anyhow!(
            "ffmpeg compression failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let size = std::fs::metadata(&dest)
        .map_err(|e| anyhow!("Compressed file missing after encode: {}", e))?
        .len();
    if size == 0 {
        let _ = std::fs::remove_file(&dest);
        return Err(anyhow!("Compressed output is empty"));
    }

    // Rename to the final .opus name (not source-with-extra-extension)
    let final_dest = source.with_extension("opus");
    std::fs::rename(&dest, &final_dest)
        .map_err(|e| anyhow!("Failed to finalize compressed file: {}", e))?;

    Ok((final_dest, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates a short real audio file and confirms ffmpeg actually
    /// shrinks it, rather than mocking the encoder.
    #[test]
    #[ignore = "needs ffmpeg + real audio generation, run explicitly"]
    fn compresses_real_audio_and_shrinks_it() {
        let ffmpeg = find_ffmpeg_path().expect("ffmpeg must be available for this test");
        let tmp = std::env::temp_dir().join(format!("synth-compress-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let source = tmp.join("source.wav");

        // 5 seconds of a 440Hz tone, uncompressed 16-bit PCM -- a real,
        // sizeable audio file to compress, not a stub.
        let status = Command::new(&ffmpeg)
            .args(["-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=5", "-ar", "44100"])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());

        let original_size = std::fs::metadata(&source).unwrap().len();
        let (compressed_path, compressed_size) = compress_to_opus(&source).unwrap();

        assert!(compressed_path.exists());
        assert!(
            compressed_size < original_size / 4,
            "expected significant size reduction: {} -> {}",
            original_size,
            compressed_size
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn errors_on_missing_source() {
        let missing = PathBuf::from("/tmp/synth-does-not-exist-xyz.mp4");
        assert!(compress_to_opus(&missing).is_err());
    }
}
