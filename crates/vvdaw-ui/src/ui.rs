//! UI components and systems

use bevy::prelude::*;
use vvdaw_comms::{AudioCommand, AudioEvent};

use crate::AudioChannelResource;

/// Current playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
}

/// Audio state resource
#[derive(Resource, Default)]
pub struct AudioState {
    pub playback: PlaybackState,
    pub status_message: String,
}

/// File path state resource
#[derive(Resource)]
pub struct FilePathState {
    pub current_path: String,
    pub available_files: Vec<String>,
    pub current_index: usize,
}

impl Default for FilePathState {
    fn default() -> Self {
        Self {
            current_path: "No file selected".to_string(),
            available_files: vec![
                "test_audio.wav".to_string(),
                "drums.wav".to_string(),
                "bass.wav".to_string(),
            ],
            current_index: 0,
        }
    }
}

/// Marker component for the play button
#[derive(Component)]
pub struct PlayButton;

/// Marker component for the stop button
#[derive(Component)]
pub struct StopButton;

/// Marker component for the next file button
#[derive(Component)]
pub struct NextFileButton;

/// Marker component for the previous file button
#[derive(Component)]
pub struct PrevFileButton;

/// Marker component for the file path text
#[derive(Component)]
pub struct FilePathText;

/// Marker component for the status text
#[derive(Component)]
pub struct StatusText;

const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.75, 0.35);

/// Type alias for button interaction query
type ButtonInteractionQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static mut BackgroundColor,
        Option<&'static PlayButton>,
        Option<&'static StopButton>,
        Option<&'static NextFileButton>,
        Option<&'static PrevFileButton>,
    ),
    (Changed<Interaction>, With<Button>),
>;

/// Setup the UI
pub fn setup_ui(mut commands: Commands) {
    // Insert resources
    commands.insert_resource(AudioState::default());
    commands.insert_resource(FilePathState::default());

    // Spawn a camera
    commands.spawn(Camera2d);

    // Root UI container
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("VVDAW - Simple Playback Test"),
                TextFont {
                    font_size: 40.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    margin: UiRect::all(Val::Px(20.0)),
                    ..default()
                },
            ));

            // File selection section
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    margin: UiRect::all(Val::Px(20.0)),
                    ..default()
                })
                .with_children(|parent| {
                    // File path label
                    parent.spawn((
                        Text::new("File: "),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.7, 0.7, 0.7)),
                    ));

                    // File path text
                    parent.spawn((
                        Text::new("No file selected"),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.9, 0.9, 1.0)),
                        Node {
                            margin: UiRect::all(Val::Px(10.0)),
                            ..default()
                        },
                        FilePathText,
                    ));

                    // File navigation buttons
                    parent
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            margin: UiRect::all(Val::Px(10.0)),
                            ..default()
                        })
                        .with_children(|parent| {
                            // Previous file button
                            spawn_button(parent, "< Prev", PrevFileButton);

                            // Next file button
                            spawn_button(parent, "Next >", NextFileButton);
                        });
                });

            // Playback controls section
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    margin: UiRect::all(Val::Px(20.0)),
                    ..default()
                })
                .with_children(|parent| {
                    // Play button
                    spawn_button(parent, "Play", PlayButton);

                    // Stop button
                    spawn_button(parent, "Stop", StopButton);
                });

            // Status text
            parent.spawn((
                Text::new("Status: Stopped"),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.8, 0.8)),
                Node {
                    margin: UiRect::all(Val::Px(20.0)),
                    ..default()
                },
                StatusText,
            ));
        });

    tracing::info!("UI setup complete");
}

/// Helper function to spawn a button
fn spawn_button(parent: &mut ChildBuilder, label: &str, marker: impl Component) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(150.0),
                height: Val::Px(50.0),
                margin: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor(Color::BLACK),
            BackgroundColor(NORMAL_BUTTON),
            marker,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new(label),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.9, 0.9)),
            ));
        });
}

/// Handle button interactions
#[allow(clippy::needless_pass_by_value)]
pub fn handle_button_interactions(
    mut interaction_query: ButtonInteractionQuery,
    audio_channels: Res<AudioChannelResource>,
    mut audio_state: ResMut<AudioState>,
    mut file_state: ResMut<FilePathState>,
) {
    for (interaction, mut color, play, stop, next, prev) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                *color = PRESSED_BUTTON.into();

                // Handle play button
                if play.is_some() {
                    tracing::info!("Play button pressed");
                    if let Err(e) = audio_channels.send_command(AudioCommand::Start) {
                        tracing::error!("Failed to send Start command: {e}");
                        audio_state.status_message = format!("Error: {e}");
                    } else {
                        audio_state.playback = PlaybackState::Playing;
                        audio_state.status_message = "Playing...".to_string();
                    }
                }

                // Handle stop button
                if stop.is_some() {
                    tracing::info!("Stop button pressed");
                    if let Err(e) = audio_channels.send_command(AudioCommand::Stop) {
                        tracing::error!("Failed to send Stop command: {e}");
                        audio_state.status_message = format!("Error: {e}");
                    } else {
                        audio_state.playback = PlaybackState::Stopped;
                        audio_state.status_message = "Stopped".to_string();
                    }
                }

                // Handle next file button
                if next.is_some() {
                    file_state.current_index =
                        (file_state.current_index + 1) % file_state.available_files.len();
                    file_state.current_path =
                        file_state.available_files[file_state.current_index].clone();
                    tracing::info!("Selected file: {}", file_state.current_path);
                }

                // Handle previous file button
                if prev.is_some() {
                    if file_state.current_index == 0 {
                        file_state.current_index = file_state.available_files.len() - 1;
                    } else {
                        file_state.current_index -= 1;
                    }
                    file_state.current_path =
                        file_state.available_files[file_state.current_index].clone();
                    tracing::info!("Selected file: {}", file_state.current_path);
                }
            }
            Interaction::Hovered => {
                *color = HOVERED_BUTTON.into();
            }
            Interaction::None => {
                *color = NORMAL_BUTTON.into();
            }
        }
    }
}

/// Update file path text display
#[allow(clippy::needless_pass_by_value)]
pub fn update_file_path_text(
    file_state: Res<FilePathState>,
    mut query: Query<&mut Text, With<FilePathText>>,
) {
    if file_state.is_changed() {
        for mut text in &mut query {
            text.0.clone_from(&file_state.current_path);
        }
    }
}

/// Poll for events from the audio thread and update UI state
#[allow(clippy::needless_pass_by_value)]
pub fn poll_audio_events(
    audio_channels: Res<AudioChannelResource>,
    mut audio_state: ResMut<AudioState>,
    mut status_query: Query<&mut Text, With<StatusText>>,
) {
    let events = audio_channels.poll_events();

    for event in events {
        match event {
            AudioEvent::Started => {
                tracing::info!("Audio started");
                audio_state.playback = PlaybackState::Playing;
                audio_state.status_message = "Status: Playing".to_string();
            }
            AudioEvent::Stopped => {
                tracing::info!("Audio stopped");
                audio_state.playback = PlaybackState::Stopped;
                audio_state.status_message = "Status: Stopped".to_string();
            }
            AudioEvent::Error(msg) => {
                tracing::error!("Audio error: {msg}");
                audio_state.status_message = format!("Error: {msg}");
            }
            AudioEvent::PeakLevel { channel, level } => {
                // Just log for now, we're not rendering meters yet
                if level > 0.1 {
                    tracing::trace!("Peak ch{channel}: {level:.3}");
                }
            }
        }
    }

    // Update status text if state changed
    if audio_state.is_changed() {
        for mut text in &mut status_query {
            text.0.clone_from(&audio_state.status_message);
        }
    }
}
