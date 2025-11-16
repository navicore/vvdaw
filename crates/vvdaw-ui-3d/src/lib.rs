//! 3D Highway UI for vvdaw
//!
//! An experimental 3D interface where tracks are represented as infinite highways
//! with stereo waveforms rendered as guardrails/walls on either side.
//!
//! This is a modular UI experiment - one of potentially many 3D interface approaches.

use bevy::prelude::*;

pub mod camera;
pub mod highway;
pub mod scene;

/// Plugin that sets up the 3D highway UI
pub struct Highway3dPlugin;

impl Plugin for Highway3dPlugin {
    fn build(&self, app: &mut App) {
        app
            // Add our custom plugins
            .add_plugins(scene::ScenePlugin)
            .add_plugins(camera::CameraPlugin)
            .add_plugins(highway::HighwayPlugin);
    }
}

/// Create a Bevy app configured for the 3D highway UI
pub fn create_app() -> App {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "vvdaw - 3D Highway UI".to_string(),
            resolution: (1920.0, 1080.0).into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(Highway3dPlugin);

    app
}
