//! Plugin abstraction layer.
//!
//! This crate defines the common `Plugin` trait that all plugin formats
//! (VST3, CLAP, etc.) must implement. This allows the audio engine to work
//! with plugins in a format-agnostic way.

use vvdaw_core::{ChannelCount, Frames, Sample, SampleRate};

/// Audio buffer for processing
pub struct AudioBuffer<'a> {
    pub inputs: &'a [&'a [Sample]],
    pub outputs: &'a mut [&'a mut [Sample]],
    pub frames: Frames,
}

/// MIDI/parameter events
#[derive(Debug, Clone)]
pub enum Event {
    /// Note on event
    NoteOn {
        channel: u8,
        note: u8,
        velocity: f32,
        sample_offset: u32,
    },
    /// Note off event
    NoteOff {
        channel: u8,
        note: u8,
        sample_offset: u32,
    },
    /// Parameter change
    ParamChange {
        id: u32,
        value: f32,
        sample_offset: u32,
    },
}

/// Buffer of events for a processing block
pub struct EventBuffer {
    pub events: Vec<Event>,
}

impl EventBuffer {
    #[must_use]
    pub const fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for EventBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a parameter
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub id: u32,
    pub name: String,
    pub min_value: f32,
    pub max_value: f32,
    pub default_value: f32,
}

/// Plugin metadata
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub unique_id: String,
}

/// Main plugin trait that all plugin formats implement
pub trait Plugin: Send {
    /// Get plugin information
    fn info(&self) -> &PluginInfo;

    /// Initialize the plugin with sample rate and max block size
    fn initialize(
        &mut self,
        sample_rate: SampleRate,
        max_block_size: Frames,
    ) -> Result<(), PluginError>;

    /// Process audio
    fn process(&mut self, audio: &mut AudioBuffer, events: &EventBuffer)
    -> Result<(), PluginError>;

    /// Set a parameter value (thread-safe, can be called from UI thread)
    fn set_parameter(&mut self, id: u32, value: f32) -> Result<(), PluginError>;

    /// Get a parameter value
    fn get_parameter(&self, id: u32) -> Result<f32, PluginError>;

    /// Get all parameters
    fn parameters(&self) -> Vec<ParameterInfo>;

    /// Get number of input channels
    fn input_channels(&self) -> ChannelCount;

    /// Get number of output channels
    fn output_channels(&self) -> ChannelCount;

    /// Deactivate and cleanup
    fn deactivate(&mut self);
}

/// Plugin-related errors
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Plugin processing failed: {0}")]
    ProcessingFailed(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Plugin format error: {0}")]
    FormatError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_buffer() {
        let mut buffer = EventBuffer::new();
        buffer.events.push(Event::NoteOn {
            channel: 0,
            note: 60,
            velocity: 0.8,
            sample_offset: 0,
        });
        assert_eq!(buffer.events.len(), 1);
    }
}
