//! UI components and systems

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use crossbeam_channel::{Receiver, Sender};
use futures_lite::future;
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
    /// Currently active sampler node ID (None if no sampler loaded)
    ///
    /// This tracks the sampler added by the UI to prevent the race condition
    /// where rapid next/prev clicking adds multiple samplers to the graph.
    /// Before adding a new sampler, we remove the previous one.
    pub current_sampler_node: Option<usize>,
}

/// File path state resource
#[derive(Resource)]
pub struct FilePathState {
    pub current_path: String,
    pub loaded_files: Vec<String>,
}

impl Default for FilePathState {
    fn default() -> Self {
        Self {
            current_path: "No file selected - click Browse to load a WAV file".to_string(),
            loaded_files: Vec::new(),
        }
    }
}

/// Channel for receiving file paths selected from the file dialog
#[derive(Resource)]
pub struct FileDialogChannel {
    pub sender: Sender<String>,
    pub receiver: Receiver<String>,
}

impl Default for FileDialogChannel {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }
}

/// Result of async WAV file loading
type WavLoadResult = Result<(Vec<f32>, u32, String), String>;

/// Resource tracking pending async WAV file loads
///
/// WAV files are loaded asynchronously to prevent UI freezing on large files (up to 500MB).
/// Each task runs on Bevy's `AsyncComputeTaskPool` and returns the loaded samples when complete.
#[derive(Resource, Default)]
pub struct PendingWavLoads {
    /// Active background loading tasks
    /// Each task returns `(samples, sample_rate, file_path)` or error message
    pub tasks: Vec<Task<WavLoadResult>>,
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

/// Marker component for the browse button
#[derive(Component)]
pub struct BrowseButton;

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
        Option<&'static BrowseButton>,
    ),
    (Changed<Interaction>, With<Button>),
>;

/// Setup the UI
#[allow(clippy::too_many_lines)]
pub fn setup_ui(mut commands: Commands) {
    // Insert resources
    commands.insert_resource(AudioState::default());
    commands.insert_resource(FilePathState::default());
    commands.insert_resource(FileDialogChannel::default());
    commands.insert_resource(PendingWavLoads::default());

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
                            // Browse button
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
                                    BorderColor::all(Color::BLACK),
                                    BackgroundColor(NORMAL_BUTTON),
                                    BrowseButton,
                                ))
                                .with_children(|parent| {
                                    parent.spawn((
                                        Text::new("Browse..."),
                                        TextFont {
                                            font_size: 20.0,
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.9, 0.9, 0.9)),
                                    ));
                                });

                            // Previous file button
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
                                    BorderColor::all(Color::BLACK),
                                    BackgroundColor(NORMAL_BUTTON),
                                    PrevFileButton,
                                ))
                                .with_children(|parent| {
                                    parent.spawn((
                                        Text::new("< Prev"),
                                        TextFont {
                                            font_size: 20.0,
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.9, 0.9, 0.9)),
                                    ));
                                });

                            // Next file button
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
                                    BorderColor::all(Color::BLACK),
                                    BackgroundColor(NORMAL_BUTTON),
                                    NextFileButton,
                                ))
                                .with_children(|parent| {
                                    parent.spawn((
                                        Text::new("Next >"),
                                        TextFont {
                                            font_size: 20.0,
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.9, 0.9, 0.9)),
                                    ));
                                });
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
                            BorderColor::all(Color::BLACK),
                            BackgroundColor(NORMAL_BUTTON),
                            PlayButton,
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                Text::new("Play"),
                                TextFont {
                                    font_size: 20.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.9, 0.9, 0.9)),
                            ));
                        });

                    // Stop button
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
                            BorderColor::all(Color::BLACK),
                            BackgroundColor(NORMAL_BUTTON),
                            StopButton,
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                Text::new("Stop"),
                                TextFont {
                                    font_size: 20.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.9, 0.9, 0.9)),
                            ));
                        });
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

/// Handle button interactions
#[allow(clippy::needless_pass_by_value)]
pub fn handle_button_interactions(
    mut interaction_query: ButtonInteractionQuery,
    audio_channels: Res<AudioChannelResource>,
    mut audio_state: ResMut<AudioState>,
    mut file_state: ResMut<FilePathState>,
    file_dialog_channel: Res<FileDialogChannel>,
    mut pending_loads: ResMut<PendingWavLoads>,
) {
    for (interaction, mut color, play, stop, next, prev, browse) in &mut interaction_query {
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

                // Handle browse button
                if browse.is_some() {
                    tracing::info!("Browse button pressed - opening file dialog");
                    audio_state.status_message = "Opening file dialog...".to_string();

                    // Spawn file dialog in background thread to avoid blocking UI
                    let sender = file_dialog_channel.sender.clone();
                    std::thread::spawn(move || {
                        if let Some(file_path) = rfd::FileDialog::new()
                            .add_filter("WAV Audio", &["wav"])
                            .set_title("Select WAV File")
                            .pick_file()
                        {
                            let path_string = file_path.display().to_string();
                            let _ = sender.send(path_string);
                        }
                    });
                }

                // Handle next file button
                if next.is_some() && !file_state.loaded_files.is_empty() {
                    // Find current file in history
                    if let Some(current_idx) = file_state
                        .loaded_files
                        .iter()
                        .position(|f| f == &file_state.current_path)
                    {
                        let next_idx = (current_idx + 1) % file_state.loaded_files.len();
                        let path = file_state.loaded_files[next_idx].clone();

                        // Update current path
                        file_state.current_path.clone_from(&path);

                        // Load WAV file asynchronously
                        load_and_send_wav(&path, &mut pending_loads, &mut audio_state);
                    }
                }

                // Handle previous file button
                if prev.is_some() && !file_state.loaded_files.is_empty() {
                    // Find current file in history
                    if let Some(current_idx) = file_state
                        .loaded_files
                        .iter()
                        .position(|f| f == &file_state.current_path)
                    {
                        let prev_idx = if current_idx == 0 {
                            file_state.loaded_files.len() - 1
                        } else {
                            current_idx - 1
                        };
                        let path = file_state.loaded_files[prev_idx].clone();

                        // Update current path
                        file_state.current_path.clone_from(&path);

                        // Load WAV file asynchronously
                        load_and_send_wav(&path, &mut pending_loads, &mut audio_state);
                    }
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
            AudioEvent::NodeAdded { node_id } => {
                tracing::debug!("Node added to graph with ID: {node_id}");
                // Track this as the current sampler (assuming UI only adds samplers)
                audio_state.current_sampler_node = Some(node_id);
            }
            AudioEvent::NodeRemoved { node_id } => {
                tracing::debug!("Node removed from graph: {node_id}");
            }
            AudioEvent::WaveformSample { .. } => {
                // Waveform samples are handled by 3D visualization, ignore in 2D UI
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

/// Poll for file paths selected from the file dialog
#[allow(clippy::needless_pass_by_value)]
pub fn poll_file_dialog(
    file_dialog_channel: Res<FileDialogChannel>,
    mut audio_state: ResMut<AudioState>,
    mut file_state: ResMut<FilePathState>,
    mut pending_loads: ResMut<PendingWavLoads>,
) {
    // Non-blocking check for file dialog results
    while let Ok(path_string) = file_dialog_channel.receiver.try_recv() {
        // Update file state
        file_state.current_path.clone_from(&path_string);

        // Add to loaded files history if not already there
        if !file_state.loaded_files.contains(&path_string) {
            file_state.loaded_files.push(path_string.clone());
        }

        // Load asynchronously in background thread
        load_and_send_wav(&path_string, &mut pending_loads, &mut audio_state);
    }
}

/// Poll for completed async WAV loading tasks and send processors to audio thread
///
/// This system checks all pending async tasks and processes completed ones.
/// Completed tasks are removed from the pending list.
#[allow(clippy::needless_pass_by_value)]
pub fn poll_wav_load_tasks(
    mut pending_loads: ResMut<PendingWavLoads>,
    audio_channels: Res<AudioChannelResource>,
    mut audio_state: ResMut<AudioState>,
) {
    // Check each pending task for completion (non-blocking)
    let mut completed_indices = Vec::new();

    for (idx, task) in pending_loads.tasks.iter_mut().enumerate() {
        // Non-blocking poll - returns Some if task is ready
        if let Some(result) = future::block_on(future::poll_once(task)) {
            completed_indices.push(idx);

            match result {
                Ok((samples, sample_rate, path)) => {
                    tracing::info!(
                        "WAV load task completed: {} frames at {}Hz",
                        samples.len() / 2,
                        sample_rate
                    );

                    // Create sampler processor with loaded audio
                    let processor = Box::new(vvdaw_audio::builtin::sampler::SamplerProcessor::new(
                        samples,
                        sample_rate,
                    ));

                    // Remove old sampler if one exists to prevent multiple samplers
                    // This prevents the race condition where rapid next/prev clicking
                    // adds multiple samplers to the graph.
                    if let Some(old_node_id) = audio_state.current_sampler_node {
                        tracing::debug!("Removing old sampler node {old_node_id}");
                        if let Err(e) =
                            audio_channels.send_command(AudioCommand::RemoveNode(old_node_id))
                        {
                            tracing::warn!("Failed to remove old sampler: {e}");
                            // Continue anyway - the new sampler will still be added
                        }
                        audio_state.current_sampler_node = None;
                    }

                    // Send sampler to audio thread
                    if let Err(e) = audio_channels.send_plugin(processor) {
                        tracing::error!("Failed to send sampler to audio thread: {e}");
                        audio_state.status_message = format!("Error: {e}");
                        continue;
                    }

                    // Send AddNode command
                    // The audio thread will send back a NodeAdded event with the ID,
                    // which we'll handle in poll_audio_events to track the new sampler
                    if let Err(e) = audio_channels.send_command(AudioCommand::AddNode) {
                        tracing::error!("Failed to send AddNode command: {e}");
                        audio_state.status_message = format!("Error: {e}");
                    } else {
                        audio_state.status_message = format!(
                            "Loaded: {}",
                            std::path::Path::new(&path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file")
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("WAV load task failed: {e}");
                    audio_state.status_message = format!("Load error: {e}");
                }
            }
        }
    }

    // Remove completed tasks (iterate in reverse to maintain indices)
    for idx in completed_indices.into_iter().rev() {
        drop(pending_loads.tasks.swap_remove(idx));
    }
}

/// Load a WAV file asynchronously and queue it for sending to the audio thread
///
/// This spawns a background task to load the file, preventing UI freezing on large files.
/// The actual sending happens in `poll_wav_load_tasks` when the task completes.
fn load_and_send_wav(
    path: &str,
    pending_loads: &mut PendingWavLoads,
    audio_state: &mut AudioState,
) {
    tracing::info!("Starting async WAV file load: {path}");
    audio_state.status_message = format!(
        "Loading: {}...",
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
    );

    // Spawn async task on compute thread pool
    let path_owned = path.to_string();
    let task_pool = AsyncComputeTaskPool::get();
    let task = task_pool.spawn(async move {
        // Load WAV file in background thread
        match load_wav_file(&path_owned) {
            Ok((samples, sample_rate)) => {
                tracing::info!(
                    "Async load complete: {} frames at {}Hz",
                    samples.len() / 2,
                    sample_rate
                );
                Ok((samples, sample_rate, path_owned))
            }
            Err(e) => {
                tracing::error!("Async load failed: {e}");
                Err(e)
            }
        }
    });

    pending_loads.tasks.push(task);
}

/// Load a WAV file and convert to interleaved stereo f32 samples
///
/// Returns (samples, `sample_rate`) where samples is [L, R, L, R, ...]
///
/// # Safety Limits
///
/// - Maximum file size: 500MB (prevents UI freezing on multi-GB files)
/// - Bit depth: 1-31 bits (prevents integer overflow)
/// - Validates file exists and is readable
fn load_wav_file(path: &str) -> Result<(Vec<f32>, u32), String> {
    // Validate file size before loading (500MB limit)
    const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500MB
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Failed to read file metadata: {e}"))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "File too large: {:.1}MB (max 500MB). Large files should be streamed, not loaded entirely into memory.",
            metadata.len() as f64 / (1024.0 * 1024.0)
        ));
    }

    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("Failed to open WAV file: {e}"))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Validate bit depth to prevent integer overflow
    if spec.bits_per_sample == 0 || spec.bits_per_sample > 31 {
        return Err(format!(
            "Unsupported bit depth: {} bits (supported: 1-31)",
            spec.bits_per_sample
        ));
    }

    tracing::debug!(
        "WAV spec: {} channels, {}Hz, {} bits",
        channels,
        sample_rate,
        spec.bits_per_sample
    );

    // Read all samples and convert to f32
    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read samples: {e}"))?,
        hound::SampleFormat::Int => {
            // Convert integer samples to f32 [-1.0, 1.0]
            // Safe: bits_per_sample is validated to be 1-31 above
            let max_value = (1_i32 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_value))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read samples: {e}"))?
        }
    };

    // Convert to interleaved stereo
    let stereo_samples = match channels {
        1 => {
            // Mono: duplicate to both channels
            let mut stereo = Vec::with_capacity(raw_samples.len() * 2);
            for sample in raw_samples {
                stereo.push(sample); // Left
                stereo.push(sample); // Right
            }
            stereo
        }
        2 => {
            // Already stereo
            raw_samples
        }
        _ => {
            // More than 2 channels: take first 2
            tracing::warn!("WAV file has {} channels, using only first 2", channels);
            let mut stereo = Vec::with_capacity((raw_samples.len() / channels) * 2);
            for chunk in raw_samples.chunks(channels) {
                stereo.push(chunk[0]); // Left
                stereo.push(chunk[1]); // Right
            }
            stereo
        }
    };

    Ok((stereo_samples, sample_rate))
}
