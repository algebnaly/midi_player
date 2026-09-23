//! Audio export module for WAV and MP3 formats.
//!
//! Provides offline rendering and file encoding for project audio data.

use anyhow::{Context, Result, anyhow};
use std::path::Path;

/// Export interleaved stereo samples to a standard 16-bit PCM WAV file.
pub fn export_wav(
    path: impl AsRef<Path>,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
) -> Result<()> {
    let path = path.as_ref();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("Failed to create WAV file: {}", path.display()))?;

    for (&l, &r) in left.iter().zip(right.iter()) {
        let sample_l = (l.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        let sample_r = (r.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        writer.write_sample(sample_l)?;
        writer.write_sample(sample_r)?;
    }

    writer.finalize().context("Failed to finalize WAV file")?;
    Ok(())
}

/// Export stereo samples to a high-quality 320 kbps MP3 file using LAME encoder.
pub fn export_mp3(
    path: impl AsRef<Path>,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
) -> Result<()> {
    let path = path.as_ref();
    let mut builder = mp3lame_encoder::Builder::new()
        .ok_or_else(|| anyhow!("Failed to initialize MP3 LAME encoder"))?;

    builder
        .set_num_channels(2)
        .map_err(|e| anyhow!("Failed to set channels: {:?}", e))?;
    builder
        .set_sample_rate(sample_rate)
        .map_err(|e| anyhow!("Failed to set sample rate: {:?}", e))?;
    builder
        .set_brate(mp3lame_encoder::Bitrate::Kbps320)
        .map_err(|e| anyhow!("Failed to set bitrate: {:?}", e))?;
    builder
        .set_quality(mp3lame_encoder::Quality::Best)
        .map_err(|e| anyhow!("Failed to set quality: {:?}", e))?;

    let mut encoder = builder
        .build()
        .map_err(|e| anyhow!("Failed to build MP3 encoder: {:?}", e))?;

    let pcm_left: Vec<i16> = left
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect();
    let pcm_right: Vec<i16> = right
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect();

    let mut mp3_buffer = Vec::new();
    let input = mp3lame_encoder::DualPcm {
        left: &pcm_left,
        right: &pcm_right,
    };

    mp3_buffer.reserve(mp3lame_encoder::max_required_buffer_size(pcm_left.len()));
    encoder
        .encode_to_vec(input, &mut mp3_buffer)
        .map_err(|e| anyhow!("MP3 encoding error: {:?}", e))?;

    mp3_buffer.reserve(mp3lame_encoder::max_required_buffer_size(1152));
    encoder
        .flush_to_vec::<mp3lame_encoder::FlushNoGap>(&mut mp3_buffer)
        .map_err(|e| anyhow!("MP3 flush error: {:?}", e))?;

    std::fs::write(path, mp3_buffer)
        .with_context(|| format!("Failed to write MP3 file: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_export_roundtrip() {
        let sample_rate = 44100;
        let left = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let right = vec![0.0, -0.5, 0.5, -1.0, 1.0];

        let tmp_path = std::env::temp_dir().join("test_output.wav");
        export_wav(&tmp_path, &left, &right, sample_rate).unwrap();

        assert!(tmp_path.exists());
        let reader = hound::WavReader::open(&tmp_path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, sample_rate);
        assert_eq!(reader.len(), 10); // 5 frames * 2 channels
        let _ = std::fs::remove_file(tmp_path);
    }

    #[test]
    fn test_mp3_export_creates_valid_file() {
        let sample_rate = 44100;
        // Generate 0.1 second of 440 Hz sine wave
        let frames = (sample_rate as f64 * 0.1) as usize;
        let left: Vec<f32> = (0..frames)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * (i as f32 / sample_rate as f32)).sin())
            .collect();
        let right = left.clone();

        let tmp_path = std::env::temp_dir().join("test_output.mp3");
        export_mp3(&tmp_path, &left, &right, sample_rate).unwrap();

        assert!(tmp_path.exists());
        let metadata = std::fs::metadata(&tmp_path).unwrap();
        assert!(metadata.len() > 0);
        let _ = std::fs::remove_file(tmp_path);
    }
}
