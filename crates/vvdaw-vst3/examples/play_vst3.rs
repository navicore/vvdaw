//! Example: Load a VST3 plugin and play it through the audio engine
//!
//! **This example produces AUDIBLE SOUND through your speakers!**
//!
//! Run with: `cargo run --example play_vst3 --release`
//!
//! Defaults to Noises.vst3 which generates noise you can hear.
//! Effect plugins need input audio - use `process_wav` example for those.

use std::time::Duration;
use tracing_subscriber::EnvFilter;
use vvdaw_audio::{AudioConfig, AudioEngine};
use vvdaw_comms::{AudioCommand, AudioEvent, create_channels};
use vvdaw_plugin::Plugin;

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .init();

    // Default to Noises.vst3 which generates audio
    // Effect plugins like ThingsCrusher need input audio (use process_wav example instead)
    let plugin_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/Library/Audio/Plug-Ins/VST3/Noises.vst3".to_string());

    println!("Loading VST3 plugin: {plugin_path}...\n");

    // Load VST3 plugin
    let plugin = vvdaw_vst3::load_plugin(&plugin_path)?;

    println!("✓ Plugin loaded: {}", plugin.info().name);
    println!("  Vendor: {}", plugin.info().vendor);
    println!(
        "  Inputs: {} / Outputs: {}",
        plugin.input_channels(),
        plugin.output_channels()
    );

    // Create communication channels
    let (mut ui_channels, audio_channels) = create_channels(256);

    // Create audio engine
    let audio_config = AudioConfig::default();
    let mut engine = AudioEngine::new(audio_config);
    engine.start(audio_channels)?;
    println!("\n✓ Audio engine started");

    // Add plugin to audio graph
    println!("\nAdding plugin to audio graph...");
    ui_channels.command_tx.push(AudioCommand::AddNode)?;
    ui_channels
        .plugin_tx
        .send(Box::new(plugin))
        .map_err(|e| anyhow::anyhow!("Failed to send plugin: {e}"))?;

    // Give the audio thread time to add the node
    std::thread::sleep(Duration::from_millis(100));

    // Start audio playback
    println!("Starting audio playback...");
    ui_channels.command_tx.push(AudioCommand::Start)?;

    // Wait for Started event
    let timeout = std::time::Instant::now() + Duration::from_secs(1);
    let mut started = false;
    while std::time::Instant::now() < timeout {
        while let Ok(event) = ui_channels.event_rx.pop() {
            if matches!(event, AudioEvent::Started) {
                started = true;
                break;
            }
        }
        if started {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    if started {
        println!("✓ Audio started");
    } else {
        println!("⚠ Didn't receive Started event");
    }

    // Play for 5 seconds and monitor peak levels
    println!("\nPlaying for 5 seconds... (monitoring peak levels)\n");
    let play_duration = Duration::from_secs(5);
    let play_start = std::time::Instant::now();
    let mut max_peak = 0.0f32;
    let mut sample_count = 0;

    while play_start.elapsed() < play_duration {
        while let Ok(event) = ui_channels.event_rx.pop() {
            match event {
                AudioEvent::PeakLevel { channel, level } => {
                    sample_count += 1;
                    max_peak = max_peak.max(level);

                    // Log every 100th sample or if level is significant
                    if sample_count % 100 == 0 || level > 0.1 {
                        println!("  Ch{channel} peak: {level:.4}");
                    }
                }
                AudioEvent::Error(msg) => {
                    eprintln!("  Audio error: {msg}");
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    println!("\nPlayback complete!");
    println!("  Max peak level: {max_peak:.4}");
    println!("  Total samples: {sample_count}");

    if max_peak > 0.0 {
        println!("\n✓ SUCCESS: Plugin generated audio output!");
    } else {
        println!("\n⚠ Plugin output was silence");
        println!("  (This might be expected - Lines may need MIDI input)");
    }

    // Stop audio
    println!("\nStopping audio...");
    ui_channels.command_tx.push(AudioCommand::Stop)?;
    std::thread::sleep(Duration::from_millis(200));

    // Drain remaining events
    while let Ok(_event) = ui_channels.event_rx.pop() {}

    engine.stop()?;
    println!("✓ Audio engine stopped");

    println!("\n✓ Test complete!");

    Ok(())
}
