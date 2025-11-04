//! Audio engine - manages audio thread and cpal integration.

use crate::AudioConfig;
use vvdaw_comms::AudioChannels;

/// The audio engine manages the audio thread and cpal stream
pub struct AudioEngine {
    config: AudioConfig,
}

impl AudioEngine {
    /// Create a new audio engine with the given configuration
    pub fn new(config: AudioConfig) -> Self {
        Self { config }
    }

    /// Start the audio engine with the provided communication channels
    pub fn start(&mut self, _channels: AudioChannels) -> Result<(), anyhow::Error> {
        tracing::info!("Audio engine starting with config: {:?}", self.config);
        // TODO: Initialize cpal, create audio thread
        Ok(())
    }

    /// Stop the audio engine
    pub fn stop(&mut self) -> Result<(), anyhow::Error> {
        tracing::info!("Audio engine stopping");
        // TODO: Stop audio thread, cleanup
        Ok(())
    }
}
