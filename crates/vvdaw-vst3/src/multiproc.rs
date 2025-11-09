//! Multi-process plugin wrapper.
//!
//! This module implements `MultiProcessPlugin`, which runs VST3 plugins in
//! separate processes for crash isolation and parallel execution.
//!
//! # Architecture
//!
//! - Main process spawns `plugin-host` subprocess
//! - Control messages (Init, SetParameter) sent via JSON over stdin/stdout
//! - Audio processing uses shared memory for zero-copy transfer
//! - Crash detection via process monitoring
//!
//! # Usage
//!
//! ```rust,ignore
//! let plugin = MultiProcessPlugin::spawn("/path/to/plugin.vst3")?;
//! plugin.initialize(48000, 512)?;
//! plugin.process(&mut audio_buffer, &event_buffer)?;
//! ```

#![allow(clippy::doc_markdown)] // Allow technical terms without backticks in module docs

use crate::{ControlMessage, ResponseMessage, SharedAudioBuffer, SharedMemory};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use vvdaw_core::{ChannelCount, Frames, SampleRate};
use vvdaw_plugin::{AudioBuffer, EventBuffer, ParameterInfo, Plugin, PluginError, PluginInfo};

/// Maximum time to wait for subprocess to respond (in milliseconds)
#[allow(dead_code)] // Will be used for timeout implementation
const SUBPROCESS_TIMEOUT_MS: u64 = 5000;

/// Maximum time to wait for audio processing (in microseconds)
#[allow(dead_code)] // Will be used for audio processing implementation
const AUDIO_PROCESSING_TIMEOUT_US: u64 = 10_000; // 10ms

/// Global instance counter for generating unique shared memory names
///
/// This ensures that multiple plugin instances in the same process
/// get unique shared memory regions instead of colliding.
static INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Send-safe wrapper for shared audio buffer pointer
///
/// Safety: The `SharedAudioBuffer` is in shared memory and is explicitly designed
/// to be accessed from multiple processes. The pointer is valid as long as
/// the `SharedMemory` it points to is alive.
struct SendSharedBuffer(*const SharedAudioBuffer);

#[allow(unsafe_code)]
unsafe impl Send for SendSharedBuffer {}

impl SendSharedBuffer {
    fn as_mut_ptr(&self) -> *mut SharedAudioBuffer {
        self.0.cast_mut()
    }
}

/// Multi-process plugin wrapper
///
/// Runs a VST3 plugin in a separate process for crash isolation.
pub struct MultiProcessPlugin {
    /// Plugin metadata
    info: PluginInfo,

    /// Subprocess handle
    child_process: Child,

    /// Stdin pipe to subprocess
    stdin: ChildStdin,

    /// Stdout pipe from subprocess
    stdout: BufReader<ChildStdout>,

    /// Shared memory region
    _shared_memory: SharedMemory,

    /// Pointer to shared audio buffer (mutable access to shared memory)
    #[allow(dead_code)] // Will be used for audio processing
    shared_buffer: SendSharedBuffer,

    /// Plugin path (for crash recovery)
    #[allow(dead_code)] // Will be used for crash recovery
    plugin_path: String,

    /// Shared memory name
    #[allow(dead_code)] // Will be used for crash recovery
    shm_name: String,

    /// Whether plugin is initialized
    initialized: bool,

    /// Current sample rate
    sample_rate: SampleRate,

    /// Maximum block size
    max_block_size: Frames,

    /// Input channel count
    input_channels: ChannelCount,

    /// Output channel count
    output_channels: ChannelCount,
}

impl MultiProcessPlugin {
    /// Spawn a new plugin in a subprocess
    ///
    /// This creates the subprocess, sets up shared memory, and waits for
    /// the plugin to signal it's ready.
    pub fn spawn(plugin_path: impl AsRef<std::path::Path>) -> Result<Self, PluginError> {
        let plugin_path = plugin_path.as_ref().to_string_lossy().to_string();

        // Generate unique shared memory name using PID + counter + timestamp
        // This prevents collisions even if process restarts and reuses PID
        let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let shm_name = format!(
            "/vvdaw_shm_{}_{}_{:x}",
            std::process::id(),
            instance_id,
            timestamp
        );

        // Create shared memory
        let mut shared_memory =
            SharedMemory::create(&shm_name, std::mem::size_of::<SharedAudioBuffer>())?;

        // Initialize the shared buffer
        // Safety: We just created this shared memory with the correct size
        #[allow(unsafe_code)]
        unsafe {
            let buffer_ptr = shared_memory.as_mut::<SharedAudioBuffer>();
            *buffer_ptr = SharedAudioBuffer::new_in_place();
        }

        // Get pointer to shared buffer
        // Safety: We just initialized it above, and it lives as long as shared_memory
        #[allow(unsafe_code)]
        let shared_buffer = SendSharedBuffer(unsafe {
            std::ptr::from_ref(shared_memory.as_ref::<SharedAudioBuffer>())
        });

        // Find plugin-host binary
        let plugin_host_path = std::env::current_exe()
            .map_err(|e| {
                PluginError::InitializationFailed(format!("Failed to get current exe: {e}"))
            })?
            .parent()
            .ok_or_else(|| {
                PluginError::InitializationFailed("No parent directory for exe".to_string())
            })?
            .join("plugin-host");

        // Spawn subprocess
        let mut child = Command::new(&plugin_host_path)
            .arg(&plugin_path)
            .arg(&shm_name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                PluginError::InitializationFailed(format!("Failed to spawn plugin-host: {e}"))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PluginError::InitializationFailed("Failed to get stdin".to_string()))?;

        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| {
            PluginError::InitializationFailed("Failed to get stdout".to_string())
        })?);

        let mut plugin = Self {
            info: PluginInfo {
                name: String::new(),
                vendor: String::new(),
                version: String::new(),
                unique_id: String::new(),
            },
            child_process: child,
            stdin,
            stdout,
            _shared_memory: shared_memory,
            shared_buffer,
            plugin_path,
            shm_name,
            initialized: false,
            sample_rate: 0,
            max_block_size: 0,
            input_channels: 0,
            output_channels: 0,
        };

        // Wait for Ready message
        match plugin.wait_for_response()? {
            ResponseMessage::Ready { info } => {
                plugin.info = info;
                // TODO: Get channel counts from plugin info
                plugin.input_channels = 2;
                plugin.output_channels = 2;
                Ok(plugin)
            }
            ResponseMessage::Error { message } => Err(PluginError::InitializationFailed(format!(
                "Plugin initialization failed: {message}"
            ))),
            _ => Err(PluginError::InitializationFailed(
                "Unexpected response from plugin".to_string(),
            )),
        }
    }

    /// Send a control message to the subprocess
    fn send_message(&mut self, message: &ControlMessage) -> Result<(), PluginError> {
        let json = serde_json::to_string(message).map_err(|e| {
            PluginError::ProcessingFailed(format!("Failed to serialize message: {e}"))
        })?;

        writeln!(self.stdin, "{json}")
            .map_err(|e| PluginError::ProcessingFailed(format!("Failed to write to stdin: {e}")))?;

        self.stdin
            .flush()
            .map_err(|e| PluginError::ProcessingFailed(format!("Failed to flush stdin: {e}")))?;

        Ok(())
    }

    /// Wait for a response message from the subprocess with timeout
    ///
    /// Uses platform-specific mechanisms to implement read timeouts:
    /// - Unix: setsockopt SO_RCVTIMEO on the file descriptor
    /// - Other platforms: Falls back to process monitoring
    fn wait_for_response(&mut self) -> Result<ResponseMessage, PluginError> {
        // Check if subprocess is alive before attempting to read
        if !self.is_alive() {
            return Err(PluginError::ProcessingFailed(
                "Subprocess has died".to_string(),
            ));
        }

        // Set read timeout on Unix platforms
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;

            let fd = self.stdout.get_ref().as_raw_fd();
            let timeout = libc::timeval {
                tv_sec: (SUBPROCESS_TIMEOUT_MS / 1000) as libc::time_t,
                tv_usec: ((SUBPROCESS_TIMEOUT_MS % 1000) * 1000) as libc::suseconds_t,
            };

            #[allow(unsafe_code)]
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVTIMEO,
                    std::ptr::from_ref(&timeout).cast(),
                    std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                );
            }
        }

        let mut line = String::new();

        self.stdout.read_line(&mut line).map_err(|e| {
            // Check if timeout occurred
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut
            {
                PluginError::ProcessingFailed(format!(
                    "Subprocess response timeout after {SUBPROCESS_TIMEOUT_MS}ms"
                ))
            } else {
                PluginError::ProcessingFailed(format!("Failed to read from stdout: {e}"))
            }
        })?;

        if line.is_empty() {
            return Err(PluginError::ProcessingFailed(
                "Subprocess closed stdout".to_string(),
            ));
        }

        serde_json::from_str(&line)
            .map_err(|e| PluginError::ProcessingFailed(format!("Failed to parse response: {e}")))
    }

    /// Check if subprocess is still alive
    fn is_alive(&mut self) -> bool {
        self.child_process.try_wait().ok().flatten().is_none()
    }
}

impl Plugin for MultiProcessPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn initialize(
        &mut self,
        sample_rate: SampleRate,
        max_block_size: Frames,
    ) -> Result<(), PluginError> {
        if !self.is_alive() {
            return Err(PluginError::InitializationFailed(
                "Subprocess has died".to_string(),
            ));
        }

        self.send_message(&ControlMessage::Init {
            sample_rate,
            max_block_size,
        })?;

        match self.wait_for_response()? {
            ResponseMessage::Initialized => {
                self.initialized = true;
                self.sample_rate = sample_rate;
                self.max_block_size = max_block_size;
                Ok(())
            }
            ResponseMessage::Error { message } => Err(PluginError::InitializationFailed(message)),
            _ => Err(PluginError::InitializationFailed(
                "Unexpected response".to_string(),
            )),
        }
    }

    fn process(
        &mut self,
        audio: &mut AudioBuffer,
        events: &EventBuffer,
    ) -> Result<(), PluginError> {
        if !self.initialized {
            return Err(PluginError::ProcessingFailed(
                "Plugin not initialized".to_string(),
            ));
        }

        if !self.is_alive() {
            return Err(PluginError::ProcessingFailed(
                "Subprocess has crashed".to_string(),
            ));
        }

        let frames = audio.frames;

        // Ensure we don't exceed buffer capacity
        if frames > 8192 {
            return Err(PluginError::ProcessingFailed(format!(
                "Frame count {frames} exceeds maximum buffer size 8192"
            )));
        }

        // Safety: shared_buffer is valid as long as _shared_memory is alive
        // We use raw pointers because SharedMemory needs interior mutability
        #[allow(unsafe_code)]
        unsafe {
            let buffer = &mut *(self.shared_buffer.as_mut_ptr());

            // 1. Copy input buffers to shared memory
            for (ch, input) in audio.inputs.iter().enumerate() {
                if ch >= crate::ipc::MAX_CHANNELS {
                    break;
                }
                buffer.inputs[ch][..frames].copy_from_slice(&input[..frames]);
            }

            // 2. Copy events to shared memory
            let event_count = events.events.len().min(crate::ipc::MAX_EVENTS);
            for (i, event) in events.events.iter().take(event_count).enumerate() {
                buffer.events[i] = event.clone().into();
            }
            buffer
                .event_count
                .store(event_count as u32, std::sync::atomic::Ordering::Release);

            // 3. Set frame count
            buffer
                .frame_count
                .store(frames as u32, std::sync::atomic::Ordering::Release);

            // 4. Signal subprocess to process
            buffer.set_state(crate::ProcessState::Process);

            // 5. Wait for processing to complete
            if !buffer.wait_for_state(crate::ProcessState::Done, AUDIO_PROCESSING_TIMEOUT_US) {
                // Timeout - check if subprocess crashed instead of timing out
                let current_state = crate::ProcessState::from_u32(
                    buffer.state.load(std::sync::atomic::Ordering::Acquire),
                );
                if current_state == crate::ProcessState::Crashed {
                    return Err(PluginError::ProcessingFailed(
                        "Subprocess crashed during processing".to_string(),
                    ));
                }
                if !self.is_alive() {
                    return Err(PluginError::ProcessingFailed(
                        "Subprocess died during processing".to_string(),
                    ));
                }
                return Err(PluginError::ProcessingFailed(
                    "Audio processing timeout".to_string(),
                ));
            }

            // Verify we actually got Done state (not Crashed or other state)
            let final_state = crate::ProcessState::from_u32(
                buffer.state.load(std::sync::atomic::Ordering::Acquire),
            );
            if final_state != crate::ProcessState::Done {
                return Err(PluginError::ProcessingFailed(format!(
                    "Unexpected state after processing: {final_state:?}"
                )));
            }

            // 6. Copy output buffers from shared memory
            for (ch, output) in audio.outputs.iter_mut().enumerate() {
                if ch >= crate::ipc::MAX_CHANNELS {
                    break;
                }
                output[..frames].copy_from_slice(&buffer.outputs[ch][..frames]);
            }

            // Reset state for next cycle
            buffer.set_state(crate::ProcessState::Idle);
        }

        Ok(())
    }

    fn set_parameter(&mut self, id: u32, value: f32) -> Result<(), PluginError> {
        if !self.is_alive() {
            return Err(PluginError::ProcessingFailed(
                "Subprocess has died".to_string(),
            ));
        }

        self.send_message(&ControlMessage::SetParameter { id, value })?;

        // SetParameter doesn't send a response for performance
        Ok(())
    }

    fn get_parameter(&self, id: u32) -> Result<f32, PluginError> {
        // This is tricky - we need mutable access to send/receive
        // For now, return an error
        Err(PluginError::InvalidParameter(format!(
            "get_parameter not yet implemented for multi-process plugins: {id}"
        )))
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        // TODO: Get parameters from subprocess
        Vec::new()
    }

    fn input_channels(&self) -> ChannelCount {
        self.input_channels
    }

    fn output_channels(&self) -> ChannelCount {
        self.output_channels
    }

    fn deactivate(&mut self) {
        if !self.is_alive() {
            return; // Already dead
        }

        // Signal audio thread to shutdown via shared memory
        // Safety: shared_buffer is valid as long as _shared_memory is alive
        #[allow(unsafe_code)]
        unsafe {
            let buffer = &mut *(self.shared_buffer.as_mut_ptr());
            buffer.set_state(crate::ProcessState::Shutdown);
        }

        // Best effort shutdown via control message
        let _ = self.send_message(&ControlMessage::Shutdown);

        // Wait for subprocess to exit gracefully with timeout
        // This prevents use-after-free by ensuring subprocess stops using shared memory
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(500);

        while self.is_alive() && start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(10));
        }

        // Force kill if still alive after timeout
        if self.is_alive() {
            let _ = self.child_process.kill();
            // Wait for kill to complete (non-blocking check loop)
            let kill_start = std::time::Instant::now();
            while self.is_alive() && kill_start.elapsed() < Duration::from_millis(100) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

impl Drop for MultiProcessPlugin {
    fn drop(&mut self) {
        self.deactivate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_nonexistent() {
        let result = MultiProcessPlugin::spawn("/nonexistent.vst3");
        assert!(result.is_err());
    }
}
