//! WAV file loading system
//!
//! Handles loading WAV files from disk and converting them to waveform data.

use bevy::prelude::*;
use hound::WavReader;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use crate::menu::FileSelected;
use crate::playback::PlaybackState;
use crate::waveform::WaveformData;

/// Plugin that handles file loading
pub struct FileLoadingPlugin;

impl Plugin for FileLoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FileLoadTask>()
            .add_message::<FileSelected>()
            .add_systems(
                Update,
                (start_file_load_system, poll_file_load_system).chain(),
            )
            .add_systems(Update, debug_waveform_state);
    }
}

/// Debug system to print waveform state periodically
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters must be passed by value
fn debug_waveform_state(waveform: Res<crate::waveform::WaveformData>, time: Res<Time>) {
    // Print every 2 seconds
    if (time.elapsed_secs() as u32).is_multiple_of(2) && time.delta_secs() < 0.1 {
        info!(
            "🔍 Waveform debug: loaded={}, frames={}, needs_update={}",
            waveform.is_loaded(),
            waveform.frame_count(),
            waveform.needs_mesh_update
        );
    }
}

/// Resource for tracking file loading tasks
#[derive(Resource, Default)]
struct FileLoadTask {
    pending: Option<std::thread::JoinHandle<Result<LoadedAudio, String>>>,
}

/// Loaded audio data
struct LoadedAudio {
    samples: Vec<f32>, // Interleaved stereo
    sample_rate: u32,
    path: PathBuf,
}

/// System that starts loading a file when selected
fn start_file_load_system(
    mut file_events: MessageReader<FileSelected>,
    mut load_task: ResMut<FileLoadTask>,
) {
    let event_count = file_events.len();
    if event_count > 0 {
        info!(
            "📬 start_file_load_system: {} FileSelected messages available",
            event_count
        );
    }

    for event in file_events.read() {
        let path = event.0.clone();
        info!("📂 File selected: {}", path.display());
        info!("🔄 Starting background WAV file load...");

        // Spawn background thread to load file
        let task = std::thread::spawn(move || load_wav_file(&path));
        load_task.pending = Some(task);
    }
}

/// System that polls for completed file loads
fn poll_file_load_system(
    mut load_task: ResMut<FileLoadTask>,
    mut waveform_data: ResMut<WaveformData>,
    mut playback_state: ResMut<PlaybackState>,
) {
    if let Some(task) = load_task.pending.take() {
        info!("🔄 poll_file_load_system: Task found, checking if finished...");
        if task.is_finished() {
            info!("✅ Task finished, joining thread...");
            match task.join() {
                Ok(Ok(audio)) => {
                    info!("🎵 Thread join succeeded with audio data!");
                    info!(
                        "✅ Successfully loaded {} frames at {}Hz",
                        audio.samples.len() / 2,
                        audio.sample_rate
                    );

                    // Update waveform data
                    info!("📊 Updating WaveformData resource...");

                    // Clear any existing streaming data
                    waveform_data.clear_streaming();

                    // Update samples and sample rate
                    waveform_data.samples = audio.samples;
                    waveform_data.sample_rate = audio.sample_rate;

                    // Request mesh update
                    waveform_data.needs_mesh_update = true;

                    info!(
                        "📊 WaveformData updated: {} frames at {}Hz",
                        waveform_data.frame_count(),
                        waveform_data.sample_rate
                    );
                    info!("📊 Mesh update requested - needs_mesh_update flag set");

                    // Update playback state
                    playback_state.loaded_file = Some(
                        audio
                            .path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string(),
                    );
                    playback_state.sample_rate = audio.sample_rate;
                    playback_state.total_duration =
                        waveform_data.frame_count() as f32 / audio.sample_rate as f32;
                    playback_state.current_position = 0.0;

                    info!("File loaded successfully");
                }
                Ok(Err(e)) => {
                    error!("Failed to load WAV file: {e}");
                }
                Err(_) => {
                    error!("File loading thread panicked");
                }
            }
        } else {
            // Put it back if not finished
            load_task.pending = Some(task);
        }
    }
}

/// Load a WAV file and return audio data
fn load_wav_file(path: &Path) -> Result<LoadedAudio, String> {
    use std::fs;

    // Validate file size (500MB limit)
    const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024;

    // Validate and sanitize path using canonicalization
    let path_obj = Path::new(path);

    // Check if file exists first (required for canonicalize)
    if !path_obj.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    // Canonicalize the path to prevent path traversal attacks
    let canonical_path =
        fs::canonicalize(path_obj).map_err(|e| format!("Failed to resolve path: {e}"))?;

    // Validate it's a file, not a directory
    if !canonical_path.is_file() {
        return Err(format!("Path is not a file: {}", canonical_path.display()));
    }

    // Validate file extension on the canonical path
    if let Some(ext) = canonical_path.extension() {
        if ext.to_str() != Some("wav") {
            return Err(format!(
                "Invalid file extension: expected .wav, got .{}",
                ext.to_string_lossy()
            ));
        }
    } else {
        return Err("File must have .wav extension".to_string());
    }
    let metadata =
        fs::metadata(&canonical_path).map_err(|e| format!("Failed to read file metadata: {e}"))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "File too large: {:.1}MB (max 500MB)",
            metadata.len() as f64 / (1024.0 * 1024.0)
        ));
    }

    let mut reader =
        WavReader::open(&canonical_path).map_err(|e| format!("Failed to open WAV file: {e}"))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Validate bit depth (support up to 32 bits for standard audio formats)
    if spec.bits_per_sample == 0 || spec.bits_per_sample > 32 {
        return Err(format!(
            "Unsupported bit depth: {} bits (supported: 1-32)",
            spec.bits_per_sample
        ));
    }

    // Read all samples and convert to f32
    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read samples: {e}"))?,
        hound::SampleFormat::Int => {
            // For 32-bit audio, we need to avoid bit shift overflow
            let max_value = if spec.bits_per_sample == 32 {
                2_147_483_648.0_f32 // 2^31
            } else {
                (1_i32 << (spec.bits_per_sample - 1)) as f32
            };
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
            warn!("WAV file has {} channels, using only first 2", channels);
            let mut stereo = Vec::with_capacity((raw_samples.len() / channels) * 2);
            for chunk in raw_samples.chunks(channels) {
                stereo.push(chunk[0]); // Left
                stereo.push(chunk[1]); // Right
            }
            stereo
        }
    };

    Ok(LoadedAudio {
        samples: stereo_samples,
        sample_rate,
        path: canonical_path,
    })
}
