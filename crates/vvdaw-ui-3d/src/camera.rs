//! Camera system with flight controls
//!
//! Implements Descent-style 6DOF camera movement for navigating the 3D highway.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<CameraAction>::default())
            .add_systems(Startup, setup_camera)
            .add_systems(Update, (camera_movement, camera_look).chain());
    }
}

/// Actions for camera control
#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum CameraAction {
    Forward,
    Backward,
    Left,
    Right,
    Up,
    Down,
    SpeedBoost,
    Look,
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
    // Create input map for camera controls
    let input_map = InputMap::new([
        (CameraAction::Forward, KeyCode::KeyW),
        (CameraAction::Backward, KeyCode::KeyS),
        (CameraAction::Left, KeyCode::KeyA),
        (CameraAction::Right, KeyCode::KeyD),
        (CameraAction::Up, KeyCode::KeyQ),
        (CameraAction::Down, KeyCode::KeyE),
        (CameraAction::SpeedBoost, KeyCode::ShiftLeft),
    ])
    .with(CameraAction::Look, MouseButton::Right);

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
        input_map,
        // TODO: Add fog once we figure out the right Bevy 0.15 API
        // Fog is in the pbr module but not publicly exported in a simple way
    ));
}

/// Handle camera movement (WASD + QE for up/down)
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn camera_movement(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &FlightCamera, &ActionState<CameraAction>)>,
) {
    for (mut transform, camera, action_state) in &mut query {
        let mut velocity = Vec3::ZERO;
        let forward = *transform.forward();
        let right = *transform.right();
        let up = Vec3::Y;

        // Forward/Backward (W/S)
        if action_state.pressed(&CameraAction::Forward) {
            velocity += forward;
        }
        if action_state.pressed(&CameraAction::Backward) {
            velocity -= forward;
        }

        // Left/Right (A/D)
        if action_state.pressed(&CameraAction::Left) {
            velocity -= right;
        }
        if action_state.pressed(&CameraAction::Right) {
            velocity += right;
        }

        // Up/Down (Q/E)
        if action_state.pressed(&CameraAction::Up) {
            velocity += up;
        }
        if action_state.pressed(&CameraAction::Down) {
            velocity -= up;
        }

        // Speed boost with Shift
        let speed_multiplier = if action_state.pressed(&CameraAction::SpeedBoost) {
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
    mut mouse_motion: MessageReader<MouseMotion>,
    mut query: Query<(
        &mut Transform,
        &mut FlightCamera,
        &ActionState<CameraAction>,
    )>,
) {
    for motion in mouse_motion.read() {
        for (mut transform, mut camera, action_state) in &mut query {
            // Only look around when Look action (right mouse button) is held
            if !action_state.pressed(&CameraAction::Look) {
                continue;
            }

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
