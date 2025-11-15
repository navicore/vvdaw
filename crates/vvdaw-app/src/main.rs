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

    // Create and start audio engine in a separate thread
    let mut engine = AudioEngine::new(audio_config);
    engine.start(audio_channels)?;

    tracing::info!("Audio engine started");

    // Create and run Bevy app with UI
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VVDAW - Visual Virtual DAW".to_string(),
                resolution: (800.0, 600.0).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(VvdawUiPlugin::new(ui_channels))
        .run();

    tracing::info!("vvdaw shutting down");

    // Stop the audio engine
    engine.stop()?;

    Ok(())
}
