//! vvdaw - Visual Virtual DAW
//!
//! Main application entry point.

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use vvdaw_audio::{AudioConfig, AudioEngine};
use vvdaw_comms::create_channels;

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
    let (_ui_channels, audio_channels) = create_channels(256);

    // Create audio configuration
    let audio_config = AudioConfig::default();
    tracing::info!("Audio config: {:?}", audio_config);

    // Create and start audio engine
    let mut engine = AudioEngine::new(audio_config);
    engine.start(audio_channels)?;

    tracing::info!("vvdaw initialized successfully");

    // TODO: Start Bevy UI

    // For now, just sleep to keep the app alive
    std::thread::sleep(std::time::Duration::from_secs(2));

    engine.stop()?;

    tracing::info!("vvdaw shutting down");

    Ok(())
}
