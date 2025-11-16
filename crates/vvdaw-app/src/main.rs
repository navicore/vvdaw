//! vvdaw - Visual Virtual DAW
//!
//! Main application entry point.

use anyhow::Result;
use bevy::prelude::*;
use clap::{Parser, ValueEnum};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use vvdaw_audio::{AudioConfig, AudioEngine};
use vvdaw_comms::create_channels;
use vvdaw_ui::VvdawUiPlugin;
use vvdaw_ui_3d::{Highway3dPlugin, waveform::WaveformData};

/// Visual Virtual DAW - An experimental 3D audio workstation
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// UI mode to use
    #[arg(short, long, value_enum, default_value_t = UiMode::ThreeD)]
    ui: UiMode,

    /// Optional WAV file to load and visualize (only used in 3D mode)
    wav_file: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum UiMode {
    /// 2D traditional UI with file browser and playback controls
    #[value(name = "2d")]
    TwoD,
    /// 3D highway visualization (default)
    #[value(name = "3d")]
    ThreeD,
}

fn main() -> Result<()> {
    // Parse command-line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vvdaw=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting vvdaw with UI mode: {:?}", args.ui);

    match args.ui {
        UiMode::TwoD => run_2d_ui(&args)?,
        UiMode::ThreeD => run_3d_ui(args)?,
    }

    Ok(())
}

/// Run the application with 2D UI
fn run_2d_ui(args: &Args) -> Result<()> {
    if args.wav_file.is_some() {
        tracing::warn!("WAV file argument is only used in 3D mode, ignoring");
    }

    // Create communication channels
    let (ui_channels, audio_channels) = create_channels(256);

    // Create audio configuration
    let audio_config = AudioConfig::default();
    tracing::info!("Audio config: {:?}", audio_config);

    // Create and start audio engine
    let mut engine = AudioEngine::new(audio_config);
    engine.start(audio_channels)?;

    tracing::info!("Audio engine started");

    // Create and run Bevy app with 2D UI
    //
    // IMPORTANT: In Bevy 0.15+, App::run() returns when:
    // - The window is closed
    // - An AppExit event is sent
    // - The process is terminated (Ctrl+C, SIGTERM, etc.)
    //
    // When App::run() returns or the process exits, the `engine` variable
    // goes out of scope and its Drop impl is called, which stops the audio
    // stream. This ensures proper cleanup in all exit scenarios.
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VVDAW - 2D UI".to_string(),
                resolution: (800, 600).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(VvdawUiPlugin::new(ui_channels))
        .run();

    tracing::info!("Bevy app exited - audio engine will be cleaned up");

    // NOTE: AudioEngine::drop() is called here automatically, which stops the audio stream.
    // Explicit engine.stop() is not needed, but we rely on the Drop impl for cleanup.

    Ok(())
}

/// Run the application with 3D highway UI
fn run_3d_ui(args: Args) -> Result<()> {
    let wav_path = args
        .wav_file
        .unwrap_or_else(|| "test_data/new-a-155.wav".to_string());

    tracing::info!("Loading WAV file: {}", wav_path);
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
            tracing::info!(
                "Loaded: {} frames at {}Hz",
                data.frame_count(),
                data.sample_rate
            );
            data
        }
        Err(e) => {
            anyhow::bail!("Failed to load WAV file: {e}");
        }
    };

    // Create and run Bevy app with 3D UI
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VVDAW - 3D Highway".to_string(),
                resolution: (1920, 1080).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(Highway3dPlugin)
        .insert_resource(waveform_data)
        .run();

    tracing::info!("Bevy app exited");

    Ok(())
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

    tracing::debug!(
        "WAV spec: {} channels, {}Hz, {} bits",
        channels,
        sample_rate,
        spec.bits_per_sample
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
            tracing::warn!("WAV file has {} channels, using only first 2", channels);
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
