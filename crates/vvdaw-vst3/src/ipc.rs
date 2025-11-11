//! Inter-process communication for multi-process plugin hosting.
//!
//! This module defines the IPC protocol for running VST3 plugins in separate processes.
//! This provides crash isolation, memory isolation, and allows plugins to run in parallel.
//!
//! # Architecture
//!
//! ## Two Communication Channels
//!
//! ### 1. Control Plane (JSON over stdin/stdout)
//!
//! Used for **infrequent, non-real-time operations**:
//! - Plugin initialization (`Init`, `Activate`, `Deactivate`)
//! - Configuration queries (`GetParameter`)
//! - One-off parameter changes from UI (e.g., user clicks, preset load)
//!
//! **Characteristics**:
//! - Low frequency (< 10 Hz)
//! - High latency tolerance (milliseconds OK)
//! - Human-readable, debuggable format
//! - Serialization overhead acceptable (~1-10µs)
//!
//! ### 2. Data Plane (Shared Memory)
//!
//! Used for **high-frequency, real-time operations**:
//! - Audio buffer transfer (zero-copy)
//! - Sample-accurate parameter automation via `Event::ParamChange`
//! - MIDI events (`Event::NoteOn`, `Event::NoteOff`)
//!
//! **Characteristics**:
//! - High frequency (audio rate, ~100 Hz - 1 kHz)
//! - Zero latency tolerance (microseconds matter)
//! - Binary format, direct memory access
//! - No serialization overhead
//!
//! ## Why Not JSON for Everything?
//!
//! Parameter automation needs **sample-accurate timing** that JSON can't provide:
//! - JSON over stdin requires parsing (non-deterministic latency)
//! - Can't specify exact sample offset for parameter changes
//! - Would add ~1-10µs overhead per change (unacceptable in audio callback)
//!
//! Instead, automation goes through `SharedAudioBuffer::events` as `Event::ParamChange`,
//! which specifies the exact sample within the buffer where the change occurs.
//!
//! # Process Flow
//!
//! 1. Main process spawns plugin-host subprocess
//! 2. Subprocess loads plugin and signals ready via JSON
//! 3. Main process sends `Init` command (JSON) with shared memory details
//! 4. For each audio callback:
//!    - Main writes input buffers to shared memory
//!    - Main writes events (MIDI, automation) to shared memory event buffer
//!    - Main sets `PROCESS` atomic flag
//!    - Plugin processes audio
//!    - Plugin sets `DONE` atomic flag
//!    - Main reads output buffers from shared memory

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use vvdaw_core::{Frames, SampleRate};
use vvdaw_plugin::PluginInfo;

/// Maximum number of audio channels supported (stereo for MVP)
pub const MAX_CHANNELS: usize = 2;

/// Maximum number of events per processing block
pub const MAX_EVENTS: usize = 1024;

/// Control messages sent from main process to plugin subprocess
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Initialize the plugin with sample rate and buffer size
    Init {
        sample_rate: SampleRate,
        max_block_size: Frames,
    },

    /// Activate the plugin (prepare for processing)
    Activate,

    /// Deactivate the plugin (stop processing)
    Deactivate,

    /// Shutdown the subprocess gracefully
    Shutdown,

    /// Set a parameter value
    SetParameter { id: u32, value: f32 },

    /// Get current parameter value
    GetParameter { id: u32 },

    /// Get all available parameters
    GetParameters,
}

/// Response messages sent from plugin subprocess to main process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseMessage {
    /// Plugin loaded successfully
    Ready { info: PluginInfo },

    /// Plugin initialized successfully
    Initialized,

    /// Plugin activated successfully
    Activated,

    /// Plugin deactivated successfully
    Deactivated,

    /// Parameter value response
    ParameterValue { id: u32, value: f32 },

    /// All parameters response
    Parameters {
        parameters: Vec<SerializableParameterInfo>,
    },

    /// Error occurred
    Error { message: String },
}

/// Serializable version of `ParameterInfo` for IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableParameterInfo {
    pub id: u32,
    pub name: String,
    pub min_value: f32,
    pub max_value: f32,
    pub default_value: f32,
}

impl From<vvdaw_plugin::ParameterInfo> for SerializableParameterInfo {
    fn from(info: vvdaw_plugin::ParameterInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            min_value: info.min_value,
            max_value: info.max_value,
            default_value: info.default_value,
        }
    }
}

impl From<SerializableParameterInfo> for vvdaw_plugin::ParameterInfo {
    fn from(info: SerializableParameterInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            min_value: info.min_value,
            max_value: info.max_value,
            default_value: info.default_value,
        }
    }
}

/// Atomic state flags for process synchronization
///
/// These are stored in shared memory and accessed by both processes.
/// Using u32 for wider platform compatibility.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Waiting for work
    Idle = 0,

    /// Main process has written buffers, plugin should process
    Process = 1,

    /// Plugin has finished processing, main can read buffers
    Done = 2,

    /// Plugin subprocess has crashed
    Crashed = 3,

    /// Main process requests audio thread to shutdown gracefully
    Shutdown = 4,
}

impl ProcessState {
    #[allow(clippy::match_same_arms)]
    pub fn from_u32(val: u32) -> Self {
        match val {
            0 => Self::Idle,
            1 => Self::Process,
            2 => Self::Done,
            3 => Self::Crashed,
            4 => Self::Shutdown,
            _ => Self::Idle, // Default to Idle for unknown values
        }
    }
}

/// Shared memory layout for audio processing
///
/// This structure is mapped into shared memory and accessed by both processes.
/// Layout is designed to be cache-friendly and minimize false sharing.
#[repr(C)]
pub struct SharedAudioBuffer {
    /// Atomic state flag for synchronization
    pub state: AtomicU32,

    /// Number of frames to process in this cycle
    pub frame_count: AtomicU32,

    /// Input audio buffers [channel][frame]
    /// Using fixed size for simplicity in shared memory
    pub inputs: [[f32; 8192]; MAX_CHANNELS],

    /// Output audio buffers [channel][frame]
    pub outputs: [[f32; 8192]; MAX_CHANNELS],

    /// Event count for this cycle
    pub event_count: AtomicU32,

    /// Events for this cycle (simple fixed-size buffer)
    /// In a full implementation, we'd use a lock-free ring buffer
    pub events: [Event; MAX_EVENTS],
}

impl SharedAudioBuffer {
    /// Create a new shared buffer on the heap (safe alternative)
    ///
    /// This allocates the buffer in a `Box` to avoid stack overflow.
    /// Use this when you need a standalone buffer for testing or other non-shared-memory use cases.
    ///
    /// For shared memory placement, use [`new_in_place()`](Self::new_in_place) instead.
    #[allow(unsafe_code)]
    #[allow(clippy::large_stack_arrays)] // We're using Box::new_uninit, so this is heap-allocated
    pub fn boxed() -> Box<Self> {
        // Box::new would use stack temporarily, so we use Box::new_uninit
        // and initialize in-place to allocate directly on the heap
        unsafe {
            let mut uninit = Box::<Self>::new_uninit();
            let ptr = uninit.as_mut_ptr();

            // Write fields directly to uninitialized memory (no stack allocation)
            std::ptr::addr_of_mut!((*ptr).state).write(AtomicU32::new(ProcessState::Idle as u32));
            std::ptr::addr_of_mut!((*ptr).frame_count).write(AtomicU32::new(0));
            std::ptr::addr_of_mut!((*ptr).inputs).write([[0.0; 8192]; MAX_CHANNELS]);
            std::ptr::addr_of_mut!((*ptr).outputs).write([[0.0; 8192]; MAX_CHANNELS]);
            std::ptr::addr_of_mut!((*ptr).event_count).write(AtomicU32::new(0));
            std::ptr::addr_of_mut!((*ptr).events).write(
                [Event::NoteOff {
                    channel: 0,
                    note: 0,
                    sample_offset: 0,
                }; MAX_EVENTS],
            );

            uninit.assume_init()
        }
    }

    /// Create a new shared buffer in-place (for shared memory)
    ///
    /// # Safety
    ///
    /// This allocates ~76KB on the stack:
    /// - Input buffers: 32KB per channel × 2 channels
    /// - Output buffers: 32KB per channel × 2 channels
    /// - Event buffer: ~12KB
    ///
    /// **This is marked unsafe because it can easily cause stack overflow if called directly.**
    ///
    /// Only use this when writing directly into shared memory (not on stack):
    /// ```rust,ignore
    /// let shm = SharedMemory::create("/name", std::mem::size_of::<SharedAudioBuffer>())?;
    /// let buffer = unsafe { shm.as_mut::<SharedAudioBuffer>() };
    /// *buffer = unsafe { SharedAudioBuffer::new_in_place() };
    /// ```
    ///
    /// For normal usage, use [`boxed()`](Self::boxed) instead.
    #[allow(unsafe_code)]
    #[allow(clippy::large_stack_arrays)]
    pub unsafe fn new_in_place() -> Self {
        Self {
            state: AtomicU32::new(ProcessState::Idle as u32),
            frame_count: AtomicU32::new(0),
            inputs: [[0.0; 8192]; MAX_CHANNELS],
            outputs: [[0.0; 8192]; MAX_CHANNELS],
            event_count: AtomicU32::new(0),
            events: [Event::NoteOff {
                channel: 0,
                note: 0,
                sample_offset: 0,
            }; MAX_EVENTS],
        }
    }

    /// Get current process state
    pub fn get_state(&self) -> ProcessState {
        ProcessState::from_u32(self.state.load(Ordering::Acquire))
    }

    /// Set process state
    pub fn set_state(&self, state: ProcessState) {
        self.state.store(state as u32, Ordering::Release);
    }

    /// Wait for a specific state with timeout
    ///
    /// Uses hybrid spin-then-sleep strategy:
    /// - Spins for <100µs (real-time critical, avoid context switch)
    /// - Yields with exponential backoff for longer waits (avoid burning CPU)
    ///
    /// Returns `true` if state reached, `false` if timeout
    pub fn wait_for_state(&self, target: ProcessState, timeout_us: u64) -> bool {
        let start = std::time::Instant::now();
        let mut sleep_us = 1; // Start with 1µs sleep

        while self.get_state() != target {
            let elapsed_us = start.elapsed().as_micros();

            if elapsed_us > u128::from(timeout_us) {
                return false;
            }

            // Spin loop for the first 100µs (real-time critical)
            if elapsed_us < 100 {
                std::hint::spin_loop();
            } else {
                // After 100µs, yield to avoid burning CPU
                // Exponential backoff: 1µs, 2µs, 4µs, ..., up to 100µs
                std::thread::sleep(std::time::Duration::from_micros(sleep_us));
                sleep_us = (sleep_us * 2).min(100);
            }
        }
        true
    }
}

// Note: No Default impl - use SharedAudioBuffer::boxed() instead to avoid stack overflow

/// Event types that can be sent between processes
///
/// This is a simplified version of `vvdaw_plugin::Event` that is `Copy`
/// so it can be stored in shared memory without complex synchronization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Event {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: f32,
        sample_offset: u32,
    },
    NoteOff {
        channel: u8,
        note: u8,
        sample_offset: u32,
    },
    ParamChange {
        id: u32,
        value: f32,
        sample_offset: u32,
    },
}

impl From<vvdaw_plugin::Event> for Event {
    fn from(event: vvdaw_plugin::Event) -> Self {
        match event {
            vvdaw_plugin::Event::NoteOn {
                channel,
                note,
                velocity,
                sample_offset,
            } => Self::NoteOn {
                channel,
                note,
                velocity,
                sample_offset,
            },
            vvdaw_plugin::Event::NoteOff {
                channel,
                note,
                sample_offset,
            } => Self::NoteOff {
                channel,
                note,
                sample_offset,
            },
            vvdaw_plugin::Event::ParamChange {
                id,
                value,
                sample_offset,
            } => Self::ParamChange {
                id,
                value,
                sample_offset,
            },
        }
    }
}

impl From<Event> for vvdaw_plugin::Event {
    fn from(event: Event) -> Self {
        match event {
            Event::NoteOn {
                channel,
                note,
                velocity,
                sample_offset,
            } => Self::NoteOn {
                channel,
                note,
                velocity,
                sample_offset,
            },
            Event::NoteOff {
                channel,
                note,
                sample_offset,
            } => Self::NoteOff {
                channel,
                note,
                sample_offset,
            },
            Event::ParamChange {
                id,
                value,
                sample_offset,
            } => Self::ParamChange {
                id,
                value,
                sample_offset,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_state_conversion() {
        assert_eq!(ProcessState::from_u32(0), ProcessState::Idle);
        assert_eq!(ProcessState::from_u32(1), ProcessState::Process);
        assert_eq!(ProcessState::from_u32(2), ProcessState::Done);
        assert_eq!(ProcessState::from_u32(3), ProcessState::Crashed);
        assert_eq!(ProcessState::from_u32(999), ProcessState::Idle); // Unknown defaults to Idle
    }

    #[test]
    fn test_shared_buffer_state() {
        let buffer = SharedAudioBuffer::boxed();
        assert_eq!(buffer.get_state(), ProcessState::Idle);

        buffer.set_state(ProcessState::Process);
        assert_eq!(buffer.get_state(), ProcessState::Process);

        buffer.set_state(ProcessState::Done);
        assert_eq!(buffer.get_state(), ProcessState::Done);
    }

    #[test]
    fn test_event_conversion() {
        let plugin_event = vvdaw_plugin::Event::NoteOn {
            channel: 1,
            note: 60,
            velocity: 0.8,
            sample_offset: 100,
        };

        let ipc_event: Event = plugin_event.into();
        let back_to_plugin: vvdaw_plugin::Event = ipc_event.into();

        match back_to_plugin {
            vvdaw_plugin::Event::NoteOn {
                channel,
                note,
                velocity,
                sample_offset,
            } => {
                assert_eq!(channel, 1);
                assert_eq!(note, 60);
                assert!((velocity - 0.8).abs() < 0.001);
                assert_eq!(sample_offset, 100);
            }
            _ => panic!("Event conversion failed"),
        }
    }
}
