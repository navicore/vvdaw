//! Highway geometry - road surface and waveform walls
//!
//! Creates the "infinite highway" where:
//! - The road surface represents the timeline
//! - Left/right walls represent stereo waveforms (guardrails)

use bevy::prelude::*;

pub struct HighwayPlugin;

impl Plugin for HighwayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_highway);
    }
}

/// Highway visual configuration
const ROAD_WIDTH: f32 = 20.0;
const ROAD_LENGTH: f32 = 500.0;
const WALL_HEIGHT: f32 = 10.0;
const WALL_THICKNESS: f32 = 0.5;

/// Setup the highway geometry
fn setup_highway(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Grid road surface
    let road_mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::new(ROAD_WIDTH, ROAD_LENGTH)));
    let road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.1, 0.15), // Dark grid surface
        metallic: 0.3,
        perceptual_roughness: 0.8,
        emissive: LinearRgba::new(0.0, 0.05, 0.08, 1.0), // Slight cyan glow
        ..default()
    });

    commands.spawn((
        Mesh3d(road_mesh),
        MeshMaterial3d(road_material),
        Transform::from_xyz(0.0, 0.0, -ROAD_LENGTH / 2.0),
    ));

    // Left wall (will become left channel waveform)
    create_wall(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(-ROAD_WIDTH / 2.0, WALL_HEIGHT / 2.0, -ROAD_LENGTH / 2.0),
        Vec3::new(WALL_THICKNESS, WALL_HEIGHT, ROAD_LENGTH),
        Color::srgb(0.0, 0.8, 1.0), // Cyan (left channel)
    );

    // Right wall (will become right channel waveform)
    create_wall(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(ROAD_WIDTH / 2.0, WALL_HEIGHT / 2.0, -ROAD_LENGTH / 2.0),
        Vec3::new(WALL_THICKNESS, WALL_HEIGHT, ROAD_LENGTH),
        Color::srgb(0.0, 1.0, 0.8), // Cyan-green (right channel, slightly different)
    );

    // TODO: Replace static walls with dynamic waveform geometry
    // For now, we have placeholder walls to prove the rendering works
}

/// Helper to create a wall segment
fn create_wall(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    size: Vec3,
    color: Color,
) {
    let mesh = meshes.add(Cuboid::new(size.x, size.y, size.z));
    let material = materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::new(
            color.to_linear().red * 0.5,
            color.to_linear().green * 0.5,
            color.to_linear().blue * 0.5,
            1.0,
        ), // Emissive glow
        metallic: 0.0,
        perceptual_roughness: 0.7,
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(position),
    ));
}
