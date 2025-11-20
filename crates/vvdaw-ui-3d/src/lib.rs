//! 3D Highway UI for vvdaw
//!
//! An experimental 3D interface where tracks are represented as infinite highways
//! with stereo waveforms rendered as guardrails/walls on either side.
//!
//! This is a modular UI experiment - one of potentially many 3D interface approaches.

use bevy::prelude::*;
use vvdaw_comms::UiChannels;

pub mod camera;
pub mod file_loading;
pub mod highway;
pub mod menu;
pub mod playback;
pub mod scene;
pub mod waveform;

/// Resource wrapping the command sender (UI -> Audio)
pub struct AudioCommandChannel(pub vvdaw_comms::CommandSender);

// SAFETY: Manual Send + Sync implementation required for rtrb::Producer
//
// WHY THIS IS NEEDED:
// `rtrb::Producer<T>` does not automatically implement `Sync` because it contains:
// - `std::cell::Cell<usize>` (interior mutability without synchronization)
// - `*mut T` raw pointers (not Send/Sync by default)
//
// These are implementation details of rtrb's lock-free algorithm, NOT a signal
// that the type is unsafe to use across threads.
//
// WHY THIS IS SAFE:
// 1. `rtrb::Producer` is explicitly designed for cross-thread communication
//    (single producer on one thread, single consumer on another thread)
// 2. Bevy's `Resource` system guarantees exclusive access - only one system
//    can access a resource at a time, preventing concurrent `&` or `&mut` access
// 3. The producer and consumer ends are completely separate - the audio thread
//    never touches the Producer, only the Consumer
// 4. rtrb uses atomic operations internally for thread-safe coordination
//
// This pattern is documented in Bevy community resources for wrapping SPSC channels.
#[allow(unsafe_code)]
unsafe impl Send for AudioCommandChannel {}
#[allow(unsafe_code)]
unsafe impl Sync for AudioCommandChannel {}

impl Resource for AudioCommandChannel {}

/// Resource wrapping the plugin sender (UI -> Audio)
pub struct AudioPluginChannel(pub crossbeam_channel::Sender<vvdaw_comms::PluginInstance>);

// crossbeam_channel::Sender already implements Send + Sync, so no manual impl needed
impl Resource for AudioPluginChannel {}

/// Resource containing information about the audio engine
///
/// This stores the actual sample rate the audio engine is running at,
/// which is reported via the `AudioEvent::EngineInitialized` event.
///
/// UI systems can use this to know what sample rate to resample imported
/// audio files to, ensuring they match the engine's actual rate.
#[derive(Resource, Debug, Clone, Default)]
pub struct AudioEngineInfo {
    /// The actual sample rate the audio engine is running at
    ///
    /// `None` until the `EngineInitialized` event is received.
    /// File loading should wait for this to be `Some` before proceeding.
    pub sample_rate: Option<u32>,
}

/// Plugin that sets up the 3D highway UI
pub struct Highway3dPlugin;

impl Plugin for Highway3dPlugin {
    fn build(&self, app: &mut App) {
        app
            // Initialize resources
            .init_resource::<AudioEngineInfo>()
            .init_resource::<waveform::WaveformData>()
            // Add our custom plugins
            .add_plugins(scene::ScenePlugin)
            .add_plugins(camera::CameraPlugin)
            .add_plugins(highway::HighwayPlugin)
            .add_plugins(menu::MenuPlugin)
            .add_plugins(playback::PlaybackPlugin)
            .add_plugins(file_loading::FileLoadingPlugin)
            // Add cleanup system for graceful shutdown
            .add_systems(Last, cleanup_on_exit);
    }
}

/// System to handle graceful shutdown when `AppExit` is triggered
///
/// AGGRESSIVE EXIT STRATEGY:
/// Due to potential deadlocks with audio threads and egui when paused,
/// we use `std::process::exit()` to force immediate termination.
///
/// This is not ideal but prevents the app from hanging indefinitely.
/// The audio thread and all resources will be cleaned up by the OS.
fn cleanup_on_exit(
    mut exit_events: MessageReader<AppExit>,
    mut audio_command_tx: Option<ResMut<AudioCommandChannel>>,
) {
    // Check if we're exiting
    if exit_events.read().next().is_some() {
        tracing::info!("App exit detected - forcing immediate shutdown");

        // Try to stop audio gracefully first
        if let Some(tx) = &mut audio_command_tx {
            let _ = tx.0.push(vvdaw_comms::AudioCommand::Stop);
        }

        // Give audio thread a tiny moment to process stop command
        std::thread::sleep(std::time::Duration::from_millis(10));

        tracing::info!("Forcing process exit to avoid potential deadlock");

        // AGGRESSIVE: Force immediate process termination
        // The OS will clean up all resources (audio threads, file handles, etc.)
        // This prevents hanging when the audio thread or egui is blocked
        std::process::exit(0);
    }
}

/// Create a Bevy app configured for the 3D highway UI
pub fn create_app(ui_channels: UiChannels) -> App {
    let mut app = App::new();

    // Extract channels for resources
    let command_tx = ui_channels.command_tx;
    let event_rx = ui_channels.event_rx;
    let plugin_tx = ui_channels.plugin_tx;

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "vvdaw - 3D Highway UI".to_string(),
                    resolution: (1920, 1080).into(),
                    ..default()
                }),
                ..default()
            })
            .disable::<bevy::log::LogPlugin>(), // Disable Bevy's LogPlugin - tracing is initialized in main.rs
    )
    .add_plugins(Highway3dPlugin)
    // Insert audio communication channels as resources
    .insert_resource(AudioCommandChannel(command_tx))
    .insert_resource(AudioPluginChannel(plugin_tx))
    .insert_resource(highway::AudioEventChannel(event_rx));

    app
}
