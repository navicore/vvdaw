//! Mixer processor - combines multiple stereo inputs.

use std::sync::atomic::{AtomicU32, Ordering};
use vvdaw_core::SampleRate;
use vvdaw_plugin::{AudioBuffer, EventBuffer, ParameterInfo, Plugin, PluginError, PluginInfo};

/// Simple 2-input stereo mixer
///
/// Mixes two stereo inputs to a single stereo output with individual
/// input gains and a master output gain.
///
/// ## Signal Flow
///
/// ```text
/// Input 1 (L/R) --[gain 1]--\
///                             >--[master]-->  Output (L/R)
/// Input 2 (L/R) --[gain 2]--/
/// ```
///
/// ## Parameters
///
/// All gains range from 0.0 to 2.0 (linear):
/// - **0**: Input 1 gain (0.0 to 2.0, default 1.0)
/// - **1**: Input 2 gain (0.0 to 2.0, default 1.0)
/// - **2**: Master gain (0.0 to 2.0, default 1.0)
///
/// Each parameter:
/// - 0.0 = silence (−∞ dB)
/// - 1.0 = unity gain (0 dB)
/// - 2.0 = double amplitude (+6 dB)
///
/// Range limited to 2.0 to prevent excessive clipping in typical use.
pub struct MixerProcessor {
    /// Input 1 gain stored as f32 bits
    input1_gain: AtomicU32,
    /// Input 2 gain stored as f32 bits
    input2_gain: AtomicU32,
    /// Master output gain stored as f32 bits
    master_gain: AtomicU32,
    sample_rate: SampleRate,
    info: PluginInfo,
}

impl Default for MixerProcessor {
    fn default() -> Self {
        Self {
            // Default gains: 1.0 (unity, 0 dB)
            input1_gain: AtomicU32::new(1.0_f32.to_bits()),
            input2_gain: AtomicU32::new(1.0_f32.to_bits()),
            master_gain: AtomicU32::new(1.0_f32.to_bits()),
            sample_rate: 48000,
            info: PluginInfo {
                name: "Mixer".to_string(),
                vendor: "vvdaw".to_string(),
                version: "1.0.0".to_string(),
                unique_id: "vvdaw.builtin.mixer".to_string(),
            },
        }
    }
}

impl MixerProcessor {
    /// Get input 1 gain (thread-safe)
    fn get_input1_gain(&self) -> f32 {
        f32::from_bits(self.input1_gain.load(Ordering::Acquire))
    }

    /// Set input 1 gain (thread-safe)
    fn set_input1_gain(&self, value: f32) {
        self.input1_gain.store(value.to_bits(), Ordering::Release);
    }

    /// Get input 2 gain (thread-safe)
    fn get_input2_gain(&self) -> f32 {
        f32::from_bits(self.input2_gain.load(Ordering::Acquire))
    }

    /// Set input 2 gain (thread-safe)
    fn set_input2_gain(&self, value: f32) {
        self.input2_gain.store(value.to_bits(), Ordering::Release);
    }

    /// Get master gain (thread-safe)
    fn get_master_gain(&self) -> f32 {
        f32::from_bits(self.master_gain.load(Ordering::Acquire))
    }

    /// Set master gain (thread-safe)
    fn set_master_gain(&self, value: f32) {
        self.master_gain.store(value.to_bits(), Ordering::Release);
    }
}

impl Plugin for MixerProcessor {
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
        let input1_gain = self.get_input1_gain();
        let input2_gain = self.get_input2_gain();
        let master_gain = self.get_master_gain();

        // Ensure we have exactly 4 inputs (2 stereo inputs) and 2 outputs (1 stereo output)
        if audio.inputs.len() != 4 {
            return Err(PluginError::ProcessingFailed(format!(
                "Mixer processor requires exactly 4 inputs (2 stereo), got {}",
                audio.inputs.len()
            )));
        }
        if audio.outputs.len() != 2 {
            return Err(PluginError::ProcessingFailed(format!(
                "Mixer processor requires exactly 2 outputs (1 stereo), got {}",
                audio.outputs.len()
            )));
        }

        // Validate buffer lengths
        for ch in 0..4 {
            if audio.inputs[ch].len() < audio.frames {
                return Err(PluginError::ProcessingFailed(format!(
                    "Input channel {} has {} samples, need at least {}",
                    ch,
                    audio.inputs[ch].len(),
                    audio.frames
                )));
            }
        }
        for ch in 0..2 {
            if audio.outputs[ch].len() < audio.frames {
                return Err(PluginError::ProcessingFailed(format!(
                    "Output channel {} has {} samples, need at least {}",
                    ch,
                    audio.outputs[ch].len(),
                    audio.frames
                )));
            }
        }

        // Input 1: channels 0-1 (L/R)
        // Input 2: channels 2-3 (L/R)
        let input1_left = audio.inputs[0];
        let input1_right = audio.inputs[1];
        let input2_left = audio.inputs[2];
        let input2_right = audio.inputs[3];

        // Mix to stereo output
        for i in 0..audio.frames {
            // Mix both inputs with their respective gains, then apply master gain
            audio.outputs[0][i] =
                input1_left[i].mul_add(input1_gain, input2_left[i] * input2_gain) * master_gain;
            audio.outputs[1][i] =
                input1_right[i].mul_add(input1_gain, input2_right[i] * input2_gain) * master_gain;
        }

        Ok(())
    }

    fn set_parameter(&mut self, id: u32, value: f32) -> Result<(), PluginError> {
        match id {
            0 => {
                // Input 1 gain: 0.0 to 2.0
                let clamped = value.clamp(0.0, 2.0);
                self.set_input1_gain(clamped);
                Ok(())
            }
            1 => {
                // Input 2 gain: 0.0 to 2.0
                let clamped = value.clamp(0.0, 2.0);
                self.set_input2_gain(clamped);
                Ok(())
            }
            2 => {
                // Master gain: 0.0 to 2.0
                let clamped = value.clamp(0.0, 2.0);
                self.set_master_gain(clamped);
                Ok(())
            }
            _ => Err(PluginError::InvalidParameter(format!(
                "Unknown parameter ID: {id}"
            ))),
        }
    }

    fn get_parameter(&self, id: u32) -> Result<f32, PluginError> {
        match id {
            0 => Ok(self.get_input1_gain()),
            1 => Ok(self.get_input2_gain()),
            2 => Ok(self.get_master_gain()),
            _ => Err(PluginError::InvalidParameter(format!(
                "Unknown parameter ID: {id}"
            ))),
        }
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![
            ParameterInfo {
                id: 0,
                name: "Input 1 Gain".to_string(),
                min_value: 0.0,
                max_value: 2.0,
                default_value: 1.0,
            },
            ParameterInfo {
                id: 1,
                name: "Input 2 Gain".to_string(),
                min_value: 0.0,
                max_value: 2.0,
                default_value: 1.0,
            },
            ParameterInfo {
                id: 2,
                name: "Master Gain".to_string(),
                min_value: 0.0,
                max_value: 2.0,
                default_value: 1.0,
            },
        ]
    }

    fn input_channels(&self) -> usize {
        4 // 2 stereo inputs
    }

    fn output_channels(&self) -> usize {
        2 // 1 stereo output
    }

    fn deactivate(&mut self) {
        // Nothing to clean up
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mixer_default() {
        let processor = MixerProcessor::default();
        assert_eq!(processor.get_input1_gain(), 1.0);
        assert_eq!(processor.get_input2_gain(), 1.0);
        assert_eq!(processor.get_master_gain(), 1.0);
    }

    #[test]
    fn test_mixer_parameters() {
        let mut processor = MixerProcessor::default();

        // Set input 1 gain
        processor.set_parameter(0, 0.5).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 0.5);

        // Set input 2 gain
        processor.set_parameter(1, 0.75).unwrap();
        assert_eq!(processor.get_parameter(1).unwrap(), 0.75);

        // Set master gain
        processor.set_parameter(2, 1.5).unwrap();
        assert_eq!(processor.get_parameter(2).unwrap(), 1.5);
    }

    #[test]
    fn test_mixer_clamping() {
        let mut processor = MixerProcessor::default();

        // Try to set gains beyond max
        processor.set_parameter(0, 10.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 2.0);

        processor.set_parameter(1, 10.0).unwrap();
        assert_eq!(processor.get_parameter(1).unwrap(), 2.0);

        processor.set_parameter(2, 10.0).unwrap();
        assert_eq!(processor.get_parameter(2).unwrap(), 2.0);

        // Try to set negative gains
        processor.set_parameter(0, -1.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 0.0);

        processor.set_parameter(1, -1.0).unwrap();
        assert_eq!(processor.get_parameter(1).unwrap(), 0.0);

        processor.set_parameter(2, -1.0).unwrap();
        assert_eq!(processor.get_parameter(2).unwrap(), 0.0);
    }

    #[test]
    fn test_mixer_processing_unity() {
        let mut processor = MixerProcessor::default();
        processor.initialize(48000, 512).unwrap();
        // Unity gains (all 1.0 by default)

        // Create test buffers: 2 stereo inputs
        let input1_l = vec![1.0; 64];
        let input1_r = vec![0.5; 64];
        let input2_l = vec![0.3; 64];
        let input2_r = vec![0.2; 64];
        let mut output_l = vec![0.0; 64];
        let mut output_r = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&input1_l, &input1_r, &input2_l, &input2_r];
        let mut outputs: Vec<&mut [f32]> = vec![&mut output_l, &mut output_r];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // With unity gains, output should be sum of inputs
        for sample in &output_l {
            assert!((*sample - 1.3).abs() < 0.001); // 1.0 + 0.3 = 1.3
        }
        for sample in &output_r {
            assert!((*sample - 0.7).abs() < 0.001); // 0.5 + 0.2 = 0.7
        }
    }

    #[test]
    fn test_mixer_processing_with_gains() {
        let mut processor = MixerProcessor::default();
        processor.initialize(48000, 512).unwrap();

        // Set gains
        processor.set_parameter(0, 0.5).unwrap(); // Input 1: 0.5
        processor.set_parameter(1, 0.0).unwrap(); // Input 2: 0.0 (muted)
        processor.set_parameter(2, 2.0).unwrap(); // Master: 2.0

        // Create test buffers
        let input1_l = vec![1.0; 64];
        let input1_r = vec![1.0; 64];
        let input2_l = vec![1.0; 64];
        let input2_r = vec![1.0; 64];
        let mut output_l = vec![0.0; 64];
        let mut output_r = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&input1_l, &input1_r, &input2_l, &input2_r];
        let mut outputs: Vec<&mut [f32]> = vec![&mut output_l, &mut output_r];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // Expected: (1.0 * 0.5 + 1.0 * 0.0) * 2.0 = 1.0
        for sample in &output_l {
            assert!((*sample - 1.0).abs() < 0.001);
        }
        for sample in &output_r {
            assert!((*sample - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_mixer_processing_master_gain() {
        let mut processor = MixerProcessor::default();
        processor.initialize(48000, 512).unwrap();

        // Only adjust master gain
        processor.set_parameter(2, 0.5).unwrap();

        // Create test buffers
        let input1_l = vec![1.0; 64];
        let input1_r = vec![1.0; 64];
        let input2_l = vec![1.0; 64];
        let input2_r = vec![1.0; 64];
        let mut output_l = vec![0.0; 64];
        let mut output_r = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&input1_l, &input1_r, &input2_l, &input2_r];
        let mut outputs: Vec<&mut [f32]> = vec![&mut output_l, &mut output_r];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // Expected: (1.0 * 1.0 + 1.0 * 1.0) * 0.5 = 1.0
        for sample in &output_l {
            assert!((*sample - 1.0).abs() < 0.001);
        }
        for sample in &output_r {
            assert!((*sample - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_invalid_parameter() {
        let mut processor = MixerProcessor::default();

        // Try to set unknown parameter
        let result = processor.set_parameter(999, 0.5);
        assert!(result.is_err());

        // Try to get unknown parameter
        let result = processor.get_parameter(999);
        assert!(result.is_err());
    }
}
