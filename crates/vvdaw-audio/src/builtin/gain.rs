//! Gain processor - simple volume control.

use std::sync::atomic::{AtomicU32, Ordering};
use vvdaw_core::SampleRate;
use vvdaw_plugin::{AudioBuffer, EventBuffer, ParameterInfo, Plugin, PluginError, PluginInfo};

/// Simple gain/volume processor
///
/// Multiplies all audio samples by a gain factor.
/// Real-time safe using atomic operations for parameter changes.
///
/// ## Parameter Range
///
/// Gain: 0.0 to 2.0 (linear)
/// - 0.0 = silence (−∞ dB)
/// - 1.0 = unity gain (0 dB, default)
/// - 2.0 = double amplitude (+6 dB)
///
/// The range allows for both attenuation and moderate boost.
/// Limited to 2.0 to prevent excessive clipping in typical use.
pub struct GainProcessor {
    /// Gain value stored as f32 bits in an atomic (for thread-safe access)
    gain: AtomicU32,
    sample_rate: SampleRate,
    info: PluginInfo,
}

impl Default for GainProcessor {
    fn default() -> Self {
        Self {
            // Default gain: 1.0 (unity, 0 dB)
            gain: AtomicU32::new(1.0_f32.to_bits()),
            sample_rate: 48000,
            info: PluginInfo {
                name: "Gain".to_string(),
                vendor: "vvdaw".to_string(),
                version: "1.0.0".to_string(),
                unique_id: "vvdaw.builtin.gain".to_string(),
            },
        }
    }
}

impl GainProcessor {
    /// Get the current gain value (thread-safe)
    fn get_gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Acquire))
    }

    /// Set the gain value (thread-safe)
    fn set_gain(&self, value: f32) {
        self.gain.store(value.to_bits(), Ordering::Release);
    }
}

impl Plugin for GainProcessor {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn initialize(
        &mut self,
        sample_rate: SampleRate,
        _max_block_size: usize,
    ) -> Result<(), PluginError> {
        self.sample_rate = sample_rate;
        Ok(())
    }

    fn process(
        &mut self,
        audio: &mut AudioBuffer,
        _events: &EventBuffer,
    ) -> Result<(), PluginError> {
        let gain = self.get_gain();

        // Ensure we have exactly 2 inputs and 2 outputs (stereo)
        if audio.inputs.len() != 2 {
            return Err(PluginError::ProcessingFailed(format!(
                "Gain processor requires exactly 2 inputs (stereo), got {}",
                audio.inputs.len()
            )));
        }
        if audio.outputs.len() != 2 {
            return Err(PluginError::ProcessingFailed(format!(
                "Gain processor requires exactly 2 outputs (stereo), got {}",
                audio.outputs.len()
            )));
        }

        // Validate buffer lengths
        for ch in 0..2 {
            if audio.inputs[ch].len() < audio.frames {
                return Err(PluginError::ProcessingFailed(format!(
                    "Input channel {} has {} samples, need at least {}",
                    ch,
                    audio.inputs[ch].len(),
                    audio.frames
                )));
            }
            if audio.outputs[ch].len() < audio.frames {
                return Err(PluginError::ProcessingFailed(format!(
                    "Output channel {} has {} samples, need at least {}",
                    ch,
                    audio.outputs[ch].len(),
                    audio.frames
                )));
            }
        }

        // Copy input to output and apply gain
        for ch in 0..2 {
            for i in 0..audio.frames {
                audio.outputs[ch][i] = audio.inputs[ch][i] * gain;
            }
        }

        Ok(())
    }

    fn set_parameter(&mut self, id: u32, value: f32) -> Result<(), PluginError> {
        match id {
            0 => {
                // Gain parameter: 0.0 to 2.0 (0 dB to +6 dB range)
                // Clamp to prevent extreme values
                let clamped = value.clamp(0.0, 2.0);
                self.set_gain(clamped);
                Ok(())
            }
            _ => Err(PluginError::InvalidParameter(format!(
                "Unknown parameter ID: {id}"
            ))),
        }
    }

    fn get_parameter(&self, id: u32) -> Result<f32, PluginError> {
        match id {
            0 => Ok(self.get_gain()),
            _ => Err(PluginError::InvalidParameter(format!(
                "Unknown parameter ID: {id}"
            ))),
        }
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![ParameterInfo {
            id: 0,
            name: "Gain".to_string(),
            min_value: 0.0,
            max_value: 2.0,
            default_value: 1.0,
        }]
    }

    fn input_channels(&self) -> usize {
        2 // Stereo
    }

    fn output_channels(&self) -> usize {
        2 // Stereo
    }

    fn deactivate(&mut self) {
        // Nothing to clean up
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gain_default() {
        let processor = GainProcessor::default();
        assert_eq!(processor.get_gain(), 1.0);
    }

    #[test]
    fn test_gain_parameter() {
        let mut processor = GainProcessor::default();

        // Set gain to 0.5
        processor.set_parameter(0, 0.5).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 0.5);

        // Set gain to 2.0
        processor.set_parameter(0, 2.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 2.0);
    }

    #[test]
    fn test_gain_clamping() {
        let mut processor = GainProcessor::default();

        // Try to set gain beyond max
        processor.set_parameter(0, 10.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 2.0); // Should be clamped

        // Try to set negative gain
        processor.set_parameter(0, -1.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 0.0); // Should be clamped
    }

    #[test]
    fn test_gain_processing() {
        let mut processor = GainProcessor::default();
        processor.initialize(48000, 512).unwrap();
        processor.set_parameter(0, 0.5).unwrap();

        // Create test buffers
        let input_l = vec![1.0; 64];
        let input_r = vec![1.0; 64];
        let mut output_l = vec![0.0; 64];
        let mut output_r = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&input_l, &input_r];
        let mut outputs: Vec<&mut [f32]> = vec![&mut output_l, &mut output_r];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // Check output
        for sample in &output_l {
            assert_eq!(*sample, 0.5);
        }
        for sample in &output_r {
            assert_eq!(*sample, 0.5);
        }
    }

    #[test]
    fn test_invalid_parameter() {
        let mut processor = GainProcessor::default();

        // Try to set unknown parameter
        let result = processor.set_parameter(999, 0.5);
        assert!(result.is_err());

        // Try to get unknown parameter
        let result = processor.get_parameter(999);
        assert!(result.is_err());
    }
}
