//! Waveform data and mesh generation
//!
//! Converts audio samples into 3D geometry for visualization.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::VecDeque;

/// Resource holding loaded audio waveform data
///
/// This is the same audio data being played back - cloned from the audio thread
/// to ensure "what you see is what you hear".
#[derive(Resource, Default)]
pub struct WaveformData {
    /// Interleaved stereo samples [L, R, L, R, ...]
    /// Empty if no audio loaded
    pub samples: Vec<f32>,
    /// Sample rate of the loaded audio
    pub sample_rate: u32,

    /// Streaming waveform data: peak values from audio thread
    /// Each tuple is (`left_peak`, `right_peak`) for one audio buffer
    /// Uses `VecDeque` for O(1) `pop_front` operations when maintaining ring buffer
    pub streaming_peaks: VecDeque<(f32, f32)>,
    /// Current playback position (frame number) from audio thread
    pub current_position: u64,
    /// Maximum number of peak samples to store (ring buffer for scrolling display)
    /// At ~90 samples/sec, 9000 samples = ~100 seconds of history
    pub max_streaming_samples: usize,

    /// Flag to force mesh regeneration (set when new file is loaded)
    pub needs_mesh_update: bool,

    /// Last position (in seconds) where mesh was generated
    /// Used to throttle mesh updates - only regenerate when position changes significantly
    pub last_mesh_position: f32,
}

impl WaveformData {
    /// Create new waveform data from samples
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
            streaming_peaks: VecDeque::new(),
            current_position: 0,
            max_streaming_samples: 9000, // ~100 seconds at 90 samples/sec
            needs_mesh_update: true,     // Always request mesh update for new data
            last_mesh_position: 0.0,
        }
    }

    /// Append a new peak sample from the audio thread
    ///
    /// Maintains a ring buffer of the most recent peak samples
    pub fn push_streaming_peak(&mut self, position: u64, left_peak: f32, right_peak: f32) {
        self.current_position = position;

        // Add new peak sample
        self.streaming_peaks.push_back((left_peak, right_peak));

        // Maintain ring buffer size (O(1) with VecDeque)
        if self.streaming_peaks.len() > self.max_streaming_samples {
            self.streaming_peaks.pop_front();
        }
    }

    /// Clear streaming data (e.g., when loading new audio)
    pub fn clear_streaming(&mut self) {
        self.streaming_peaks.clear();
        self.current_position = 0;
        self.last_mesh_position = 0.0;
    }

    /// Get streaming peak data for visualization
    pub fn streaming_left_channel(&self) -> Vec<f32> {
        self.streaming_peaks.iter().map(|(l, _)| *l).collect()
    }

    /// Get streaming peak data for visualization
    pub fn streaming_right_channel(&self) -> Vec<f32> {
        self.streaming_peaks.iter().map(|(_, r)| *r).collect()
    }

    /// Get the number of stereo frames
    pub fn frame_count(&self) -> usize {
        self.samples.len() / 2
    }

    /// Check if waveform data is loaded
    pub fn is_loaded(&self) -> bool {
        !self.samples.is_empty()
    }

    /// Extract left channel samples (de-interleave)
    pub fn left_channel(&self) -> Vec<f32> {
        self.samples.iter().step_by(2).copied().collect()
    }

    /// Extract right channel samples (de-interleave)
    pub fn right_channel(&self) -> Vec<f32> {
        self.samples.iter().skip(1).step_by(2).copied().collect()
    }
}

/// Configuration for waveform mesh generation
pub struct WaveformMeshConfig {
    /// How many samples to skip between vertices (LOD)
    /// 1 = every sample, 10 = every 10th sample, etc.
    pub sample_stride: usize,
    /// Width of the wall in world units
    pub width: f32,
    /// Scale factor for amplitude (height)
    pub amplitude_scale: f32,
    /// Length per second of audio in world units
    pub time_scale: f32,
    /// Base height above the road surface (center line of waveform)
    pub base_height: f32,
    /// Time window to render (seconds before and after current position)
    pub window_duration: f32,
}

impl Default for WaveformMeshConfig {
    fn default() -> Self {
        Self {
            sample_stride: 10, // Skip samples for performance
            width: 0.5,
            amplitude_scale: 10.0,
            time_scale: 50.0,      // 50 units per second
            base_height: 10.0,     // Elevate waveform 10 units above the road
            window_duration: 15.0, // Render 15 seconds ahead and behind (30 total)
        }
    }
}

/// Generate a 3D mesh from channel samples for a time window
///
/// Creates a wall-like mesh where:
/// - Z-axis = time (along the highway, extending backward)
/// - Y-axis = amplitude (height of the wall)
/// - X-axis = thickness (width of the wall, should match wall X position)
///
/// Only renders samples within the time window centered on `current_position_seconds`
pub fn generate_channel_mesh(
    samples: &[f32],
    sample_rate: u32,
    current_position_seconds: f32,
    config: &WaveformMeshConfig,
) -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();

    if samples.is_empty() {
        // Return empty mesh if no samples
        return Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
    }

    // Validate sample_stride to prevent division by zero
    let sample_stride = config.sample_stride.max(1);

    // Calculate time window bounds
    let start_time = (current_position_seconds - config.window_duration).max(0.0);
    let end_time = current_position_seconds + config.window_duration;

    // Convert to sample indices
    let start_sample = (start_time * sample_rate as f32) as usize;
    let end_sample = ((end_time * sample_rate as f32) as usize).min(samples.len());

    // Early return if window is outside sample range
    if start_sample >= samples.len() || start_sample >= end_sample {
        return Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
    }

    // Calculate time per sample
    let time_per_sample = 1.0 / sample_rate as f32;

    // Generate vertices only for the visible window
    let mut vertex_index: u32 = 0;
    let mut first_vertex = true;

    for sample_idx in (start_sample..end_sample).step_by(sample_stride) {
        let sample = samples[sample_idx];

        // Time relative to the start of the audio
        let absolute_time = sample_idx as f32 * time_per_sample;

        // Z position relative to current playback position (keeps mesh centered at origin)
        let relative_time = absolute_time - current_position_seconds;
        let z = -relative_time * config.time_scale;

        // Waveform oscillates around base_height (center line)
        let y_wave = sample.mul_add(config.amplitude_scale, config.base_height);
        let y_center = config.base_height;
        let half_width = config.width / 2.0;

        // Create quad for this sample (2 triangles)
        // Front face (toward positive X)
        positions.push([half_width, y_wave, z]); // Waveform value
        positions.push([half_width, y_center, z]); // Center line

        // Back face (toward negative X)
        positions.push([-half_width, y_wave, z]); // Waveform value
        positions.push([-half_width, y_center, z]); // Center line

        // Create indices for quad (if not the first vertex)
        if !first_vertex {
            let base = vertex_index;

            // Front face (2 triangles)
            indices.push(base - 4);
            indices.push(base - 3);
            indices.push(base + 1);

            indices.push(base - 4);
            indices.push(base + 1);
            indices.push(base);

            // Back face (2 triangles)
            indices.push(base - 2);
            indices.push(base + 2);
            indices.push(base - 1);

            indices.push(base - 1);
            indices.push(base + 2);
            indices.push(base + 3);
        }

        first_vertex = false;
        vertex_index += 4;
    }

    // Build mesh
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));

    // Compute smooth normals (works with indexed geometry)
    mesh.compute_smooth_normals();

    mesh
}
