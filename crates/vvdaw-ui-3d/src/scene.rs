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
    // Bright neutral ambient light for clear colors
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 500.0,
        affects_lightmapped_meshes: true,
    });

    // Bright white directional light
    commands.spawn((
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 10000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(10.0, 50.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // TODO: Add volumetric fog when we have the highway geometry
    // Bevy's fog is configured per-camera in the camera module
}
