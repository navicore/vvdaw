//! 3D highway scene with real WAV file visualization
//!
//! Loads an actual audio file and visualizes its waveforms as 3D geometry.
//!
//! Usage:
//!   `cargo` run --example `highway_wav` -p vvdaw-ui-3d -- <path-to-wav>
//!
//! Controls:
//!   W/A/S/D - Move forward/left/back/right
//!   Q/E - Move up/down
//!   Shift - Speed boost
//!   Right Mouse + Move - Look around
//!   Esc - Exit

use vvdaw_comms::create_channels;
use vvdaw_ui_3d::waveform::WaveformData;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let wav_path = if args.len() > 1 {
        args[1].clone()
    } else {
        "test_data/new-a-155.wav".to_string()
    };

    println!("Starting 3D Highway UI with WAV file visualization...");
    println!("Loading: {wav_path}");
    println!();
    println!("Controls:");
    println!("  W/A/S/D - Move");
    println!("  Q/E - Up/Down");
    println!("  Shift - Speed boost");
    println!("  Right Mouse + Move - Look around");
    println!("  Esc - Exit");
    println!();

    // Load WAV file
    let waveform_data = match load_wav_file(&wav_path) {
        Ok(data) => {
            println!(
                "✓ Loaded: {} frames at {}Hz",
                data.frame_count(),
                data.sample_rate
            );
            data
        }
        Err(e) => {
            eprintln!("✗ Failed to load WAV file: {e}");
            eprintln!();
            eprintln!("Make sure the file exists and is a valid WAV file.");
            std::process::exit(1);
        }
    };

    // Create communication channels (visualization only - no audio engine)
    let (ui_channels, _audio_channels) = create_channels(256);

    // Create app with loaded waveform data
    let mut app = vvdaw_ui_3d::create_app(ui_channels);

    // Insert the loaded waveform data as a resource
    app.insert_resource(waveform_data);

    app.run();
}

/// Load a WAV file and return waveform data
fn load_wav_file(path: &str) -> Result<WaveformData, String> {
    use hound::WavReader;

    // Validate file size (500MB limit)
    const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024;
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Failed to read file metadata: {e}"))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "File too large: {:.1}MB (max 500MB)",
            metadata.len() as f64 / (1024.0 * 1024.0)
        ));
    }

    let mut reader = WavReader::open(path).map_err(|e| format!("Failed to open WAV file: {e}"))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Validate bit depth
    if spec.bits_per_sample == 0 || spec.bits_per_sample > 31 {
        return Err(format!(
            "Unsupported bit depth: {} bits (supported: 1-31)",
            spec.bits_per_sample
        ));
    }

    println!(
        "WAV spec: {} channels, {}Hz, {} bits",
        channels, sample_rate, spec.bits_per_sample
    );

    // Read all samples and convert to f32
    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read samples: {e}"))?,
        hound::SampleFormat::Int => {
            let max_value = (1_i32 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_value))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read samples: {e}"))?
        }
    };

    // Convert to interleaved stereo
    let stereo_samples = match channels {
        1 => {
            // Mono: duplicate to both channels
            let mut stereo = Vec::with_capacity(raw_samples.len() * 2);
            for sample in raw_samples {
                stereo.push(sample); // Left
                stereo.push(sample); // Right
            }
            stereo
        }
        2 => {
            // Already stereo
            raw_samples
        }
        _ => {
            // More than 2 channels: take first 2
            eprintln!("Warning: WAV file has {channels} channels, using only first 2");
            let mut stereo = Vec::with_capacity((raw_samples.len() / channels) * 2);
            for chunk in raw_samples.chunks(channels) {
                stereo.push(chunk[0]); // Left
                stereo.push(chunk[1]); // Right
            }
            stereo
        }
    };

    Ok(WaveformData::new(stereo_samples, sample_rate))
}
