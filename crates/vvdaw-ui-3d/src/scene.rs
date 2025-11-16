//! Scene setup - lighting, atmosphere, and environment

use bevy::prelude::*;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene);
    }
}

/// Setup the basic 3D scene with lighting and atmosphere
fn setup_scene(mut commands: Commands) {
    // Ambient light - dark but not black (matching the concept images)
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.05, 0.08, 0.1), // Very dark blue-teal ambient
        brightness: 100.0,
    });

    // Directional light (simulating moonlight or distant atmospheric glow)
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.2, 0.4, 0.5), // Cyan-tinted light
            illuminance: 5000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(10.0, 50.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // TODO: Add volumetric fog when we have the highway geometry
    // Bevy's fog is configured per-camera in the camera module
}
