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
use std::sync::{Arc, Mutex};
use vvdaw_plugin::Plugin;
use vvdaw_vst3::{ControlMessage, ProcessState, ResponseMessage, SharedAudioBuffer, SharedMemory};

/// Send-safe wrapper for shared audio buffer pointer
struct SendSharedBuffer(*const SharedAudioBuffer);

#[allow(unsafe_code)]
unsafe impl Send for SendSharedBuffer {}

#[allow(clippy::too_many_lines)]
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
        let json = serde_json::to_string(&error)
            .unwrap_or_else(|_| r#"{"Error":{"message":"Usage error"}}"#.to_string());
        println!("{json}");
        process::exit(1);
    }

    let plugin_path = &args[1];
    let shm_name = &args[2];

    // Load the plugin
    let plugin = match vvdaw_vst3::load_plugin(plugin_path) {
        Ok(p) => p,
        Err(e) => {
            let error = ResponseMessage::Error {
                message: format!("Failed to load plugin: {e}"),
            };
            let json = serde_json::to_string(&error)
                .unwrap_or_else(|_| r#"{"Error":{"message":"Plugin load error"}}"#.to_string());
            println!("{json}");
            process::exit(1);
        }
    };

    // Get plugin info
    let info = plugin.info().clone();

    // Open shared memory region
    #[allow(clippy::used_underscore_binding)]
    let _shared_memory =
        match SharedMemory::open(shm_name, std::mem::size_of::<SharedAudioBuffer>()) {
            Ok(shm) => shm,
            Err(e) => {
                let error = ResponseMessage::Error {
                    message: format!("Failed to open shared memory: {e}"),
                };
                let json = serde_json::to_string(&error).unwrap_or_else(|_| {
                    r#"{"Error":{"message":"Shared memory error"}}"#.to_string()
                });
                println!("{json}");
                process::exit(1);
            }
        };

    // Get pointer to shared audio buffer
    // Safety: SharedMemory is valid and the buffer layout matches SharedAudioBuffer
    #[allow(unsafe_code)]
    #[allow(clippy::used_underscore_binding)]
    let shared_buffer_ptr = SendSharedBuffer(unsafe {
        std::ptr::from_ref(_shared_memory.as_ref::<SharedAudioBuffer>())
    });

    // Wrap plugin in Arc<Mutex<>> for thread-safe sharing
    let plugin = Arc::new(Mutex::new(plugin));

    // Clone Arc for audio thread (increments reference count)
    let plugin_for_audio = plugin.clone();

    // Spawn audio processing thread
    let audio_thread = std::thread::spawn(move || {
        audio_processing_loop(&plugin_for_audio, &shared_buffer_ptr);
    });

    // Send Ready response
    let ready = ResponseMessage::Ready { info };
    let json =
        serde_json::to_string(&ready).unwrap_or_else(|_| r#"{"Ready":{"info":null}}"#.to_string());
    println!("{json}");
    if let Err(e) = io::stdout().flush() {
        eprintln!("Failed to flush stdout: {e}");
    }

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
                let json = serde_json::to_string(&error)
                    .unwrap_or_else(|_| r#"{"Error":{"message":"Parse error"}}"#.to_string());
                println!("{json}");
                let _ = io::stdout().flush();
                continue;
            }
        };

        // Handle control message
        match handle_control_message(&plugin, &message) {
            Ok(response) => {
                if let Some(resp) = response {
                    let json = serde_json::to_string(&resp).unwrap_or_else(|_| {
                        r#"{"Error":{"message":"Response serialization error"}}"#.to_string()
                    });
                    println!("{json}");
                    let _ = io::stdout().flush();
                }
            }
            Err(e) => {
                let error = ResponseMessage::Error { message: e };
                let json = serde_json::to_string(&error)
                    .unwrap_or_else(|_| r#"{"Error":{"message":"Handler error"}}"#.to_string());
                println!("{json}");
                let _ = io::stdout().flush();
            }
        }
    }

    // Clean up
    if let Ok(mut plugin) = plugin.lock() {
        plugin.deactivate();
    }

    // Wait for audio thread to finish (it won't, but join anyway for clean shutdown)
    let _ = audio_thread.join();
}

/// Handle a control message and return optional response
fn handle_control_message(
    plugin: &Arc<Mutex<vvdaw_vst3::Vst3Plugin>>,
    message: &ControlMessage,
) -> Result<Option<ResponseMessage>, String> {
    match message {
        ControlMessage::Init {
            sample_rate,
            max_block_size,
        } => {
            plugin
                .lock()
                .map_err(|e| format!("Plugin lock poisoned: {e}"))?
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
            plugin
                .lock()
                .map_err(|e| format!("Plugin lock poisoned: {e}"))?
                .deactivate();
            Ok(Some(ResponseMessage::Deactivated))
        }

        ControlMessage::Shutdown => {
            plugin
                .lock()
                .map_err(|e| format!("Plugin lock poisoned: {e}"))?
                .deactivate();
            process::exit(0);
        }

        ControlMessage::SetParameter { id, value } => {
            plugin
                .lock()
                .map_err(|e| format!("Plugin lock poisoned: {e}"))?
                .set_parameter(*id, *value)
                .map_err(|e| format!("Failed to set parameter: {e}"))?;
            Ok(None) // No response needed for set parameter
        }

        ControlMessage::GetParameter { id } => {
            let value = plugin
                .lock()
                .map_err(|e| format!("Plugin lock poisoned: {e}"))?
                .get_parameter(*id)
                .map_err(|e| format!("Failed to get parameter: {e}"))?;
            Ok(Some(ResponseMessage::ParameterValue { id: *id, value }))
        }

        ControlMessage::GetParameters => {
            let params = plugin
                .lock()
                .map_err(|e| format!("Plugin lock poisoned: {e}"))?
                .parameters();

            // Convert to serializable format
            let parameters: Vec<_> = params.into_iter().map(Into::into).collect();
            Ok(Some(ResponseMessage::Parameters { parameters }))
        }
    }
}

/// Audio processing loop running in separate thread
///
/// # Stack Size Requirements
///
/// This function allocates approximately 64KB of buffers on the stack:
/// - Input buffers: 2 channels × 8192 frames × 4 bytes = 64KB
/// - Output buffers: 2 channels × 8192 frames × 4 bytes = 64KB
///
/// **Total stack usage: ~128KB minimum**
///
/// Default thread stack size on most platforms (2MB) is sufficient.
/// If spawning this thread manually with `std::thread::Builder`, ensure
/// stack size is at least 256KB to account for other function frames.
fn audio_processing_loop(
    plugin: &Arc<Mutex<vvdaw_vst3::Vst3Plugin>>,
    shared_buffer_ptr: &SendSharedBuffer,
) {
    use std::sync::atomic::Ordering;
    use vvdaw_plugin::{AudioBuffer, EventBuffer};

    const MAX_CHANNELS: usize = 2;
    const MAX_FRAMES: usize = 8192;
    const MAX_EVENTS: usize = 1024; // Must match vvdaw_vst3::ipc::MAX_EVENTS

    // Safety: shared_buffer_ptr is valid as long as the main thread keeps _shared_memory alive
    #[allow(unsafe_code)]
    let shared_buffer = unsafe { &mut *(shared_buffer_ptr.0.cast_mut()) };

    // Pre-allocate buffers to avoid allocations in audio callback (real-time safety)
    // We intentionally use stack allocation here for performance (avoid heap fragmentation)
    #[allow(clippy::large_stack_arrays)]
    let mut input_buffers: [[f32; MAX_FRAMES]; MAX_CHANNELS] = [[0.0; MAX_FRAMES]; MAX_CHANNELS];
    #[allow(clippy::large_stack_arrays)]
    let mut output_buffers: [[f32; MAX_FRAMES]; MAX_CHANNELS] = [[0.0; MAX_FRAMES]; MAX_CHANNELS];

    // Pre-allocate event buffer with capacity to avoid allocations in loop
    let mut event_buffer = EventBuffer::new();
    event_buffer.events.reserve(MAX_EVENTS);

    loop {
        // Wait for Process signal (1 second timeout)
        if !shared_buffer.wait_for_state(ProcessState::Process, 1_000_000) {
            // Check if shutdown was requested during timeout
            let current_state = ProcessState::from_u32(shared_buffer.state.load(Ordering::Acquire));
            if current_state == ProcessState::Shutdown {
                return; // Exit audio thread gracefully
            }
            continue; // Timeout, check again
        }

        // Get frame count and validate bounds (prevent malicious/buggy subprocess data)
        let frame_count = shared_buffer.frame_count.load(Ordering::Acquire) as usize;
        if frame_count > MAX_FRAMES {
            eprintln!("Invalid frame_count from shared memory: {frame_count} > {MAX_FRAMES}");
            shared_buffer.set_state(ProcessState::Crashed);
            return;
        }

        // Read events from shared memory and validate bounds
        let event_count = shared_buffer.event_count.load(Ordering::Acquire) as usize;
        if event_count > MAX_EVENTS {
            eprintln!("Invalid event_count from shared memory: {event_count} > {MAX_EVENTS}");
            shared_buffer.set_state(ProcessState::Crashed);
            return;
        }

        // Reuse pre-allocated event buffer (clear from previous cycle)
        event_buffer.events.clear();
        for i in 0..event_count {
            event_buffer.events.push(shared_buffer.events[i].into());
        }

        // Copy input data from shared memory to pre-allocated buffers
        for (ch, buf) in input_buffers.iter_mut().enumerate() {
            buf[..frame_count].copy_from_slice(&shared_buffer.inputs[ch][..frame_count]);
        }

        // Create slices from pre-allocated buffers using fixed-size arrays (no allocation)
        let input_refs: [&[f32]; MAX_CHANNELS] = [
            &input_buffers[0][..frame_count],
            &input_buffers[1][..frame_count],
        ];

        // Split output_buffers to get non-overlapping mutable slices
        let (out0, out1) = output_buffers.split_at_mut(1);
        let mut output_refs: [&mut [f32]; MAX_CHANNELS] =
            [&mut out0[0][..frame_count], &mut out1[0][..frame_count]];

        let mut audio = AudioBuffer {
            inputs: &input_refs,
            outputs: &mut output_refs,
            frames: frame_count,
        };

        // Process through plugin (use try_lock to avoid blocking in audio thread)
        let result = match plugin.try_lock() {
            Ok(mut plugin) => plugin.process(&mut audio, &event_buffer),
            Err(std::sync::TryLockError::Poisoned(e)) => {
                eprintln!("Plugin lock poisoned: {e}");
                shared_buffer.set_state(ProcessState::Crashed);
                return;
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                // Lock is held by control thread - this is an underrun
                // Output silence and continue (real-time constraint: never block)
                eprintln!("Audio thread: mutex contention, outputting silence");
                for buf in &mut output_buffers {
                    buf[..frame_count].fill(0.0);
                }
                Ok(())
            }
        };

        if let Err(e) = result {
            eprintln!("Plugin processing failed: {e}");
            shared_buffer.set_state(ProcessState::Crashed);
            return;
        }

        // Write outputs back to shared memory
        for (ch, buf) in output_buffers.iter().enumerate() {
            shared_buffer.outputs[ch][..frame_count].copy_from_slice(&buf[..frame_count]);
        }

        // Signal completion
        shared_buffer.set_state(ProcessState::Done);
    }
}
