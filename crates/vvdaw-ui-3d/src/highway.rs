//! Highway geometry - road surface and waveform walls
//!
//! Creates the "infinite highway" where:
//! - The road surface represents the timeline
//! - Left/right walls represent stereo waveforms (guardrails)

use crate::waveform::{WaveformData, WaveformMeshConfig, generate_channel_mesh};
use bevy::prelude::*;

pub struct HighwayPlugin;

impl Plugin for HighwayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            (setup_highway, generate_test_waveform_if_empty).chain(),
        )
        .add_systems(Update, update_waveform_meshes);
    }
}

/// Marker component for left channel wall
#[derive(Component)]
struct LeftWall;

/// Marker component for right channel wall
#[derive(Component)]
struct RightWall;

/// Highway visual configuration
const ROAD_WIDTH: f32 = 20.0;
const ROAD_LENGTH: f32 = 500.0;

/// Setup the highway geometry (road + placeholder walls)
fn setup_highway(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Grid road surface - dark gray
    let road_mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::new(ROAD_WIDTH, ROAD_LENGTH)));
    let road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.2), // Medium gray for visibility
        metallic: 0.0,
        perceptual_roughness: 0.8,
        ..default()
    });

    commands.spawn((
        Mesh3d(road_mesh),
        MeshMaterial3d(road_material),
        Transform::from_xyz(0.0, 0.0, -ROAD_LENGTH / 2.0),
    ));

    // Spawn placeholder entities for waveform walls
    // Meshes will be generated when waveform data is loaded

    // Left channel wall (placeholder) - bright green
    commands.spawn((
        Mesh3d(meshes.add(Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::render::render_asset::RenderAssetUsages::RENDER_WORLD,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 1.0, 0.0), // Pure bright green
            metallic: 0.0,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(-ROAD_WIDTH / 2.0, 0.0, 0.0),
        LeftWall,
    ));

    // Right channel wall (placeholder) - bright red
    commands.spawn((
        Mesh3d(meshes.add(Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::render::render_asset::RenderAssetUsages::RENDER_WORLD,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.0, 0.0), // Pure bright red
            metallic: 0.0,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(ROAD_WIDTH / 2.0, 0.0, 0.0),
        RightWall,
    ));
}

/// Generate test waveform data (sine waves for POC) if no data is already loaded
fn generate_test_waveform_if_empty(mut waveform: ResMut<WaveformData>) {
    const SAMPLE_RATE: u32 = 48000;
    const DURATION_SECS: f32 = 2.0;
    const FREQUENCY_LEFT: f32 = 220.0; // A3
    const FREQUENCY_RIGHT: f32 = 330.0; // E4

    // Only generate test data if waveform is empty
    if waveform.is_loaded() {
        tracing::info!(
            "Using pre-loaded waveform data: {} frames at {}Hz",
            waveform.frame_count(),
            waveform.sample_rate
        );
        return;
    }

    let num_samples = (SAMPLE_RATE as f32 * DURATION_SECS) as usize;
    let mut samples = Vec::with_capacity(num_samples * 2);

    for i in 0..num_samples {
        let t = i as f32 / SAMPLE_RATE as f32;

        // Left channel - lower frequency sine wave
        let left = (2.0 * std::f32::consts::PI * FREQUENCY_LEFT * t).sin() * 0.5;

        // Right channel - higher frequency sine wave
        let right = (2.0 * std::f32::consts::PI * FREQUENCY_RIGHT * t).sin() * 0.5;

        samples.push(left);
        samples.push(right);
    }

    *waveform = WaveformData::new(samples, SAMPLE_RATE);

    tracing::info!(
        "Generated test waveform: {} frames at {}Hz",
        waveform.frame_count(),
        SAMPLE_RATE
    );
}

/// Update waveform wall meshes when waveform data changes
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn update_waveform_meshes(
    waveform: Res<WaveformData>,
    mut meshes: ResMut<Assets<Mesh>>,
    left_query: Query<&Mesh3d, With<LeftWall>>,
    right_query: Query<&Mesh3d, With<RightWall>>,
) {
    const TARGET_LENGTH: f32 = 400.0;

    // Only update if waveform data has changed
    if !waveform.is_changed() || !waveform.is_loaded() {
        return;
    }

    let frame_count = waveform.frame_count();
    let duration_secs = frame_count as f32 / waveform.sample_rate as f32;
    tracing::info!(
        "Updating waveform meshes from {} frames ({:.1}s duration)",
        frame_count,
        duration_secs
    );

    // Adaptive LOD based on file size
    // Target ~10k vertices maximum for good performance
    let target_vertices = 10_000;
    let sample_stride = (frame_count / target_vertices).max(1);

    // Adaptive time scale to fit waveform in visible area
    // Target 400 units total length (fits well within camera view)
    let time_scale = if duration_secs > 0.0 {
        TARGET_LENGTH / duration_secs
    } else {
        50.0
    };

    let config = WaveformMeshConfig {
        sample_stride,
        time_scale,
        amplitude_scale: 20.0, // Increased from 10.0 for better visibility
        base_height: 15.0,     // Elevate waveform walls above the road
        ..Default::default()
    };

    tracing::info!(
        "Mesh config: stride={}, time_scale={:.2}, estimated vertices={}, length={:.1} units",
        sample_stride,
        time_scale,
        frame_count / sample_stride,
        duration_secs * time_scale
    );

    // Update left channel mesh
    if let Ok(mesh_handle) = left_query.get_single() {
        tracing::info!("Generating left channel mesh...");
        let left_samples = waveform.left_channel();
        let mesh = generate_channel_mesh(&left_samples, waveform.sample_rate, &config);

        if let Some(mesh_asset) = meshes.get_mut(&mesh_handle.0) {
            *mesh_asset = mesh;
            tracing::info!("✓ Left channel mesh updated");
        } else {
            tracing::warn!("Failed to get left mesh asset");
        }
    } else {
        tracing::warn!("Left wall entity not found");
    }

    // Update right channel mesh
    if let Ok(mesh_handle) = right_query.get_single() {
        tracing::info!("Generating right channel mesh...");
        let right_samples = waveform.right_channel();
        let mesh = generate_channel_mesh(&right_samples, waveform.sample_rate, &config);

        if let Some(mesh_asset) = meshes.get_mut(&mesh_handle.0) {
            *mesh_asset = mesh;
            tracing::info!("✓ Right channel mesh updated");
        } else {
            tracing::warn!("Failed to get right mesh asset");
        }
    } else {
        tracing::warn!("Right wall entity not found");
    }
}
