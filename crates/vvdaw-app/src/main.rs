//! vvdaw - Visual Virtual DAW
//!
//! Main application entry point.

use anyhow::Result;
use bevy::prelude::*;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use vvdaw_audio::{AudioConfig, AudioEngine};
use vvdaw_comms::create_channels;
use vvdaw_ui::VvdawUiPlugin;

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vvdaw=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting vvdaw...");

    // Create communication channels
    let (ui_channels, audio_channels) = create_channels(256);

    // Create audio configuration
    let audio_config = AudioConfig::default();
    tracing::info!("Audio config: {:?}", audio_config);

    // Create and start audio engine
    let mut engine = AudioEngine::new(audio_config);
    engine.start(audio_channels)?;

    tracing::info!("Audio engine started");

    // Create and run Bevy app with UI
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
                title: "VVDAW - Visual Virtual DAW".to_string(),
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
