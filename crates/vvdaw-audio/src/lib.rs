//! Audio engine and processing graph.
//!
//! This crate provides the audio graph, audio thread management,
//! and integration with cpal for audio I/O.

pub mod engine;
pub mod graph;
pub mod session;

pub use engine::AudioEngine;
pub use graph::AudioGraph;
pub use session::Session;

use vvdaw_core::{Frames, SampleRate};

/// Audio configuration
#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub sample_rate: SampleRate,
    pub block_size: Frames,
    pub input_channels: usize,
    pub output_channels: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            block_size: 256,
            input_channels: 2,
            output_channels: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.block_size, 256);
    }
}
