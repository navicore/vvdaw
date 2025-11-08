//! Standalone VST3 Plugin Host Process
//!
//! This binary runs in a separate process to host a single VST3 plugin.
//! It communicates with the main process via:
//! - Shared memory for audio buffers (zero-copy, real-time safe)
//! - stdin/stdout for control messages (JSON)
//! - Atomic flags for synchronization
//!
//! This isolates the plugin so that:
//! - Plugin crashes don't kill the main application
//! - Plugin memory issues are isolated
//! - Each plugin gets its own Objective-C runtime (no class conflicts)
//! - Plugins can run in parallel on different CPU cores
//!
//! Usage: plugin-host <path-to-plugin.vst3> <shm-name>
//!
//! The host process will:
//! 1. Load the plugin
//! 2. Open shared memory region
//! 3. Send Ready message to stdout
//! 4. Wait for control messages from stdin
//! 5. Process audio when signaled via atomic flags
//! 6. Shutdown gracefully on Shutdown message

use std::env;
use std::io::{self, BufRead, Write};
use std::process;
use std::sync::Arc;
use vvdaw_plugin::Plugin;
use vvdaw_vst3::{ControlMessage, ProcessState, ResponseMessage, SharedAudioBuffer};

fn main() {
    // Set up panic handler to notify main process of crashes
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("PLUGIN CRASHED: {panic_info}");
        process::exit(2); // Exit code 2 indicates crash
    }));

    // Get command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        let error = ResponseMessage::Error {
            message: "Usage: plugin-host <path-to-plugin.vst3> <shm-name>".to_string(),
        };
        println!("{}", serde_json::to_string(&error).unwrap());
        process::exit(1);
    }

    let plugin_path = &args[1];
    let shm_name = &args[2];

    // Load the plugin
    let mut plugin = match vvdaw_vst3::load_plugin(plugin_path) {
        Ok(p) => p,
        Err(e) => {
            let error = ResponseMessage::Error {
                message: format!("Failed to load plugin: {e}"),
            };
            println!("{}", serde_json::to_string(&error).unwrap());
            process::exit(1);
        }
    };

    // Get plugin info
    let info = plugin.info().clone();

    // Open shared memory region
    // TODO: Platform-specific shared memory implementation
    // For now, we'll use a placeholder
    #[allow(clippy::let_underscore_untyped)]
    let _ = shm_name; // Placeholder - will be used when implementing shared memory

    // Send Ready response
    let ready = ResponseMessage::Ready { info };
    println!("{}", serde_json::to_string(&ready).unwrap());
    io::stdout().flush().unwrap();

    // Process control messages from stdin
    let stdin = io::stdin();
    let reader = stdin.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Error reading stdin: {e}");
                break;
            }
        };

        // Parse control message
        let message: ControlMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                let error = ResponseMessage::Error {
                    message: format!("Failed to parse control message: {e}"),
                };
                println!("{}", serde_json::to_string(&error).unwrap());
                io::stdout().flush().unwrap();
                continue;
            }
        };

        // Handle control message
        match handle_control_message(&mut plugin, &message) {
            Ok(response) => {
                if let Some(resp) = response {
                    println!("{}", serde_json::to_string(&resp).unwrap());
                    io::stdout().flush().unwrap();
                }
            }
            Err(e) => {
                let error = ResponseMessage::Error { message: e };
                println!("{}", serde_json::to_string(&error).unwrap());
                io::stdout().flush().unwrap();
            }
        }
    }

    // Clean up
    plugin.deactivate();
}

/// Handle a control message and return optional response
fn handle_control_message(
    plugin: &mut vvdaw_vst3::Vst3Plugin,
    message: &ControlMessage,
) -> Result<Option<ResponseMessage>, String> {
    match message {
        ControlMessage::Init {
            sample_rate,
            max_block_size,
        } => {
            plugin
                .initialize(*sample_rate, *max_block_size)
                .map_err(|e| format!("Failed to initialize plugin: {e}"))?;
            Ok(Some(ResponseMessage::Initialized))
        }

        ControlMessage::Activate => {
            // VST3 plugins are activated in initialize()
            // This is here for protocol compatibility
            Ok(Some(ResponseMessage::Activated))
        }

        ControlMessage::Deactivate => {
            plugin.deactivate();
            Ok(Some(ResponseMessage::Deactivated))
        }

        ControlMessage::Shutdown => {
            plugin.deactivate();
            process::exit(0);
        }

        ControlMessage::SetParameter { id, value } => {
            plugin
                .set_parameter(*id, *value)
                .map_err(|e| format!("Failed to set parameter: {e}"))?;
            Ok(None) // No response needed for set parameter
        }

        ControlMessage::GetParameter { id } => {
            let value = plugin
                .get_parameter(*id)
                .map_err(|e| format!("Failed to get parameter: {e}"))?;
            Ok(Some(ResponseMessage::ParameterValue { id: *id, value }))
        }
    }
}

/// Audio processing loop (will be called from main loop when implemented)
#[allow(dead_code)]
fn audio_processing_loop(
    _plugin: &mut vvdaw_vst3::Vst3Plugin,
    shared_buffer: &Arc<SharedAudioBuffer>,
) {
    loop {
        // Wait for Process signal
        if !shared_buffer.wait_for_state(ProcessState::Process, 1_000_000) {
            // 1 second timeout
            eprintln!("Audio processing timeout");
            continue;
        }

        // TODO: Process audio
        // 1. Read input buffers from shared memory
        // 2. Read events from shared memory
        // 3. Call plugin.process()
        // 4. Write output buffers to shared memory

        // Signal completion
        shared_buffer.set_state(ProcessState::Done);
    }
}
