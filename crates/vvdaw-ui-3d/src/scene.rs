//! Scene setup - cinematic lighting, atmosphere, and environment
//!
//! Implements physically-based lighting for industrial/photographic look:
//! - Directional sun (warm, low angle, dramatic shadows)
//! - Ambient environment lighting (cool tint for contrast)
//! - Optional fill light to control shadow depth

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

use crate::highway::ROAD_LENGTH;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene);
    }
}

/// Setup cinematic 3D scene with PBR lighting
fn setup_scene(mut commands: Commands) {
    // --- Environment / Ambient Lighting ---
    // Moderate ambient for outdoor industrial scene (cool tint)
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.6, 0.7, 0.8), // Cool blue-ish tint
        brightness: 300.0,                 // Moderate ambient for outdoor scene
        affects_lightmapped_meshes: true,
    });

    // --- Directional Key Light (Sun) ---
    // Low angle for dramatic shadows across highway and walls
    // Warm tint for cinematic contrast with cool ambient
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.95, 0.85), // Warm golden sunlight
            illuminance: 5000.0, // Bright overcast day (between OVERCAST_DAY and full sun)
            shadows_enabled: true, // Re-enabled with NotShadowCaster on custom meshes
            shadow_depth_bias: 0.02, // Prevent shadow acne
            shadow_normal_bias: 0.6, // Prevent peter-panning
            ..default()
        },
        // Low angle from side - creates long dramatic shadows
        Transform::from_xyz(-30.0, 25.0, -20.0).looking_at(Vec3::ZERO, Vec3::Y),
        // Configure shadow cascades for our highway scene
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 50.0, // First cascade covers near highway
            maximum_distance: ROAD_LENGTH,  // Match highway length
            ..default()
        }
        .build(),
    ));

    // --- Fill Light (Optional) ---
    // Subtle fill to lift deep shadows without losing contrast
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.7, 0.75, 0.8), // Cool fill light
            illuminance: 1500.0,                // Moderate fill intensity
            shadows_enabled: false,             // No shadows from fill
            ..default()
        },
        // Opposite side from key light
        Transform::from_xyz(20.0, 15.0, 15.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // TODO: Add HDR skybox for image-based lighting
    // TODO: Add volumetric fog for atmospheric depth
}
