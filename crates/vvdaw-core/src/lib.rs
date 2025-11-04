//! Core types, traits, and constants shared across the vvdaw ecosystem.
//!
//! This crate provides fundamental building blocks that all other vvdaw crates depend on.

/// Sample rate in Hz
pub type SampleRate = u32;

/// Number of audio frames (samples per channel)
pub type Frames = usize;

/// Audio sample type (32-bit float is standard for plugin hosting)
pub type Sample = f32;

/// Number of audio channels
pub type ChannelCount = usize;

/// Common sample rates
pub mod sample_rates {
    use super::SampleRate;

    /// 44.1 kHz sample rate (CD quality)
    pub const SR_44100: SampleRate = 44100;
    /// 48 kHz sample rate (professional audio standard)
    pub const SR_48000: SampleRate = 48000;
    /// 88.2 kHz sample rate (2x CD quality)
    pub const SR_88200: SampleRate = 88200;
    /// 96 kHz sample rate (high resolution audio)
    pub const SR_96000: SampleRate = 96000;
}

/// Audio buffer block sizes
pub mod block_sizes {
    use super::Frames;

    /// 64 frames per block (very low latency, ~1.3ms @ 48kHz)
    pub const BLOCK_64: Frames = 64;
    /// 128 frames per block (low latency, ~2.7ms @ 48kHz)
    pub const BLOCK_128: Frames = 128;
    /// 256 frames per block (balanced, ~5.3ms @ 48kHz)
    pub const BLOCK_256: Frames = 256;
    /// 512 frames per block (higher latency, ~10.7ms @ 48kHz)
    pub const BLOCK_512: Frames = 512;
}

/// Common error type
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Audio-related error
    #[error("Audio error: {0}")]
    Audio(String),

    /// Plugin-related error
    #[error("Plugin error: {0}")]
    Plugin(String),

    /// I/O error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Other unclassified error
    #[error("Other error: {0}")]
    Other(String),
}

/// Result type alias using our Error type
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_rates() {
        assert_eq!(sample_rates::SR_48000, 48000);
    }
}
