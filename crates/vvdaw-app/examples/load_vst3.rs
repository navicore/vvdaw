//! Example: Load a VST3 plugin and process audio through it.
//!
//! This example demonstrates the complete workflow:
//! 1. Loading a VST3 plugin from a .vst3 bundle
//! 2. Starting the audio engine
//! 3. Sending the plugin to the audio thread
//! 4. Processing audio through the plugin
//!
//! Usage:
//!   `cargo run --example load_vst3 -- /path/to/plugin.vst3`
//!
//! Note: You need a real VST3 plugin installed to run this example.
//! Common locations:
//!   macOS: /Library/Audio/Plug-Ins/VST3/
//!   Windows: C:\Program Files\Common Files\VST3\
//!   Linux: ~/.vst3/

use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use vvdaw_audio::{AudioConfig, AudioEngine};
use vvdaw_comms::{AudioCommand, AudioEvent, create_channels};
use vvdaw_plugin::Plugin;
use vvdaw_vst3::load_plugin;

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // Get plugin path from command line
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <path-to-plugin.vst3>", args[0]);
        eprintln!();
        eprintln!("Common VST3 locations:");
        eprintln!("  macOS:   /Library/Audio/Plug-Ins/VST3/");
        eprintln!("  Windows: C:\\Program Files\\Common Files\\VST3\\");
        eprintln!("  Linux:   ~/.vst3/");
        std::process::exit(1);
    }

    let plugin_path = PathBuf::from(&args[1]);

    println!("\n=== VST3 Plugin Loading Example ===\n");
    println!("Loading plugin from: {}", plugin_path.display());

    // Step 1: Load the VST3 plugin
    println!("\n[1/5] Loading VST3 plugin...");
    let plugin = load_plugin(&plugin_path).context("Failed to load VST3 plugin")?;

    let plugin_info = plugin.info();
    println!("✓ Loaded: {} by {}", plugin_info.name, plugin_info.vendor);
    println!("  Version: {}", plugin_info.version);
    println!("  Inputs: {}", plugin.input_channels());
    println!("  Outputs: {}", plugin.output_channels());

    // Step 2: Create communication channels
    println!("\n[2/5] Creating communication channels...");
    let (mut ui_channels, audio_channels) = create_channels(256);
    println!("✓ Channels created");

    // Step 3: Start the audio engine
    println!("\n[3/5] Starting audio engine...");
    let config = AudioConfig::default();
    println!("  Sample rate: {} Hz", config.sample_rate);
    println!("  Block size: {} frames", config.block_size);

    let mut engine = AudioEngine::new(config);

    engine
        .start(audio_channels)
        .context("Failed to start audio engine")?;
    println!("✓ Audio engine started");

    // Give the engine time to initialize
    thread::sleep(Duration::from_millis(100));

    // Step 4: Send the plugin to the audio thread
    println!("\n[4/5] Sending plugin to audio thread...");
    ui_channels
        .plugin_tx
        .send(Box::new(plugin))
        .map_err(|_| anyhow::anyhow!("Failed to send plugin to audio thread"))?;

    // Send AddNode command
    ui_channels
        .command_tx
        .push(AudioCommand::AddNode)
        .map_err(|_| anyhow::anyhow!("Failed to send AddNode command"))?;

    println!("✓ Plugin sent to audio thread");

    // Give the audio thread time to process the AddNode command
    thread::sleep(Duration::from_millis(200));

    // Step 5: Start audio processing
    println!("\n[5/5] Starting audio playback...");
    ui_channels
        .command_tx
        .push(AudioCommand::Start)
        .map_err(|_| anyhow::anyhow!("Failed to send Start command"))?;

    println!("✓ Audio processing started");

    // Monitor events from the audio thread
    println!("\n=== Audio Processing (monitoring for 5 seconds) ===\n");
    let start_time = std::time::Instant::now();
    let monitor_duration = Duration::from_secs(5);

    let mut peak_count = 0;
    let mut started_received = false;

    while start_time.elapsed() < monitor_duration {
        while let Ok(event) = ui_channels.event_rx.pop() {
            match event {
                AudioEvent::Started => {
                    if !started_received {
                        println!("→ Audio processing STARTED");
                        started_received = true;
                    }
                }
                AudioEvent::Stopped => {
                    println!("→ Audio processing STOPPED");
                }
                AudioEvent::PeakLevel { channel, level } => {
                    peak_count += 1;
                    if peak_count % 100 == 0 {
                        // Print peak levels occasionally
                        println!("  Peak [ch {channel}]: {level:.4}");
                    }
                }
                AudioEvent::Error(msg) => {
                    eprintln!("✗ Audio error: {msg}");
                }
                AudioEvent::NodeAdded { node_id } => {
                    println!("→ Node {node_id} added to audio graph");
                }
            }
        }

        thread::sleep(Duration::from_millis(50));
    }

    println!("\n=== Shutting Down ===\n");

    // Stop audio processing
    println!("[1/2] Stopping audio...");
    ui_channels
        .command_tx
        .push(AudioCommand::Stop)
        .map_err(|_| anyhow::anyhow!("Failed to send Stop command"))?;

    thread::sleep(Duration::from_millis(100));

    // Stop the engine
    println!("[2/2] Stopping engine...");
    engine.stop()?;

    println!("\n✓ Example completed successfully!\n");

    Ok(())
}
