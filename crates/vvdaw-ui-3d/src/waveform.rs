//! Waveform data and mesh generation
//!
//! Converts audio samples into 3D geometry for visualization.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

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
}

impl WaveformData {
    /// Create new waveform data from samples
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
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
}

impl Default for WaveformMeshConfig {
    fn default() -> Self {
        Self {
            sample_stride: 10, // Skip samples for performance
            width: 0.5,
            amplitude_scale: 10.0,
            time_scale: 50.0,  // 50 units per second
            base_height: 10.0, // Elevate waveform 10 units above the road
        }
    }
}

/// Generate a 3D mesh from channel samples
///
/// Creates a wall-like mesh where:
/// - Z-axis = time (along the highway, extending backward)
/// - Y-axis = amplitude (height of the wall)
/// - X-axis = thickness (width of the wall, should match wall X position)
pub fn generate_channel_mesh(
    samples: &[f32],
    sample_rate: u32,
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

    // Calculate time per sample
    let time_per_sample = 1.0 / sample_rate as f32;

    // Generate vertices
    let mut vertex_index: u32 = 0;

    for (i, sample) in samples.iter().step_by(config.sample_stride).enumerate() {
        let time = (i * config.sample_stride) as f32 * time_per_sample;
        let z = -time * config.time_scale; // Negative so it extends backward along highway

        // Waveform oscillates around base_height (center line)
        let y_wave = config.base_height + (sample * config.amplitude_scale);
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
        if i > 0 {
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
