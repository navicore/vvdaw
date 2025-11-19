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
            .init_resource::<FileLoadingState>()
            .init_resource::<CurrentSamplerNode>()
            .add_message::<FileSelected>()
            .add_systems(
                Update,
                (start_file_load_system, poll_file_load_system).chain(),
            );
    }
}

/// Resource for tracking file loading tasks
#[derive(Resource, Default)]
struct FileLoadTask {
    pending: Option<std::thread::JoinHandle<Result<LoadedAudio, String>>>,
}

/// Resource for tracking file loading state and errors
#[derive(Resource, Default, Clone)]
pub struct FileLoadingState {
    pub is_loading: bool,
    pub error: Option<String>,
}

/// Resource for tracking the currently loaded sampler node
#[derive(Resource, Default)]
pub struct CurrentSamplerNode {
    pub node_id: Option<usize>,
}

impl FileLoadingState {
    pub fn start_loading(&mut self) {
        self.is_loading = true;
        self.error = None;
    }

    pub fn complete_successfully(&mut self) {
        self.is_loading = false;
        self.error = None;
    }

    pub fn fail_with_error(&mut self, error: String) {
        self.is_loading = false;
        self.error = Some(error);
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }
}

/// Loaded audio data
#[derive(Debug)]
struct LoadedAudio {
    samples: Vec<f32>, // Interleaved stereo
    sample_rate: u32,
    path: PathBuf,
}

/// System that starts loading a file when selected
fn start_file_load_system(
    mut file_events: MessageReader<FileSelected>,
    mut load_task: ResMut<FileLoadTask>,
    mut loading_state: ResMut<FileLoadingState>,
) {
    for event in file_events.read() {
        // Check if a load is already in progress
        if load_task.pending.is_some() {
            info!("File load already in progress, ignoring new request");
            continue;
        }

        let path = event.0.clone();
        info!("Loading WAV file: {}", path.display());

        // Update loading state
        loading_state.start_loading();

        // Spawn background thread to load file
        let task = std::thread::spawn(move || load_wav_file(&path));
        load_task.pending = Some(task);
    }
}

/// System that polls for completed file loads
#[allow(clippy::too_many_arguments)] // Bevy system parameters
#[allow(clippy::needless_pass_by_value)] // Bevy SystemParam requires Option<Res<T>>, not Option<&Res<T>>
fn poll_file_load_system(
    mut load_task: ResMut<FileLoadTask>,
    mut waveform_data: ResMut<WaveformData>,
    mut playback_state: ResMut<PlaybackState>,
    mut loading_state: ResMut<FileLoadingState>,
    mut current_sampler: ResMut<CurrentSamplerNode>,
    mut audio_command_tx: Option<ResMut<crate::AudioCommandChannel>>,
    audio_plugin_tx: Option<Res<crate::AudioPluginChannel>>,
) {
    if let Some(task) = load_task.pending.take() {
        if task.is_finished() {
            match task.join() {
                Ok(Ok(audio)) => {
                    info!(
                        "Successfully loaded {} frames at {}Hz",
                        audio.samples.len() / 2,
                        audio.sample_rate
                    );

                    // Clone samples for audio engine (will be moved into sampler)
                    let samples_for_engine = audio.samples.clone();
                    let sample_rate = audio.sample_rate;

                    // Update waveform data (for visualization)
                    waveform_data.clear_streaming();
                    waveform_data.samples = audio.samples;
                    waveform_data.sample_rate = audio.sample_rate;
                    waveform_data.needs_mesh_update = true;

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

                    // Send sampler to audio engine
                    if let (Some(cmd_tx), Some(plugin_tx)) =
                        (&mut audio_command_tx, &audio_plugin_tx)
                    {
                        info!("Preparing to send sampler to audio engine");

                        // Step 1: Stop playback (required before modifying graph)
                        if playback_state.status == crate::playback::PlaybackStatus::Stopped {
                            info!("Playback already stopped, skipping Stop command");
                        } else {
                            info!(
                                "→ Sending Stop command (playback is {:?})",
                                playback_state.status
                            );
                            if let Err(e) = cmd_tx.0.push(vvdaw_comms::AudioCommand::Stop) {
                                error!("✗ Failed to send Stop command: {e:?}");
                            }
                            playback_state.status = crate::playback::PlaybackStatus::Stopped;
                        }

                        // Step 2: Remove old sampler node if it exists
                        if let Some(old_node_id) = current_sampler.node_id {
                            info!("→ Sending RemoveNode({old_node_id}) command");
                            if let Err(e) = cmd_tx
                                .0
                                .push(vvdaw_comms::AudioCommand::RemoveNode(old_node_id))
                            {
                                error!("✗ Failed to send RemoveNode command: {e:?}");
                            }
                            current_sampler.node_id = None;
                        } else {
                            info!("No existing sampler node to remove");
                        }

                        // Step 3: Create and send new sampler
                        info!("→ Creating new sampler processor");
                        let sampler = vvdaw_audio::builtin::sampler::SamplerProcessor::new(
                            samples_for_engine,
                            sample_rate,
                        );

                        // Send plugin instance to audio thread
                        info!("→ Sending plugin instance via crossbeam_channel");
                        if let Err(e) = plugin_tx.0.send(Box::new(sampler)) {
                            error!("✗ Failed to send sampler to audio engine: {e}");
                            loading_state.fail_with_error(format!("Audio engine error: {e}"));
                            return;
                        }

                        // Send AddNode command (audio thread will create the node)
                        info!("→ Sending AddNode command");
                        if let Err(e) = cmd_tx.0.push(vvdaw_comms::AudioCommand::AddNode) {
                            error!("✗ Failed to send AddNode command: {e:?}");
                            loading_state.fail_with_error(
                                "Failed to add sampler to audio graph".to_string(),
                            );
                            return;
                        }

                        info!("✓ All commands sent to audio engine");
                    } else {
                        tracing::warn!("Audio engine not available - visualization only");
                    }

                    // Update loading state
                    loading_state.complete_successfully();

                    info!("File loaded successfully");
                }
                Ok(Err(e)) => {
                    error!("Failed to load WAV file: {e}");
                    loading_state.fail_with_error(e);
                }
                Err(_) => {
                    let error_msg = "File loading thread panicked".to_string();
                    error!("{error_msg}");
                    loading_state.fail_with_error(error_msg);
                }
            }
        } else {
            // Put it back if not finished
            load_task.pending = Some(task);
        }
    }
}

/// Target sample rate for the audio engine
const ENGINE_SAMPLE_RATE: u32 = 48000;

/// Resample stereo audio from one sample rate to another using linear interpolation
fn resample_stereo(stereo_samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    use dasp::{Signal, interpolate::linear::Linear, signal};

    // Early validation: handle empty input
    if stereo_samples.is_empty() {
        debug!("Resample called with empty input, returning empty Vec");
        return Vec::new();
    }

    // Early validation: prevent division by zero
    if source_rate == 0 {
        error!("Source sample rate is 0, cannot resample");
        return stereo_samples.to_vec();
    }

    if target_rate == 0 {
        error!("Target sample rate is 0, cannot resample");
        return stereo_samples.to_vec();
    }

    info!(
        "Resampling from {}Hz to {}Hz ({:.1}% speed change)",
        source_rate,
        target_rate,
        ((f64::from(target_rate) / f64::from(source_rate)) - 1.0) * 100.0
    );

    // Convert interleaved stereo to stereo frames [[L, R], [L, R], ...]
    let frame_count = stereo_samples.len() / 2;
    let stereo_frames: Vec<[f32; 2]> = stereo_samples
        .chunks_exact(2)
        .map(|chunk| [chunk[0], chunk[1]])
        .collect();

    // Create signal from stereo frames
    let stereo_signal = signal::from_iter(stereo_frames.iter().copied());

    // Calculate number of output frames
    let source_rate_f64 = f64::from(source_rate);
    let target_rate_f64 = f64::from(target_rate);
    let output_frame_count =
        ((frame_count as f64) * (target_rate_f64 / source_rate_f64)).ceil() as usize;

    // Resample stereo signal using linear interpolation
    let resampled_frames: Vec<[f32; 2]> = stereo_signal
        .from_hz_to_hz(
            Linear::new([0.0, 0.0], [0.0, 0.0]), // Stereo interpolator state
            source_rate_f64,
            target_rate_f64,
        )
        .take(output_frame_count)
        .collect();

    // Convert stereo frames back to interleaved samples
    let mut resampled = Vec::with_capacity(output_frame_count * 2);
    for frame in resampled_frames {
        resampled.push(frame[0]); // Left
        resampled.push(frame[1]); // Right
    }

    info!(
        "✓ Resampled: {} → {} frames",
        frame_count, output_frame_count
    );

    resampled
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

    // Resample to engine sample rate if needed (load-time resampling)
    let (final_samples, final_sample_rate) = if sample_rate == ENGINE_SAMPLE_RATE {
        (stereo_samples, sample_rate)
    } else {
        let resampled = resample_stereo(&stereo_samples, sample_rate, ENGINE_SAMPLE_RATE);
        (resampled, ENGINE_SAMPLE_RATE)
    };

    Ok(LoadedAudio {
        samples: final_samples,
        sample_rate: final_sample_rate,
        path: canonical_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_wav_nonexistent_file() {
        let result = load_wav_file(Path::new("/nonexistent/file.wav"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("File not found"));
    }

    #[test]
    fn test_load_wav_invalid_extension() {
        // Create a temporary file with wrong extension
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_invalid.txt");
        fs::write(&test_file, b"not a wav file").unwrap();

        let result = load_wav_file(&test_file);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid file extension"));

        // Cleanup
        let _ = fs::remove_file(test_file);
    }

    #[test]
    fn test_load_wav_directory_not_file() {
        let temp_dir = std::env::temp_dir();
        let result = load_wav_file(&temp_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Path is not a file"));
    }

    #[test]
    fn test_load_wav_path_traversal_prevention() {
        // Attempting to load a file with path traversal should fail safely
        // The canonicalize step will resolve ../ and prevent traversal
        let result = load_wav_file(Path::new("../../etc/passwd"));
        // Should fail because file doesn't exist or isn't a .wav
        assert!(result.is_err());
    }

    #[test]
    fn test_load_wav_missing_extension() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_no_extension");
        fs::write(&test_file, b"data").unwrap();

        let result = load_wav_file(&test_file);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("File must have .wav extension")
        );

        // Cleanup
        let _ = fs::remove_file(test_file);
    }

    #[test]
    fn test_file_loading_state_lifecycle() {
        let mut state = FileLoadingState::default();

        // Initial state
        assert!(!state.is_loading);
        assert!(state.error.is_none());

        // Start loading
        state.start_loading();
        assert!(state.is_loading);
        assert!(state.error.is_none());

        // Complete successfully
        state.complete_successfully();
        assert!(!state.is_loading);
        assert!(state.error.is_none());

        // Start loading again
        state.start_loading();
        assert!(state.is_loading);

        // Fail with error
        state.fail_with_error("Test error".to_string());
        assert!(!state.is_loading);
        assert_eq!(state.error, Some("Test error".to_string()));

        // Clear error
        state.clear_error();
        assert!(state.error.is_none());
    }

    #[test]
    fn test_resample_stereo_empty_input() {
        let result = resample_stereo(&[], 44100, 48000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_resample_stereo_zero_source_rate() {
        // Should return input unchanged when source rate is 0 (defensive behavior)
        let input = vec![0.1, -0.1, 0.2, -0.2];
        let result = resample_stereo(&input, 0, 48000);
        assert_eq!(result, input);
    }

    #[test]
    fn test_resample_stereo_zero_target_rate() {
        // Should return input unchanged when target rate is 0 (defensive behavior)
        let input = vec![0.1, -0.1, 0.2, -0.2];
        let result = resample_stereo(&input, 44100, 0);
        assert_eq!(result, input);
    }

    #[test]
    fn test_resample_stereo_valid_upsampling() {
        // Test upsampling from 44.1kHz to 48kHz (small sample)
        let input = vec![0.5, -0.5, 0.3, -0.3]; // 2 stereo frames
        let result = resample_stereo(&input, 44100, 48000);

        // Should have more frames after upsampling (44.1 -> 48 is ~8.8% increase)
        // 2 frames * (48000/44100) ≈ 2.18 frames → ceil to 3 frames = 6 samples
        assert!(!result.is_empty());
        assert!(result.len() >= input.len());
    }

    #[test]
    fn test_resample_stereo_valid_downsampling() {
        // Test downsampling from 48kHz to 44.1kHz (small sample)
        let input = vec![0.5, -0.5, 0.3, -0.3, 0.1, -0.1]; // 3 stereo frames
        let result = resample_stereo(&input, 48000, 44100);

        // Should have fewer frames after downsampling (48 -> 44.1 is ~8.2% decrease)
        // 3 frames * (44100/48000) ≈ 2.75 frames → ceil to 3 frames = 6 samples
        assert!(!result.is_empty());
        assert!(result.len() <= input.len() + 2); // Allow small rounding difference
    }

    #[test]
    fn test_resample_stereo_same_rate() {
        // When source and target rates are the same, should be close to input
        let input = vec![0.5, -0.5, 0.3, -0.3];
        let result = resample_stereo(&input, 48000, 48000);

        // Should have similar length (may differ slightly due to interpolation)
        assert!(!result.is_empty());
        assert!((result.len() as i32 - input.len() as i32).abs() <= 2);
    }
}
