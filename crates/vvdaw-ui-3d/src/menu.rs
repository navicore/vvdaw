//! Menu bar system for 3D UI
//!
//! Provides File menu for loading WAV files and controlling playback.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use rfd::FileDialog;
use std::path::PathBuf;
use tracing::info;

/// Plugin that adds menu bar to the 3D UI
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .init_resource::<FileDialogState>()
            .add_systems(EguiPrimaryContextPass, menu_bar_system)
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
) -> Result {
    egui::TopBottomPanel::top("menu_bar").show(contexts.ctx_mut()?, |ui| {
        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Load WAV...").clicked() {
                    // Spawn file dialog in background thread
                    let task = std::thread::spawn(|| {
                        FileDialog::new()
                            .add_filter("WAV Audio", &["wav"])
                            .set_title("Load WAV File")
                            .pick_file()
                    });
                    file_dialog.pending_task = Some(task);
                    ui.close();
                }

                ui.separator();

                if ui.button("Exit").clicked() {
                    app_exit.write(AppExit::Success);
                }
            });

            ui.menu_button("Playback", |ui| {
                if ui.button("Play/Pause  [Space]").clicked() {
                    // TODO: Send play/pause command
                    ui.close();
                }

                if ui.button("Stop  [S]").clicked() {
                    // TODO: Send stop command
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
