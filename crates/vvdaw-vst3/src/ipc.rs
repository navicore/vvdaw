//! Inter-process communication for multi-process plugin hosting.
//!
//! This module defines the IPC protocol for running VST3 plugins in separate processes.
//! This provides crash isolation, memory isolation, and allows plugins to run in parallel.
//!
//! # Architecture
//!
//! - **Shared Memory**: Audio buffers are mapped between processes for zero-copy transfer
//! - **Lock-Free Ring Buffers**: Events and MIDI use `rtrb` for real-time safe communication
//! - **Atomic Flags**: Process synchronization and crash detection
//!
//! # Process Flow
//!
//! 1. Main process spawns plugin-host subprocess
//! 2. Subprocess loads plugin and signals ready
//! 3. Main process sends Init command with shared memory details
//! 4. For each audio callback:
//!    - Main writes input buffers to shared memory
//!    - Main writes events to ring buffer
//!    - Main sets PROCESS atomic flag
//!    - Plugin processes audio
//!    - Plugin sets DONE atomic flag
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
        /// File descriptor or handle for shared memory region
        #[serde(skip)]
        shm_fd: i32,
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

    /// Error occurred
    Error { message: String },
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
}

impl ProcessState {
    #[allow(clippy::match_same_arms)]
    pub fn from_u32(val: u32) -> Self {
        match val {
            0 => Self::Idle,
            1 => Self::Process,
            2 => Self::Done,
            3 => Self::Crashed,
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
    /// Create a new shared buffer (called by main process)
    #[allow(clippy::large_stack_arrays)]
    pub fn new() -> Self {
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
    /// Returns true if state reached, false if timeout
    pub fn wait_for_state(&self, target: ProcessState, timeout_us: u64) -> bool {
        let start = std::time::Instant::now();
        while self.get_state() != target {
            if start.elapsed().as_micros() > u128::from(timeout_us) {
                return false;
            }
            std::hint::spin_loop();
        }
        true
    }
}

impl Default for SharedAudioBuffer {
    fn default() -> Self {
        Self::new()
    }
}

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
        let buffer = SharedAudioBuffer::new();
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
