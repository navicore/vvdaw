//! Highway geometry - road surface and waveform walls
//!
//! Creates the "infinite highway" where:
//! - The road surface represents the timeline
//! - Left/right walls represent stereo waveforms (guardrails)

use crate::waveform::{WaveformData, WaveformMeshConfig, generate_channel_mesh};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use vvdaw_comms::{AudioEvent, EventReceiver};

/// Resource wrapping the audio event receiver channel
pub struct AudioEventChannel(pub EventReceiver);

// SAFETY: This is safe because:
// 1. EventReceiver (rtrb::Consumer) is specifically designed for lock-free single-consumer use
// 2. Bevy guarantees single-threaded access to Resources (no concurrent access)
// 3. The audio thread (producer) and UI thread (consumer) never access the same end
// 4. rtrb uses atomic operations internally for thread-safe communication
#[allow(unsafe_code)]
unsafe impl Send for AudioEventChannel {}
#[allow(unsafe_code)]
unsafe impl Sync for AudioEventChannel {}

impl Resource for AudioEventChannel {}

pub struct HighwayPlugin;

impl Plugin for HighwayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_highway)
            .add_systems(Update, process_audio_events)
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
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
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
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
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

/// Update waveform wall meshes when waveform data changes
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn update_waveform_meshes(
    mut waveform: ResMut<WaveformData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    left_query: Query<(Entity, &Mesh3d), With<LeftWall>>,
    right_query: Query<(Entity, &Mesh3d), With<RightWall>>,
) {
    const TARGET_LENGTH: f32 = 400.0;

    // Debug: Always log the flag state every frame
    if waveform.needs_mesh_update {
        tracing::info!("🚨 MESH UPDATE REQUESTED - needs_mesh_update flag is TRUE");
        tracing::info!("🚨 Waveform has {} frames loaded", waveform.frame_count());
    }

    // Only update if mesh update is requested
    if !waveform.needs_mesh_update {
        return;
    }

    if !waveform.is_loaded() {
        tracing::warn!("Mesh update requested but no waveform data loaded");
        waveform.needs_mesh_update = false; // Clear flag
        return;
    }

    let frame_count = waveform.frame_count();
    let duration_secs = frame_count as f32 / waveform.sample_rate as f32;
    tracing::info!(
        "🎨 Waveform changed detected! Updating meshes from {} frames ({:.1}s duration)",
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
    if let Ok((entity, _mesh_handle)) = left_query.single() {
        tracing::info!("Generating left channel mesh...");
        let left_samples = waveform.left_channel();
        tracing::info!("Left channel has {} samples", left_samples.len());

        let mesh = generate_channel_mesh(&left_samples, waveform.sample_rate, &config);
        tracing::info!(
            "Generated left mesh with vertex count: {:?}",
            mesh.count_vertices()
        );

        // Add new mesh to assets and update entity's component
        let new_handle = meshes.add(mesh);
        commands.entity(entity).insert(Mesh3d(new_handle));
        tracing::info!("✓ Left channel mesh created and assigned to entity");
    } else {
        tracing::warn!("❌ Left wall entity not found");
    }

    // Update right channel mesh
    if let Ok((entity, _mesh_handle)) = right_query.single() {
        tracing::info!("Generating right channel mesh...");
        let right_samples = waveform.right_channel();
        tracing::info!("Right channel has {} samples", right_samples.len());

        let mesh = generate_channel_mesh(&right_samples, waveform.sample_rate, &config);
        tracing::info!(
            "Generated right mesh with vertex count: {:?}",
            mesh.count_vertices()
        );

        // Add new mesh to assets and update entity's component
        let new_handle = meshes.add(mesh);
        commands.entity(entity).insert(Mesh3d(new_handle));
        tracing::info!("✓ Right channel mesh created and assigned to entity");
    } else {
        tracing::warn!("❌ Right wall entity not found");
    }

    // Clear the update flag
    waveform.needs_mesh_update = false;
    tracing::info!("✓ Mesh update complete - flag cleared");
}

/// Process audio events from the audio thread
///
/// Reads waveform sample events and updates the `WaveformData` resource
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn process_audio_events(
    event_channel: Option<ResMut<AudioEventChannel>>,
    mut waveform: ResMut<WaveformData>,
) {
    // Early return if audio event channel is not available (e.g., in basic examples)
    let Some(mut channel) = event_channel else {
        return;
    };

    // Process all available audio events (non-blocking)
    while let Ok(event) = channel.0.pop() {
        match event {
            AudioEvent::WaveformSample {
                position,
                left_peak,
                right_peak,
            } => {
                // Update waveform data with new streaming sample
                waveform.push_streaming_peak(position, left_peak, right_peak);
            }
            AudioEvent::Started => {
                tracing::info!("Audio playback started");
            }
            AudioEvent::Stopped => {
                tracing::info!("Audio playback stopped");
            }
            AudioEvent::Error(msg) => {
                tracing::error!("Audio error: {}", msg);
            }
            _ => {
                // Ignore other events (NodeAdded, PeakLevel, etc.)
            }
        }
    }
}
