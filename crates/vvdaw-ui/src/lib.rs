//! Bevy-based UI and visualization.
//!
//! This crate provides the 2D/3D visual interface using Bevy.

use bevy::prelude::*;
use std::sync::{Arc, Mutex};
use vvdaw_comms::{AudioCommand, AudioEvent, UiChannels};

mod ui;

pub use ui::{AudioState, PlaybackState};

/// Main UI plugin for Bevy
pub struct VvdawUiPlugin {
    /// Audio communication channels (wrapped for thread safety)
    pub audio_channels: Arc<Mutex<UiChannels>>,
}

impl VvdawUiPlugin {
    /// Create a new UI plugin with audio channels
    pub fn new(channels: UiChannels) -> Self {
        Self {
            audio_channels: Arc::new(Mutex::new(channels)),
        }
    }
}

impl Plugin for VvdawUiPlugin {
    fn build(&self, app: &mut App) {
        // Insert audio channels as a resource
        app.insert_resource(AudioChannelResource {
            channels: self.audio_channels.clone(),
        });

        // Add UI systems
        app.add_systems(Startup, ui::setup_ui).add_systems(
            Update,
            (
                ui::handle_button_interactions,
                ui::update_file_path_text,
                ui::poll_audio_events,
                ui::poll_file_dialog,
            ),
        );
    }
}

/// Resource holding the audio communication channels
#[derive(Resource, Clone)]
pub struct AudioChannelResource {
    pub channels: Arc<Mutex<UiChannels>>,
}

impl AudioChannelResource {
    /// Send a command to the audio thread
    pub fn send_command(&self, command: AudioCommand) -> Result<(), String> {
        let mut channels = self.channels.lock().map_err(|e| e.to_string())?;
        channels
            .command_tx
            .push(command)
            .map_err(|_| "Command channel full".to_string())
    }

    /// Send a plugin instance to the audio thread
    pub fn send_plugin(&self, plugin: vvdaw_comms::PluginInstance) -> Result<(), String> {
        let channels = self.channels.lock().map_err(|e| e.to_string())?;
        channels
            .plugin_tx
            .send(plugin)
            .map_err(|_| "Plugin channel disconnected".to_string())
    }

    /// Poll for events from the audio thread
    pub fn poll_events(&self) -> Vec<AudioEvent> {
        let Ok(mut channels) = self.channels.lock() else {
            return vec![];
        };

        let mut events = Vec::new();
        while let Ok(event) = channels.event_rx.pop() {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder test - to be replaced when UI implementation begins
    }
}
