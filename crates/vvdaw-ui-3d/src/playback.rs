//! Playback state and control systems

use bevy::prelude::*;
use leafwing_input_manager::prelude::*;
use tracing::info;

/// Plugin that manages playback state
pub struct PlaybackPlugin;

impl Plugin for PlaybackPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputManagerPlugin::<PlaybackAction>::default())
            .init_resource::<PlaybackState>()
            .add_message::<PlaybackCommand>()
            .add_systems(Startup, setup_playback_input)
            .add_systems(Update, keyboard_input_system)
            .add_systems(Update, handle_playback_commands);
    }
}

/// Actions for playback control
#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum PlaybackAction {
    Toggle,
    Stop,
}

/// Marker component for playback controller
#[derive(Component)]
struct PlaybackController;

/// Current playback state
#[derive(Resource, Debug)]
pub struct PlaybackState {
    pub status: PlaybackStatus,
    pub current_position: f32, // In seconds
    pub total_duration: f32,   // Total track length
    pub sample_rate: u32,
    pub loaded_file: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            current_position: 0.0,
            total_duration: 0.0,
            sample_rate: 48000,
            loaded_file: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

/// Commands for controlling playback
#[derive(Debug)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Stop,
    Toggle,    // Play if stopped/paused, pause if playing
    Seek(f32), // Jump to position in seconds
}

impl Message for PlaybackCommand {}

/// Setup playback input controls
fn setup_playback_input(mut commands: Commands) {
    // Create input map for playback controls
    let input_map = InputMap::new([
        (PlaybackAction::Toggle, KeyCode::Space),
        (PlaybackAction::Stop, KeyCode::KeyX),
    ]);

    commands.spawn((PlaybackController, input_map));
}

/// System to handle keyboard input for playback controls
#[allow(clippy::needless_pass_by_value)]
fn keyboard_input_system(
    query: Query<&ActionState<PlaybackAction>, With<PlaybackController>>,
    mut commands: MessageWriter<PlaybackCommand>,
) {
    let Ok(action_state) = query.single() else {
        return;
    };

    // Space: Toggle play/pause
    if action_state.just_pressed(&PlaybackAction::Toggle) {
        commands.write(PlaybackCommand::Toggle);
    }

    // X: Stop
    if action_state.just_pressed(&PlaybackAction::Stop) {
        commands.write(PlaybackCommand::Stop);
    }
}

/// System to handle playback commands
fn handle_playback_commands(
    mut commands: MessageReader<PlaybackCommand>,
    mut state: ResMut<PlaybackState>,
) {
    for command in commands.read() {
        match command {
            PlaybackCommand::Play => {
                info!("Play command");
                state.status = PlaybackStatus::Playing;
                // TODO: Send to audio engine
            }
            PlaybackCommand::Pause => {
                info!("Pause command");
                state.status = PlaybackStatus::Paused;
                // TODO: Send to audio engine
            }
            PlaybackCommand::Stop => {
                info!("Stop command");
                state.status = PlaybackStatus::Stopped;
                state.current_position = 0.0;
                // TODO: Send to audio engine
            }
            PlaybackCommand::Toggle => {
                match state.status {
                    PlaybackStatus::Stopped | PlaybackStatus::Paused => {
                        info!("Toggle -> Play");
                        state.status = PlaybackStatus::Playing;
                    }
                    PlaybackStatus::Playing => {
                        info!("Toggle -> Pause");
                        state.status = PlaybackStatus::Paused;
                    }
                }
                // TODO: Send to audio engine
            }
            PlaybackCommand::Seek(position) => {
                info!("Seek to {position}s");
                state.current_position = position.clamp(0.0, state.total_duration);
                // TODO: Send to audio engine
            }
        }
    }
}
