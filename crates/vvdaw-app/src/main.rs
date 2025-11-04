//! vvdaw - Visual Virtual DAW
//!
//! Main application entry point.

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use vvdaw_audio::{AudioConfig, AudioEngine};
use vvdaw_comms::{AudioCommand, AudioEvent, create_channels};

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
    let (mut ui_channels, audio_channels) = create_channels(256);

    // Create audio configuration
    let audio_config = AudioConfig::default();
    tracing::info!("Audio config: {:?}", audio_config);

    // Create and start audio engine
    let mut engine = AudioEngine::new(audio_config);
    engine.start(audio_channels)?;

    tracing::info!("vvdaw initialized successfully");

    // Send Start command to begin audio playback
    tracing::info!("Sending Start command to audio thread");
    if let Err(e) = ui_channels.command_tx.push(AudioCommand::Start) {
        tracing::error!("Failed to send Start command: {:?}", e);
        return Ok(()); // Exit gracefully
    }

    // Poll for events from audio thread
    let start_time = std::time::Instant::now();
    let run_duration = std::time::Duration::from_secs(3);

    tracing::info!(
        "Playing test tone for {} seconds...",
        run_duration.as_secs()
    );

    while start_time.elapsed() < run_duration {
        // Process events from audio thread
        while let Ok(event) = ui_channels.event_rx.pop() {
            match event {
                AudioEvent::Started => {
                    tracing::info!("Audio playback started");
                }
                AudioEvent::Stopped => {
                    tracing::info!("Audio playback stopped");
                }
                AudioEvent::Error(msg) => {
                    tracing::error!("Audio error: {}", msg);
                }
                AudioEvent::PeakLevel { channel, level } => {
                    // Only log occasionally to avoid spam
                    if level > 0.05 {
                        tracing::trace!("Peak level ch{}: {:.3}", channel, level);
                    }
                }
            }
        }

        // Sleep briefly to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Send Stop command
    tracing::info!("Sending Stop command to audio thread");
    if let Err(e) = ui_channels.command_tx.push(AudioCommand::Stop) {
        tracing::warn!("Failed to send Stop command: {:?}", e);
        // Continue with shutdown anyway
    }

    // Give audio thread time to process Stop command
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Process final events
    while let Ok(event) = ui_channels.event_rx.pop() {
        if matches!(event, AudioEvent::Stopped) {
            tracing::info!("Audio playback stopped");
        }
    }

    engine.stop()?;

    tracing::info!("vvdaw shutting down");

    Ok(())
}
