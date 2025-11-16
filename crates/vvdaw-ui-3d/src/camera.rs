//! Camera system with flight controls
//!
//! Implements Descent-style 6DOF camera movement for navigating the 3D highway.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_camera)
            .add_systems(Update, (camera_movement, camera_look).chain());
    }
}

/// Component marking the flight camera
#[derive(Component)]
pub struct FlightCamera {
    /// Movement speed (units per second)
    pub speed: f32,
    /// Rotation speed (radians per pixel)
    pub look_sensitivity: f32,
    /// Current pitch angle (radians)
    pub pitch: f32,
    /// Current yaw angle (radians)
    pub yaw: f32,
}

impl Default for FlightCamera {
    fn default() -> Self {
        Self {
            speed: 20.0,
            look_sensitivity: 0.003,
            pitch: 0.0,
            yaw: 0.0,
        }
    }
}

/// Setup the camera at the starting position
fn setup_camera(mut commands: Commands) {
    // Position camera to view the waveforms
    // - Behind and above the start of the highway
    // - Looking forward down the highway (negative Z direction)
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.05, 0.08)), // Very dark blue
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 20.0).looking_at(Vec3::new(0.0, 5.0, -50.0), Vec3::Y),
        FlightCamera::default(),
        // TODO: Add fog once we figure out the right Bevy 0.15 API
        // Fog is in the pbr module but not publicly exported in a simple way
    ));
}

/// Handle camera movement (WASD + QE for up/down)
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn camera_movement(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<(&mut Transform, &FlightCamera)>,
) {
    for (mut transform, camera) in &mut query {
        let mut velocity = Vec3::ZERO;
        let forward = *transform.forward();
        let right = *transform.right();
        let up = Vec3::Y;

        // Forward/Backward (W/S)
        if keyboard.pressed(KeyCode::KeyW) {
            velocity += forward;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            velocity -= forward;
        }

        // Left/Right (A/D)
        if keyboard.pressed(KeyCode::KeyA) {
            velocity -= right;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            velocity += right;
        }

        // Up/Down (Q/E)
        if keyboard.pressed(KeyCode::KeyQ) {
            velocity += up;
        }
        if keyboard.pressed(KeyCode::KeyE) {
            velocity -= up;
        }

        // Speed boost with Shift
        let speed_multiplier = if keyboard.pressed(KeyCode::ShiftLeft) {
            3.0
        } else {
            1.0
        };

        // Apply movement
        let delta =
            velocity.normalize_or_zero() * camera.speed * speed_multiplier * time.delta_secs();
        transform.translation += delta;
    }
}

/// Handle camera look (mouse movement)
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn camera_look(
    mut mouse_motion: EventReader<MouseMotion>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut query: Query<(&mut Transform, &mut FlightCamera)>,
) {
    // Only look around when right mouse button is held
    if !mouse_button.pressed(MouseButton::Right) {
        return;
    }

    for motion in mouse_motion.read() {
        for (mut transform, mut camera) in &mut query {
            // Update yaw and pitch
            camera.yaw -= motion.delta.x * camera.look_sensitivity;
            camera.pitch -= motion.delta.y * camera.look_sensitivity;

            // Clamp pitch to avoid gimbal lock
            camera.pitch = camera.pitch.clamp(-1.5, 1.5);

            // Apply rotation
            transform.rotation = Quat::from_euler(
                EulerRot::YXZ,
                camera.yaw,
                camera.pitch,
                0.0, // No roll
            );
        }
    }
}
