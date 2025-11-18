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
        UiMode::ThreeD => run_3d_ui(&args),
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
fn run_3d_ui(args: &Args) {
    if args.wav_file.is_some() {
        tracing::warn!("WAV file argument ignored in 3D mode - use File > Load WAV menu instead");
    }

    tracing::info!("Starting 3D Highway UI");
    println!();
    println!("Controls:");
    println!("  W/A/S/D     - Move forward/left/back/right");
    println!("  Q/E         - Move up/down");
    println!("  Shift       - Speed boost");
    println!("  Right Mouse - Look around");
    println!("  Space       - Play/Pause");
    println!("  Tab         - Toggle camera mode");
    println!("  Ctrl+O      - Load WAV file");
    println!("  Esc         - Exit");
    println!();
    println!("Use File > Load WAV to get started!");
    println!();

    // Create and run Bevy app with 3D UI
    // The Highway3dPlugin now includes menu system and file loading
    vvdaw_ui_3d::create_app().run();

    tracing::info!("Bevy app exited");
}
