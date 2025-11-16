//! Sample playback processor - plays loaded audio files.

use vvdaw_core::SampleRate;
use vvdaw_plugin::{AudioBuffer, EventBuffer, Plugin, PluginError, PluginInfo};

/// Sample playback processor
///
/// Plays back pre-loaded audio samples (e.g., from WAV files).
/// Continuously loops the audio while playing.
///
/// # Real-Time Safety
///
/// Uses `Box<[f32]>` instead of `Arc<Vec<f32>>` for sample storage to ensure
/// deterministic deallocation. When this processor is dropped (e.g., node removed
/// from graph), the boxed slice deallocates predictably, avoiding potential
/// non-deterministic Arc reference count decrements in the audio callback.
pub struct SamplerProcessor {
    /// Audio samples (interleaved stereo: [L, R, L, R, ...])
    ///
    /// Boxed slice ensures:
    /// - Deterministic deallocation (single heap free)
    /// - No reference counting overhead
    /// - Real-time safe cleanup when node is removed
    samples: Box<[f32]>,
    /// Current playback position (in frames, not samples)
    position: usize,
    /// Sample rate of the loaded audio
    audio_sample_rate: SampleRate,
    /// Engine sample rate
    engine_sample_rate: SampleRate,
    /// Plugin info
    info: PluginInfo,
}

impl SamplerProcessor {
    /// Create a new sampler with loaded audio data
    ///
    /// # Arguments
    /// * `samples` - Interleaved stereo audio data [L, R, L, R, ...]
    /// * `sample_rate` - Sample rate of the loaded audio
    ///
    /// # Real-Time Safety
    ///
    /// Converts `Vec<f32>` to `Box<[f32]>` for deterministic memory management.
    /// The conversion happens here (UI thread), not in the audio callback.
    pub fn new(samples: Vec<f32>, sample_rate: SampleRate) -> Self {
        Self {
            samples: samples.into_boxed_slice(),
            position: 0,
            audio_sample_rate: sample_rate,
            engine_sample_rate: 48000, // Will be updated in initialize()
            info: PluginInfo {
                name: "Sampler".to_string(),
                vendor: "vvdaw".to_string(),
                version: "1.0.0".to_string(),
                unique_id: "vvdaw.builtin.sampler".to_string(),
            },
        }
    }

    /// Get the number of frames in the loaded audio
    fn frame_count(&self) -> usize {
        self.samples.len() / 2 // Divide by 2 for stereo
    }
}

impl Plugin for SamplerProcessor {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn initialize(
        &mut self,
        sample_rate: SampleRate,
        _max_block_size: usize,
    ) -> Result<(), PluginError> {
        self.engine_sample_rate = sample_rate;

        // ⚠️ LIMITATION: No sample rate conversion implemented
        //
        // If the loaded audio file's sample rate doesn't match the engine sample rate,
        // playback will be at the wrong speed:
        // - 44.1kHz file on 48kHz engine → plays ~8.8% faster, pitch shifted up
        // - 48kHz file on 44.1kHz engine → plays ~8.2% slower, pitch shifted down
        //
        // WORKAROUNDS (for production use):
        // 1. Resample audio files offline before loading (using ffmpeg, sox, etc.)
        // 2. Implement real-time sample rate conversion (SRC):
        //    - Linear interpolation (fast, lower quality)
        //    - libsamplerate / rubberband (high quality, more CPU)
        //    - sinc resampling (best quality, highest CPU cost)
        // 3. Load-time resampling in the UI thread before sending to audio thread
        //
        // For this POC, we accept the limitation because:
        // - Most modern audio is 44.1kHz or 48kHz
        // - Sample rate conversion is complex and CPU-intensive
        // - Users can easily resample files offline if needed
        if self.audio_sample_rate != sample_rate {
            tracing::warn!(
                "Sample rate mismatch: audio is {}Hz, engine is {}Hz. Playback speed will be incorrect (~{:.1}%).",
                self.audio_sample_rate,
                sample_rate,
                ((f64::from(sample_rate) / f64::from(self.audio_sample_rate) - 1.0) * 100.0).abs()
            );
        }

        Ok(())
    }

    fn input_channels(&self) -> usize {
        0 // No inputs, only outputs
    }

    fn output_channels(&self) -> usize {
        2 // Stereo output
    }

    fn process(
        &mut self,
        audio: &mut AudioBuffer,
        _events: &EventBuffer,
    ) -> Result<(), PluginError> {
        // Validate we have 2 outputs (stereo)
        if audio.outputs.len() != 2 {
            return Err(PluginError::ProcessingFailed(format!(
                "Sampler requires exactly 2 outputs (stereo), got {}",
                audio.outputs.len()
            )));
        }

        // Validate buffer lengths
        for ch in 0..2 {
            if audio.outputs[ch].len() < audio.frames {
                return Err(PluginError::ProcessingFailed(format!(
                    "Output channel {ch} has {} samples, need at least {}",
                    audio.outputs[ch].len(),
                    audio.frames
                )));
            }
        }

        let frame_count = self.frame_count();

        // If no samples loaded, output silence
        if frame_count == 0 {
            for ch in 0..2 {
                for i in 0..audio.frames {
                    audio.outputs[ch][i] = 0.0;
                }
            }
            return Ok(());
        }

        // Output samples with looping
        for i in 0..audio.frames {
            // Get current frame position (with wrapping)
            let frame_pos = self.position % frame_count;
            let sample_pos = frame_pos * 2; // Convert to sample index (interleaved stereo)

            // Output left and right channels
            audio.outputs[0][i] = self.samples[sample_pos]; // Left
            audio.outputs[1][i] = self.samples[sample_pos + 1]; // Right

            // Advance position
            self.position += 1;
        }

        Ok(())
    }

    fn get_parameter(&self, id: u32) -> Result<f32, PluginError> {
        Err(PluginError::InvalidParameter(format!(
            "Sampler has no parameter with id {id}"
        )))
    }

    fn set_parameter(&mut self, id: u32, _value: f32) -> Result<(), PluginError> {
        Err(PluginError::InvalidParameter(format!(
            "Sampler has no parameter with id {id}"
        )))
    }

    fn parameters(&self) -> Vec<vvdaw_plugin::ParameterInfo> {
        vec![] // No parameters for now
    }

    fn deactivate(&mut self) {
        // Reset playback position on deactivation
        self.position = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sampler_creation() {
        let audio_data = vec![0.5, -0.5, 0.3, -0.3]; // 2 frames of stereo
        let sampler = SamplerProcessor::new(audio_data, 48000);
        assert_eq!(sampler.frame_count(), 2);
        assert_eq!(sampler.output_channels(), 2);
        assert_eq!(sampler.input_channels(), 0);
    }

    #[test]
    fn test_sampler_playback() {
        let audio_data = vec![1.0, -1.0, 0.5, -0.5]; // 2 frames
        let mut sampler = SamplerProcessor::new(audio_data, 48000);
        sampler.initialize(48000, 512).unwrap();

        let mut output_l = vec![0.0; 4];
        let mut output_r = vec![0.0; 4];
        let mut audio = AudioBuffer {
            inputs: &[],
            outputs: &mut [&mut output_l, &mut output_r],
            frames: 4,
        };

        sampler
            .process(&mut audio, &EventBuffer::default())
            .unwrap();

        // Should play 2 frames then loop
        assert_eq!(output_l[0], 1.0); // Frame 0 left
        assert_eq!(output_r[0], -1.0); // Frame 0 right
        assert_eq!(output_l[1], 0.5); // Frame 1 left
        assert_eq!(output_r[1], -0.5); // Frame 1 right
        assert_eq!(output_l[2], 1.0); // Loop back to frame 0
        assert_eq!(output_r[2], -1.0);
        assert_eq!(output_l[3], 0.5); // Frame 1 again
        assert_eq!(output_r[3], -0.5);
    }

    #[test]
    fn test_sampler_empty() {
        let mut sampler = SamplerProcessor::new(vec![], 48000);
        sampler.initialize(48000, 512).unwrap();

        let mut output_l = vec![999.0; 4];
        let mut output_r = vec![999.0; 4];
        let mut audio = AudioBuffer {
            inputs: &[],
            outputs: &mut [&mut output_l, &mut output_r],
            frames: 4,
        };

        sampler
            .process(&mut audio, &EventBuffer::default())
            .unwrap();

        // Should output silence
        for i in 0..4 {
            assert_eq!(output_l[i], 0.0);
            assert_eq!(output_r[i], 0.0);
        }
    }
}
