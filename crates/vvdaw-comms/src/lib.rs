//! Lockless communication primitives for audio-thread-safe communication.
//!
//! This crate provides wrappers and utilities for passing data between
//! the real-time audio thread and the UI/control thread without blocking.

pub use rtrb;
pub use triple_buffer;

use crossbeam_channel::{Receiver, Sender};
use vvdaw_core::Sample;

/// Commands that can be sent from UI thread to audio thread
///
/// IMPORTANT: All variants must be real-time safe (no heap allocation/deallocation).
/// For complex data (like plugin instances), use the separate `plugin_tx` channel.
#[derive(Debug, Clone)]
pub enum AudioCommand {
    /// Start audio processing
    Start,
    /// Stop audio processing
    Stop,
    /// Set a parameter value (`node_id`, `param_id`, value)
    SetParameter(usize, u32, f32),
    /// Add a node to the graph
    AddNode,
    /// Remove a node from the graph
    RemoveNode(usize),
    /// Connect two nodes
    Connect {
        /// Source node ID
        from: usize,
        /// Destination node ID
        to: usize,
    },
    /// Disconnect two nodes
    Disconnect {
        /// Source node ID
        from: usize,
        /// Destination node ID
        to: usize,
    },
}

/// Events sent from audio thread back to UI thread
#[derive(Debug, Clone)]
pub enum AudioEvent {
    /// Audio processing started
    Started,
    /// Audio processing stopped
    Stopped,
    /// Audio engine initialized with actual device sample rate
    ///
    /// Sent once when the audio engine successfully starts and configures
    /// the audio device. Reports the actual sample rate the engine is using,
    /// which may differ from the requested rate if the device doesn't support it.
    ///
    /// UI should use this rate for resampling imported audio files.
    EngineInitialized {
        /// Actual sample rate the audio engine is running at (e.g., 44100, 48000)
        sample_rate: u32,
    },
    /// Error occurred
    Error(String),
    /// Peak level update (for meters, visualization)
    PeakLevel {
        /// Channel number
        channel: usize,
        /// Peak level value
        level: Sample,
    },
    /// Node was added to the graph
    ///
    /// Sent after `AddNode` command succeeds, reports the assigned node ID.
    /// This allows the UI to track node IDs for later removal/modification.
    NodeAdded {
        /// The ID assigned to the newly added node
        node_id: usize,
    },
    /// Node was removed from the graph
    ///
    /// Sent after `RemoveNode` command succeeds.
    /// This confirms the node has been removed from the audio graph.
    NodeRemoved {
        /// The ID of the removed node
        node_id: usize,
    },
    /// Waveform sample data for visualization
    ///
    /// Sent from audio thread with peak values for the current audio buffer.
    /// Position indicates the frame number in the audio stream for sync.
    WaveformSample {
        /// Frame position in the audio stream (accumulates continuously)
        position: u64,
        /// Left channel peak value for this buffer
        left_peak: Sample,
        /// Right channel peak value for this buffer
        right_peak: Sample,
    },
}

/// Type alias for command channel (UI -> Audio)
pub type CommandSender = rtrb::Producer<AudioCommand>;
/// Command receiver (audio thread)
pub type CommandReceiver = rtrb::Consumer<AudioCommand>;

/// Type alias for event channel (Audio -> UI)
pub type EventSender = rtrb::Producer<AudioEvent>;
/// Event receiver (UI thread)
pub type EventReceiver = rtrb::Consumer<AudioEvent>;

/// Type alias for plugin instance (sent from UI to audio thread)
pub type PluginInstance = Box<dyn vvdaw_plugin::Plugin>;

/// Create a pair of channels for bidirectional communication
pub fn create_channels(capacity: usize) -> (UiChannels, AudioChannels) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(capacity);
    let (evt_tx, evt_rx) = rtrb::RingBuffer::new(capacity);
    let (plugin_tx, plugin_rx) = crossbeam_channel::unbounded();

    let ui_channels = UiChannels {
        command_tx: cmd_tx,
        event_rx: evt_rx,
        plugin_tx,
    };

    let audio_channels = AudioChannels {
        command_rx: cmd_rx,
        event_tx: evt_tx,
        plugin_rx,
    };

    (ui_channels, audio_channels)
}

/// Channels for the UI thread (sends commands, receives events)
pub struct UiChannels {
    /// Command sender (UI -> Audio)
    pub command_tx: rtrb::Producer<AudioCommand>,
    /// Event receiver (Audio -> UI)
    pub event_rx: rtrb::Consumer<AudioEvent>,
    /// Plugin sender (UI -> Audio) - separate channel for non-Clone types
    pub plugin_tx: Sender<PluginInstance>,
}

/// Channels for the audio thread (receives commands, sends events)
pub struct AudioChannels {
    /// Command receiver (UI -> Audio)
    pub command_rx: rtrb::Consumer<AudioCommand>,
    /// Event sender (Audio -> UI)
    pub event_tx: rtrb::Producer<AudioEvent>,
    /// Plugin receiver (UI -> Audio) - `try_recv` is non-blocking
    pub plugin_rx: Receiver<PluginInstance>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_creation() {
        let (mut ui, _audio) = create_channels(256);
        assert!(ui.command_tx.push(AudioCommand::Start).is_ok());
    }
}
