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

// SAFETY: Manual Send + Sync implementation required for rtrb::Consumer
//
// WHY THIS IS NEEDED:
// `rtrb::Consumer<T>` does not automatically implement `Sync` because it contains:
// - `std::cell::Cell<usize>` (interior mutability without synchronization)
// - `*mut T` raw pointers (not Send/Sync by default)
//
// These are implementation details of rtrb's lock-free algorithm, NOT a signal
// that the type is unsafe to use across threads.
//
// WHY THIS IS SAFE:
// 1. `rtrb::Consumer` is explicitly designed for cross-thread communication
//    (single producer on one thread, single consumer on another thread)
// 2. Bevy's `Resource` system guarantees exclusive access - only one system
//    can access a resource at a time, preventing concurrent `&` or `&mut` access
// 3. The producer and consumer ends are completely separate - the UI thread
//    never touches the Producer, only the Consumer
// 4. rtrb uses atomic operations internally for thread-safe coordination
//
// This pattern is documented in Bevy community resources for wrapping SPSC channels.
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
            .add_systems(Update, update_waveform_meshes)
            .add_systems(Update, update_playback_position);
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
    // Position at edge of road (ROAD_WIDTH is half_size, so full width is 2*ROAD_WIDTH)
    // Offset by half the waveform width so inner edge aligns with road edge
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
        Transform::from_xyz(-ROAD_WIDTH - 0.25, 0.0, 0.0), // -20.25 for waveform width 0.5
        LeftWall,
    ));

    // Right channel wall (placeholder) - bright red
    // Position at edge of road (ROAD_WIDTH is half_size, so full width is 2*ROAD_WIDTH)
    // Offset by half the waveform width so inner edge aligns with road edge
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
        Transform::from_xyz(ROAD_WIDTH + 0.25, 0.0, 0.0), // 20.25 for waveform width 0.5
        RightWall,
    ));
}

/// Update waveform wall meshes dynamically as playback advances
///
/// Creates a scrolling waveform window that follows the playback position.
/// Throttles updates to only regenerate meshes when position changes significantly.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn update_waveform_meshes(
    mut waveform: ResMut<WaveformData>,
    playback: Res<crate::playback::PlaybackState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    left_query: Query<(Entity, &Mesh3d), With<LeftWall>>,
    right_query: Query<(Entity, &Mesh3d), With<RightWall>>,
) {
    // Throttle mesh updates: only regenerate if position changed significantly
    // or if force update is requested (e.g., new file loaded)
    const UPDATE_THRESHOLD: f32 = 0.1; // Update every 0.1 seconds of playback

    // Only update if waveform is loaded
    if !waveform.is_loaded() {
        if waveform.needs_mesh_update {
            waveform.needs_mesh_update = false;
        }
        return;
    }

    // Get current playback position in seconds
    let current_position = playback.current_position;

    let position_delta = (current_position - waveform.last_mesh_position).abs();

    if !waveform.needs_mesh_update && position_delta < UPDATE_THRESHOLD {
        return; // Skip update - position hasn't changed enough
    }

    // Configuration for scrolling waveform window
    let config = WaveformMeshConfig {
        sample_stride: 20, // Higher stride for better performance with scrolling
        amplitude_scale: 20.0,
        base_height: 15.0,
        window_duration: 15.0, // Show 15 seconds before and after cursor
        ..Default::default()
    };

    // Update left channel mesh
    if let Ok((entity, _mesh_handle)) = left_query.single() {
        let left_samples = waveform.left_channel();
        let mesh = generate_channel_mesh(
            &left_samples,
            waveform.sample_rate,
            current_position,
            &config,
        );
        let new_handle = meshes.add(mesh);
        commands.entity(entity).insert(Mesh3d(new_handle));
    }

    // Update right channel mesh
    if let Ok((entity, _mesh_handle)) = right_query.single() {
        let right_samples = waveform.right_channel();
        let mesh = generate_channel_mesh(
            &right_samples,
            waveform.sample_rate,
            current_position,
            &config,
        );
        let new_handle = meshes.add(mesh);
        commands.entity(entity).insert(Mesh3d(new_handle));
    }

    // Update tracking and clear flags
    waveform.last_mesh_position = current_position;
    waveform.needs_mesh_update = false;
}

/// Process audio events from the audio thread
///
/// Reads audio events and updates resources
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn process_audio_events(
    event_channel: Option<ResMut<AudioEventChannel>>,
    mut waveform: ResMut<WaveformData>,
    mut current_sampler: ResMut<crate::file_loading::CurrentSamplerNode>,
    mut engine_info: ResMut<crate::AudioEngineInfo>,
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
            AudioEvent::EngineInitialized { sample_rate } => {
                tracing::info!("✓ Audio engine initialized at {}Hz", sample_rate);
                engine_info.sample_rate = Some(sample_rate);
            }
            AudioEvent::NodeAdded { node_id } => {
                tracing::info!("✓ Sampler node added with ID: {node_id}");
                current_sampler.node_id = Some(node_id);
            }
            AudioEvent::NodeRemoved { node_id } => {
                tracing::info!("✓ Sampler node removed: {node_id}");
            }
            AudioEvent::Error(msg) => {
                tracing::error!("Audio error: {}", msg);
            }
            AudioEvent::PeakLevel { .. } => {
                // Ignore peak levels for now
            }
        }
    }
}

/// Update playback position from waveform data
///
/// Converts the frame position from `WaveformData` to seconds in `PlaybackState`
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn update_playback_position(
    waveform: Res<WaveformData>,
    mut playback: ResMut<crate::playback::PlaybackState>,
) {
    // Only update if we have valid sample rate (avoid division by zero)
    if waveform.sample_rate > 0 {
        // Convert frame position to seconds
        // Position is in frames, sample_rate is frames per second
        let position_seconds = waveform.current_position as f32 / waveform.sample_rate as f32;
        playback.current_position = position_seconds;
    }
}
