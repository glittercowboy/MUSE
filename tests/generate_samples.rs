/// Test that generates the WAV sample files needed by drum_machine.muse.
/// Run with `cargo test generate_sample_wavs -- --ignored` to regenerate.
/// The samples directory should already contain the committed WAV files.
use std::path::Path;

fn project_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn samples_dir() -> std::path::PathBuf {
    project_root().join("examples").join("samples")
}

#[test]
fn generate_sample_wavs() {
    let samples_dir = samples_dir();
    std::fs::create_dir_all(&samples_dir).unwrap();

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    // kick.wav — 60Hz sine burst, ~100ms (4410 samples)
    {
        let path = samples_dir.join("kick.wav");
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        let num_samples = 4410;
        for i in 0..num_samples {
            let t = i as f64 / 44100.0;
            let freq = 60.0;
            let envelope = 1.0 - (i as f64 / num_samples as f64); // linear decay
            let sample = (2.0 * std::f64::consts::PI * freq * t).sin() * envelope * 0.8;
            writer.write_sample((sample * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 100);
    }

    // snare.wav — white noise burst, ~200ms (8820 samples) 
    {
        let path = samples_dir.join("snare.wav");
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        let num_samples = 8820;
        // Simple pseudo-random noise using LCG
        let mut rng_state: u64 = 12345;
        for i in 0..num_samples {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let noise = ((rng_state >> 33) as i64 as f64) / (u32::MAX as f64 / 2.0) - 1.0;
            let envelope = 1.0 - (i as f64 / num_samples as f64);
            let sample = noise * envelope * 0.6;
            writer.write_sample((sample * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        assert!(path.exists());
    }

    // hihat.wav — high-frequency noise burst, ~50ms (2205 samples)
    {
        let path = samples_dir.join("hihat.wav");
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        let num_samples = 2205;
        let mut rng_state: u64 = 67890;
        for i in 0..num_samples {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let noise = ((rng_state >> 33) as i64 as f64) / (u32::MAX as f64 / 2.0) - 1.0;
            let envelope = 1.0 - (i as f64 / num_samples as f64);
            let sample = noise * envelope * 0.5;
            writer.write_sample((sample * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        assert!(path.exists());
    }
}

#[test]
fn verify_sample_wavs_exist() {
    let samples_dir = samples_dir();
    for name in &["kick.wav", "snare.wav", "hihat.wav"] {
        let path = samples_dir.join(name);
        assert!(path.exists(), "Missing sample file: {}", path.display());
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 44, "WAV file too small: {} ({} bytes)", name, metadata.len());
    }
}

#[test]
fn verify_sample_wavs_decodable() {
    let samples_dir = samples_dir();
    for name in &["kick.wav", "snare.wav", "hihat.wav"] {
        let path = samples_dir.join(name);
        let reader = hound::WavReader::open(&path)
            .unwrap_or_else(|e| panic!("Cannot decode {}: {}", name, e));
        let spec = reader.spec();
        assert_eq!(spec.channels, 1, "{} should be mono", name);
        assert_eq!(spec.sample_rate, 44100, "{} should be 44100Hz", name);
        let sample_count = reader.len() as usize;
        assert!(sample_count > 0, "{} should have samples", name);
    }
}
