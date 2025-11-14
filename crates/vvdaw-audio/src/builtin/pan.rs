//! Pan processor - stereo panning control.

use std::f32::consts::FRAC_PI_2;
use std::sync::atomic::{AtomicU32, Ordering};
use vvdaw_core::SampleRate;
use vvdaw_plugin::{AudioBuffer, EventBuffer, ParameterInfo, Plugin, PluginError, PluginInfo};

/// Stereo pan processor using constant-power panning
///
/// Constant-power panning maintains perceived loudness as the sound moves
/// across the stereo field by ensuring that L² + R² = constant.
pub struct PanProcessor {
    /// Pan position stored as f32 bits (-1.0 = full left, 0.0 = center, 1.0 = full right)
    pan: AtomicU32,
    sample_rate: SampleRate,
    info: PluginInfo,
}

impl Default for PanProcessor {
    fn default() -> Self {
        Self {
            // Default pan: 0.0 (center)
            pan: AtomicU32::new(0.0_f32.to_bits()),
            sample_rate: 48000,
            info: PluginInfo {
                name: "Pan".to_string(),
                vendor: "vvdaw".to_string(),
                version: "1.0.0".to_string(),
                unique_id: "vvdaw.builtin.pan".to_string(),
            },
        }
    }
}

impl PanProcessor {
    /// Get the current pan value (thread-safe)
    fn get_pan(&self) -> f32 {
        f32::from_bits(self.pan.load(Ordering::Acquire))
    }

    /// Set the pan value (thread-safe)
    fn set_pan(&self, value: f32) {
        self.pan.store(value.to_bits(), Ordering::Release);
    }

    /// Calculate constant-power pan gains
    ///
    /// Returns (`left_gain`, `right_gain`) using constant-power law:
    /// - pan = -1.0: (1.0, 0.0) - full left
    /// - pan =  0.0: (0.707, 0.707) - center
    /// - pan =  1.0: (0.0, 1.0) - full right
    fn calculate_gains(pan: f32) -> (f32, f32) {
        // Convert pan from [-1, 1] to [0, 1]
        let pan_normalized = (pan + 1.0) * 0.5;

        // Convert to angle [0, π/2]
        let angle = pan_normalized * FRAC_PI_2;

        // Constant-power panning using cos/sin
        let left_gain = angle.cos();
        let right_gain = angle.sin();

        (left_gain, right_gain)
    }
}

impl Plugin for PanProcessor {
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
        let pan = self.get_pan();
        let (left_gain, right_gain) = Self::calculate_gains(pan);

        // Ensure we have exactly stereo input and output
        if audio.inputs.len() != 2 {
            return Err(PluginError::ProcessingFailed(format!(
                "Pan processor requires exactly 2 inputs (stereo), got {}",
                audio.inputs.len()
            )));
        }
        if audio.outputs.len() != 2 {
            return Err(PluginError::ProcessingFailed(format!(
                "Pan processor requires exactly 2 outputs (stereo), got {}",
                audio.outputs.len()
            )));
        }

        let left_in = audio.inputs[0];
        let right_in = audio.inputs[1];

        // Apply constant-power panning (stereo balance)
        // Left output gets left input with left gain, right output gets right input with right gain
        for i in 0..audio.frames {
            audio.outputs[0][i] = left_in[i] * left_gain;
            audio.outputs[1][i] = right_in[i] * right_gain;
        }

        Ok(())
    }

    fn set_parameter(&mut self, id: u32, value: f32) -> Result<(), PluginError> {
        match id {
            0 => {
                // Pan parameter: -1.0 (left) to 1.0 (right)
                let clamped = value.clamp(-1.0, 1.0);
                self.set_pan(clamped);
                Ok(())
            }
            _ => Err(PluginError::InvalidParameter(format!(
                "Unknown parameter ID: {id}"
            ))),
        }
    }

    fn get_parameter(&self, id: u32) -> Result<f32, PluginError> {
        match id {
            0 => Ok(self.get_pan()),
            _ => Err(PluginError::InvalidParameter(format!(
                "Unknown parameter ID: {id}"
            ))),
        }
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![ParameterInfo {
            id: 0,
            name: "Pan".to_string(),
            min_value: -1.0,
            max_value: 1.0,
            default_value: 0.0,
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
    fn test_pan_default() {
        let processor = PanProcessor::default();
        assert_eq!(processor.get_pan(), 0.0);
    }

    #[test]
    fn test_pan_parameter() {
        let mut processor = PanProcessor::default();

        // Set pan to left
        processor.set_parameter(0, -1.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), -1.0);

        // Set pan to right
        processor.set_parameter(0, 1.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 1.0);

        // Set pan to center
        processor.set_parameter(0, 0.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 0.0);
    }

    #[test]
    fn test_pan_clamping() {
        let mut processor = PanProcessor::default();

        // Try to set pan beyond max
        processor.set_parameter(0, 10.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), 1.0); // Should be clamped

        // Try to set pan below min
        processor.set_parameter(0, -10.0).unwrap();
        assert_eq!(processor.get_parameter(0).unwrap(), -1.0); // Should be clamped
    }

    #[test]
    fn test_constant_power_gains() {
        // At center, both gains should be ~0.707 (sqrt(0.5))
        let (left, right) = PanProcessor::calculate_gains(0.0);
        assert!((left - 0.707).abs() < 0.01);
        assert!((right - 0.707).abs() < 0.01);

        // Verify constant power: L² + R² ≈ 1
        let power = left.mul_add(left, right * right);
        assert!((power - 1.0).abs() < 0.01);

        // At full left
        let (left, right) = PanProcessor::calculate_gains(-1.0);
        assert!((left - 1.0).abs() < 0.01);
        assert!(right.abs() < 0.01);

        // At full right
        let (left, right) = PanProcessor::calculate_gains(1.0);
        assert!(left.abs() < 0.01);
        assert!((right - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_pan_processing_center() {
        let mut processor = PanProcessor::default();
        processor.initialize(48000, 512).unwrap();
        processor.set_parameter(0, 0.0).unwrap(); // Center

        // Create test buffers
        let left_in = vec![1.0; 64];
        let right_in = vec![0.5; 64];
        let mut left_out = vec![0.0; 64];
        let mut right_out = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&left_in, &right_in];
        let mut outputs: Vec<&mut [f32]> = vec![&mut left_out, &mut right_out];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // At center pan, both channels should be at ~0.707 gain
        let (left_gain, right_gain) = PanProcessor::calculate_gains(0.0);
        let expected_left = 1.0 * left_gain; // 1.0 * 0.707
        let expected_right = 0.5 * right_gain; // 0.5 * 0.707

        for sample in &left_out {
            assert!((*sample - expected_left).abs() < 0.01);
        }
        for sample in &right_out {
            assert!((*sample - expected_right).abs() < 0.01);
        }
    }

    #[test]
    fn test_pan_processing_full_left() {
        let mut processor = PanProcessor::default();
        processor.initialize(48000, 512).unwrap();
        processor.set_parameter(0, -1.0).unwrap(); // Full left

        // Create test buffers
        let left_in = vec![1.0; 64];
        let right_in = vec![0.5; 64];
        let mut left_out = vec![0.0; 64];
        let mut right_out = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&left_in, &right_in];
        let mut outputs: Vec<&mut [f32]> = vec![&mut left_out, &mut right_out];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // At full left, left channel should have full gain (1.0), right should be silent
        for sample in &left_out {
            assert!((*sample - 1.0).abs() < 0.01); // 1.0 * 1.0 = 1.0
        }
        for sample in &right_out {
            assert!(sample.abs() < 0.01); // 0.5 * 0.0 = 0.0
        }
    }

    #[test]
    fn test_pan_processing_full_right() {
        let mut processor = PanProcessor::default();
        processor.initialize(48000, 512).unwrap();
        processor.set_parameter(0, 1.0).unwrap(); // Full right

        // Create test buffers with distinct L/R values
        let left_in = vec![0.8; 64];
        let right_in = vec![0.6; 64];
        let mut left_out = vec![0.0; 64];
        let mut right_out = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&left_in, &right_in];
        let mut outputs: Vec<&mut [f32]> = vec![&mut left_out, &mut right_out];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // At full right, left channel should be silent, right should have full gain
        for sample in &left_out {
            assert!(sample.abs() < 0.01); // 0.8 * 0.0 = 0.0
        }
        for sample in &right_out {
            assert!((*sample - 0.6).abs() < 0.01); // 0.6 * 1.0 = 0.6
        }
    }

    #[test]
    fn test_pan_processing_distinct_values_left() {
        let mut processor = PanProcessor::default();
        processor.initialize(48000, 512).unwrap();
        processor.set_parameter(0, -0.5).unwrap(); // Mid-left

        // Create test buffers with very distinct L/R values
        let left_in = vec![1.0; 64];
        let right_in = vec![0.2; 64];
        let mut left_out = vec![0.0; 64];
        let mut right_out = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&left_in, &right_in];
        let mut outputs: Vec<&mut [f32]> = vec![&mut left_out, &mut right_out];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // At mid-left, left channel should be stronger than right
        let (left_gain, right_gain) = PanProcessor::calculate_gains(-0.5);
        let expected_left = 1.0 * left_gain;
        let expected_right = 0.2 * right_gain;

        // Verify left output is from left input only
        for sample in &left_out {
            assert!((*sample - expected_left).abs() < 0.01);
        }
        // Verify right output is from right input only
        for sample in &right_out {
            assert!((*sample - expected_right).abs() < 0.01);
        }
        // Verify stereo balance: left should be louder
        assert!(left_gain > right_gain);
    }

    #[test]
    fn test_pan_processing_distinct_values_right() {
        let mut processor = PanProcessor::default();
        processor.initialize(48000, 512).unwrap();
        processor.set_parameter(0, 0.5).unwrap(); // Mid-right

        // Create test buffers with very distinct L/R values
        let left_in = vec![0.3; 64];
        let right_in = vec![0.9; 64];
        let mut left_out = vec![0.0; 64];
        let mut right_out = vec![0.0; 64];

        let inputs: Vec<&[f32]> = vec![&left_in, &right_in];
        let mut outputs: Vec<&mut [f32]> = vec![&mut left_out, &mut right_out];

        let mut audio = AudioBuffer {
            inputs: &inputs,
            outputs: &mut outputs,
            frames: 64,
        };

        let events = EventBuffer::new();

        // Process
        processor.process(&mut audio, &events).unwrap();

        // At mid-right, right channel should be stronger than left
        let (left_gain, right_gain) = PanProcessor::calculate_gains(0.5);
        let expected_left = 0.3 * left_gain;
        let expected_right = 0.9 * right_gain;

        // Verify left output is from left input only
        for sample in &left_out {
            assert!((*sample - expected_left).abs() < 0.01);
        }
        // Verify right output is from right input only
        for sample in &right_out {
            assert!((*sample - expected_right).abs() < 0.01);
        }
        // Verify stereo balance: right should be louder
        assert!(right_gain > left_gain);
    }

    #[test]
    fn test_invalid_parameter() {
        let mut processor = PanProcessor::default();

        // Try to set unknown parameter
        let result = processor.set_parameter(999, 0.0);
        assert!(result.is_err());

        // Try to get unknown parameter
        let result = processor.get_parameter(999);
        assert!(result.is_err());
    }
}
