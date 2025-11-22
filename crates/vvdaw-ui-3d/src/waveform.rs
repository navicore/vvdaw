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
    /// Thickness of the wall (depth in X direction)
    pub wall_thickness: f32,
    /// Total height of the base wall panel
    pub wall_height: f32,
    /// How far the waveform extrudes from the base wall surface
    pub waveform_extrusion: f32,
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
            sample_stride: 10,        // Skip samples for performance
            wall_thickness: 0.5,      // 0.5 units thick
            wall_height: 25.0,        // 25 units tall base wall
            waveform_extrusion: 0.15, // Waveform protrudes 0.15 units from base
            amplitude_scale: 10.0,    // Amplitude scaling
            time_scale: 50.0,         // 50 units per second
            base_height: 10.0,        // Center line at 10 units above road
            window_duration: 15.0,    // Render 15 seconds ahead and behind (30 total)
        }
    }
}

/// Create an empty mesh with required PBR attributes
fn create_empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, Vec::<[f32; 4]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    mesh
}

/// Add base wall rectangle faces (inner and outer)
fn add_wall_faces(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    tangents: &mut Vec<[f32; 4]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    corners: [[f32; 3]; 8],
    is_left_wall: bool,
) {
    let base_start_idx = positions.len() as u32;
    positions.extend_from_slice(&corners);

    // Determine normal directions based on wall side
    let (normal_inner, normal_outer) = if is_left_wall {
        ([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0])
    } else {
        ([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0])
    };
    normals.extend_from_slice(&[
        normal_inner,
        normal_inner,
        normal_inner,
        normal_inner,
        normal_outer,
        normal_outer,
        normal_outer,
        normal_outer,
    ]);

    let tangent = [0.0, 0.0, 1.0, 1.0];
    tangents.extend_from_slice(&[
        tangent, tangent, tangent, tangent, tangent, tangent, tangent, tangent,
    ]);

    uvs.extend_from_slice(&[
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 1.0],
        [1.0, 0.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 1.0],
        [1.0, 0.0],
    ]);

    // Inner and outer face triangles
    indices.extend_from_slice(&[
        base_start_idx,
        base_start_idx + 1,
        base_start_idx + 2,
        base_start_idx + 2,
        base_start_idx + 1,
        base_start_idx + 3,
        base_start_idx + 4,
        base_start_idx + 6,
        base_start_idx + 5,
        base_start_idx + 6,
        base_start_idx + 7,
        base_start_idx + 5,
    ]);
}

/// Add top or bottom edge quad
fn add_edge_quad(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    tangents: &mut Vec<[f32; 4]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
) {
    let start_idx = positions.len() as u32;
    positions.extend_from_slice(&corners);
    normals.extend_from_slice(&[normal, normal, normal, normal]);

    let tangent = [0.0, 0.0, 1.0, 1.0];
    tangents.extend_from_slice(&[tangent, tangent, tangent, tangent]);
    uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]);

    indices.extend_from_slice(&[
        start_idx,
        start_idx + 1,
        start_idx + 2,
        start_idx + 1,
        start_idx + 3,
        start_idx + 2,
    ]);
}

/// Calculate normal for a quad from its vertex positions
///
/// Uses cross product of two edge vectors to compute the face normal.
/// The normal is normalized to unit length for proper lighting calculations.
fn calculate_quad_normal(v0: [f32; 3], v1: [f32; 3], v2: [f32; 3]) -> [f32; 3] {
    // Calculate two edge vectors
    let edge1 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let edge2 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];

    // Cross product: edge1 × edge2
    let normal = [
        edge1[1].mul_add(edge2[2], -(edge1[2] * edge2[1])),
        edge1[2].mul_add(edge2[0], -(edge1[0] * edge2[2])),
        edge1[0].mul_add(edge2[1], -(edge1[1] * edge2[0])),
    ];

    // Normalize to unit length
    let length = normal[0]
        .mul_add(
            normal[0],
            normal[1].mul_add(normal[1], normal[2] * normal[2]),
        )
        .sqrt();
    if length > 0.0001 {
        [normal[0] / length, normal[1] / length, normal[2] / length]
    } else {
        // Fallback for degenerate quad (shouldn't happen in practice)
        [0.0, 1.0, 0.0]
    }
}

/// Generate separate meshes for base wall and waveform relief
///
/// Returns (`base_wall_mesh`, `waveform_mesh`) as separate meshes that can have different materials.
/// - Base wall: solid rectangular panel from ground to `wall_height` (for concrete material)
/// - Waveform: relief geometry extruding from the inner face toward road center (for glowing material)
///
/// Only renders samples within the time window centered on `current_position_seconds`
///
/// `is_left_wall`: true for left wall (extrudes in +X), false for right wall (extrudes in -X)
pub fn generate_channel_meshes(
    samples: &[f32],
    sample_rate: u32,
    current_position_seconds: f32,
    config: &WaveformMeshConfig,
    is_left_wall: bool,
) -> (Mesh, Mesh) {
    (
        generate_base_wall_mesh(
            samples,
            sample_rate,
            current_position_seconds,
            config,
            is_left_wall,
        ),
        generate_waveform_mesh(
            samples,
            sample_rate,
            current_position_seconds,
            config,
            is_left_wall,
        ),
    )
}

/// Generate base wall mesh (solid panel)
fn generate_base_wall_mesh(
    samples: &[f32],
    sample_rate: u32,
    current_position_seconds: f32,
    config: &WaveformMeshConfig,
    is_left_wall: bool,
) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut tangents = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    if samples.is_empty() {
        return create_empty_mesh();
    }

    let start_time = (current_position_seconds - config.window_duration).max(0.0);
    let end_time = current_position_seconds + config.window_duration;
    let start_sample = (start_time * sample_rate as f32) as usize;
    let end_sample = ((end_time * sample_rate as f32) as usize).min(samples.len());

    if start_sample >= samples.len() || start_sample >= end_sample {
        return create_empty_mesh();
    }

    let time_per_sample = 1.0 / sample_rate as f32;
    let half_thickness = config.wall_thickness / 2.0;

    // Calculate z positions for start and end of window
    let start_abs_time = start_sample as f32 * time_per_sample;
    let end_abs_time = end_sample.saturating_sub(1) as f32 * time_per_sample;
    let z_start = -(start_abs_time - current_position_seconds) * config.time_scale;
    let z_end = -(end_abs_time - current_position_seconds) * config.time_scale;

    // --- STEP 1: Generate base wall panel (simple rectangle) ---

    // Inner face vertices (toward road center, +X)
    let inner_top_left = [half_thickness, config.wall_height, z_start];
    let inner_bottom_left = [half_thickness, 0.0, z_start];
    let inner_top_right = [half_thickness, config.wall_height, z_end];
    let inner_bottom_right = [half_thickness, 0.0, z_end];

    // Outer face vertices (away from road, -X)
    let outer_top_left = [-half_thickness, config.wall_height, z_start];
    let outer_bottom_left = [-half_thickness, 0.0, z_start];
    let outer_top_right = [-half_thickness, config.wall_height, z_end];
    let outer_bottom_right = [-half_thickness, 0.0, z_end];

    // Add base wall inner/outer faces
    add_wall_faces(
        &mut positions,
        &mut normals,
        &mut tangents,
        &mut uvs,
        &mut indices,
        [
            inner_top_left,
            inner_bottom_left,
            inner_top_right,
            inner_bottom_right,
            outer_top_left,
            outer_bottom_left,
            outer_top_right,
            outer_bottom_right,
        ],
        is_left_wall,
    );

    // Add top edge
    add_edge_quad(
        &mut positions,
        &mut normals,
        &mut tangents,
        &mut uvs,
        &mut indices,
        [
            inner_top_left,
            inner_top_right,
            outer_top_left,
            outer_top_right,
        ],
        [0.0, 1.0, 0.0],
    );

    // Add bottom edge
    add_edge_quad(
        &mut positions,
        &mut normals,
        &mut tangents,
        &mut uvs,
        &mut indices,
        [
            inner_bottom_left,
            outer_bottom_left,
            inner_bottom_right,
            outer_bottom_right,
        ],
        [0.0, -1.0, 0.0],
    );

    // Build base wall mesh
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// Generate waveform relief mesh (extrudes toward road center)
fn generate_waveform_mesh(
    samples: &[f32],
    sample_rate: u32,
    current_position_seconds: f32,
    config: &WaveformMeshConfig,
    is_left_wall: bool,
) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut tangents = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    if samples.is_empty() {
        return create_empty_mesh();
    }

    let sample_stride = config.sample_stride.max(1);
    let start_time = (current_position_seconds - config.window_duration).max(0.0);
    let end_time = current_position_seconds + config.window_duration;
    let start_sample = (start_time * sample_rate as f32) as usize;
    let end_sample = ((end_time * sample_rate as f32) as usize).min(samples.len());

    if start_sample >= samples.len() || start_sample >= end_sample {
        return create_empty_mesh();
    }

    let time_per_sample = 1.0 / sample_rate as f32;
    let half_thickness = config.wall_thickness / 2.0;

    // Generate waveform relief on outer face
    let mut prev_z = None;
    let mut prev_y_wave = None;

    for sample_idx in (start_sample..end_sample).step_by(sample_stride) {
        let sample = samples[sample_idx];
        let absolute_time = sample_idx as f32 * time_per_sample;
        let relative_time = absolute_time - current_position_seconds;
        let z = -relative_time * config.time_scale;

        // Waveform height oscillates around base_height
        let y_wave = sample.mul_add(config.amplitude_scale, config.base_height);

        // Waveform extrudes toward road center (inner face)
        // Left wall: inner face at +half_thickness, extrude toward +X (road center)
        // Right wall: inner face at -half_thickness, extrude toward -X (road center)
        let (x_base, x_extruded) = if is_left_wall {
            // Left wall: positioned at negative X, so +half_thickness is the inner face
            let base = half_thickness;
            let extruded = base + config.waveform_extrusion;
            (base, extruded)
        } else {
            // Right wall: positioned at positive X, so -half_thickness is the inner face
            let base = -half_thickness;
            let extruded = base - config.waveform_extrusion;
            (base, extruded)
        };

        if let (Some(prev_z_val), Some(prev_y_val)) = (prev_z, prev_y_wave) {
            // Create quad connecting previous sample to current sample
            let wave_start_idx = positions.len() as u32;

            // Four vertices for the waveform quad
            let v0 = [x_base, prev_y_val, prev_z_val]; // Previous on base
            let v1 = [x_extruded, prev_y_val, prev_z_val]; // Previous extruded
            let v2 = [x_base, y_wave, z]; // Current on base
            let v3 = [x_extruded, y_wave, z]; // Current extruded

            positions.extend_from_slice(&[v0, v1, v2, v3]);

            // Calculate proper normal from quad geometry
            let wave_normal = calculate_quad_normal(v0, v1, v2);
            normals.extend_from_slice(&[wave_normal, wave_normal, wave_normal, wave_normal]);

            // Tangents for the quad (Z direction along wall, w=1.0 for handedness)
            let wave_tangent = [0.0, 0.0, 1.0, 1.0];
            tangents.extend_from_slice(&[wave_tangent, wave_tangent, wave_tangent, wave_tangent]);

            // UVs for the quad (simple planar mapping)
            uvs.extend_from_slice(&[
                [0.0, 0.0], // Previous on base
                [1.0, 0.0], // Previous extruded
                [0.0, 1.0], // Current on base
                [1.0, 1.0], // Current extruded
            ]);

            // Two triangles for the quad
            indices.extend_from_slice(&[
                wave_start_idx,
                wave_start_idx + 2,
                wave_start_idx + 1,
                wave_start_idx + 1,
                wave_start_idx + 2,
                wave_start_idx + 3,
            ]);
        }

        prev_z = Some(z);
        prev_y_wave = Some(y_wave);
    }

    // Build waveform mesh
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}
