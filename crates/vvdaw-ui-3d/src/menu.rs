//! Menu bar system for 3D UI
//!
//! Provides File menu for loading WAV files and controlling playback.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use rfd::FileDialog;
use std::path::PathBuf;
use tracing::info;

use crate::playback::{PlaybackCommand, PlaybackState};

/// Plugin that adds menu bar to the 3D UI
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .init_resource::<FileDialogState>()
            .add_systems(EguiPrimaryContextPass, menu_bar_system)
            .add_systems(EguiPrimaryContextPass, hud_overlay_system)
            .add_systems(Update, file_dialog_poll_system);
    }
}

/// State for tracking file dialog operations
#[derive(Resource, Default)]
struct FileDialogState {
    pending_task: Option<std::thread::JoinHandle<Option<PathBuf>>>,
}

/// Message sent when a file is selected
pub struct FileSelected(pub PathBuf);

impl Message for FileSelected {}

/// Menu bar system
fn menu_bar_system(
    mut contexts: EguiContexts,
    mut file_dialog: ResMut<FileDialogState>,
    mut app_exit: MessageWriter<AppExit>,
    mut playback_commands: MessageWriter<PlaybackCommand>,
) -> Result {
    egui::TopBottomPanel::top("menu_bar").show(contexts.ctx_mut()?, |ui| {
        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Load WAV...").clicked() {
                    // Only spawn dialog if one isn't already open
                    if file_dialog.pending_task.is_none() {
                        // Spawn file dialog in background thread
                        let task = std::thread::spawn(|| {
                            FileDialog::new()
                                .add_filter("WAV Audio", &["wav"])
                                .set_title("Load WAV File")
                                .pick_file()
                        });
                        file_dialog.pending_task = Some(task);
                    }
                    ui.close();
                }

                ui.separator();

                if ui.button("Exit").clicked() {
                    app_exit.write(AppExit::Success);
                    ui.close();
                }
            });

            ui.menu_button("Playback", |ui| {
                if ui.button("Play/Pause  [Space]").clicked() {
                    playback_commands.write(PlaybackCommand::Toggle);
                    ui.close();
                }

                if ui.button("Stop  [X]").clicked() {
                    playback_commands.write(PlaybackCommand::Stop);
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui.button("Toggle Camera Mode  [Tab]").clicked() {
                    // TODO: Toggle camera mode
                    ui.close();
                }
            });

            ui.label("|");

            if ui.button("?").clicked() {
                // TODO: Show help overlay
            }
        });
    });

    Ok(())
}

/// HUD overlay system
#[allow(clippy::needless_pass_by_value)]
fn hud_overlay_system(
    mut contexts: EguiContexts,
    playback_state: Res<PlaybackState>,
    mut loading_state: ResMut<crate::file_loading::FileLoadingState>,
) -> Result {
    egui::Window::new("Status")
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .anchor(egui::Align2::LEFT_BOTTOM, [10.0, -10.0])
        .show(contexts.ctx_mut()?, |ui| {
            ui.vertical(|ui| {
                // Loading status
                if loading_state.is_loading {
                    ui.colored_label(egui::Color32::YELLOW, "⏳ Loading...");
                }

                // Error display with dismiss button
                if let Some(error) = loading_state.error.clone() {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::RED, format!("❌ Error: {error}"));
                        if ui.small_button("✖").clicked() {
                            loading_state.clear_error();
                        }
                    });
                }

                // Playback status
                let status_text = match playback_state.status {
                    crate::playback::PlaybackStatus::Stopped => "⏹ Stopped",
                    crate::playback::PlaybackStatus::Playing => "▶ Playing",
                    crate::playback::PlaybackStatus::Paused => "⏸ Paused",
                };
                ui.label(status_text);

                // Time display
                let current_min = (playback_state.current_position / 60.0) as u32;
                let current_sec = (playback_state.current_position % 60.0) as u32;
                let total_min = (playback_state.total_duration / 60.0) as u32;
                let total_sec = (playback_state.total_duration % 60.0) as u32;

                ui.label(format!(
                    "Time: {current_min:02}:{current_sec:02} / {total_min:02}:{total_sec:02}"
                ));

                // Loaded file
                if let Some(filename) = &playback_state.loaded_file {
                    ui.label(format!("File: {filename}"));
                } else {
                    ui.label("File: None");
                }

                // Sample rate
                ui.label(format!("Sample Rate: {}Hz", playback_state.sample_rate));
            });
        });

    Ok(())
}

/// System to poll file dialog results
fn file_dialog_poll_system(
    mut file_dialog: ResMut<FileDialogState>,
    mut file_selected: MessageWriter<FileSelected>,
) {
    if let Some(task) = file_dialog.pending_task.take() {
        if task.is_finished() {
            if let Ok(Some(path)) = task.join() {
                info!("File selected: {}", path.display());
                file_selected.write(FileSelected(path));
            }
        } else {
            // Put it back if not finished
            file_dialog.pending_task = Some(task);
        }
    }
}
