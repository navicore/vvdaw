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

// SAFETY: This is safe because:
// 1. CommandSender (rtrb::Producer) is specifically designed for lock-free single-producer use
// 2. Bevy guarantees single-threaded access to Resources (no concurrent access)
// 3. The UI thread (producer) and audio thread (consumer) never access the same end
// 4. rtrb uses atomic operations internally for thread-safe communication
#[allow(unsafe_code)]
unsafe impl Send for AudioCommandChannel {}
#[allow(unsafe_code)]
unsafe impl Sync for AudioCommandChannel {}

impl Resource for AudioCommandChannel {}

/// Resource wrapping the plugin sender (UI -> Audio)
pub struct AudioPluginChannel(pub crossbeam_channel::Sender<vvdaw_comms::PluginInstance>);

// SAFETY: This is safe because:
// 1. crossbeam_channel::Sender is explicitly Send + Sync by design
// 2. Bevy Resources are accessed by only one system at a time
// 3. The Sender is designed for multi-producer scenarios (even safer for single-producer)
#[allow(unsafe_code)]
unsafe impl Send for AudioPluginChannel {}
#[allow(unsafe_code)]
unsafe impl Sync for AudioPluginChannel {}

impl Resource for AudioPluginChannel {}

/// Plugin that sets up the 3D highway UI
pub struct Highway3dPlugin;

impl Plugin for Highway3dPlugin {
    fn build(&self, app: &mut App) {
        app
            // Initialize resources
            .init_resource::<waveform::WaveformData>()
            // Add our custom plugins
            .add_plugins(scene::ScenePlugin)
            .add_plugins(camera::CameraPlugin)
            .add_plugins(highway::HighwayPlugin)
            .add_plugins(menu::MenuPlugin)
            .add_plugins(playback::PlaybackPlugin)
            .add_plugins(file_loading::FileLoadingPlugin);
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
