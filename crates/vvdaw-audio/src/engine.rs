//! Audio engine - manages audio thread and cpal integration.

use crate::{AudioConfig, AudioGraph};
use anyhow::{Context, Result};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use smallvec::SmallVec;
use vvdaw_comms::{AudioChannels, AudioCommand, AudioEvent};

/// The audio engine manages the audio thread and cpal stream
pub struct AudioEngine {
    config: AudioConfig,
    stream: Option<Stream>,
}

impl AudioEngine {
    /// Create a new audio engine with the given configuration
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            stream: None,
        }
    }

    /// Start the audio engine with the provided communication channels
    #[allow(clippy::too_many_lines)] // Audio callback is complex by nature
    pub fn start(&mut self, mut channels: AudioChannels) -> Result<()> {
        tracing::info!("Audio engine starting with config: {:?}", self.config);

        // Get the default host
        let host = cpal::default_host();
        tracing::debug!("Using audio host: {}", host.id().name());

        // Get the default output device
        let device = host
            .default_output_device()
            .context("No output device available")?;
        tracing::info!("Using output device: {}", device.name()?);

        // Get the device's default config to see what sample rate it actually supports
        let device_config = device
            .default_output_config()
            .context("Failed to get default output config")?;
        let device_sample_rate = device_config.sample_rate().0;

        tracing::info!(
            "Device default sample rate: {}Hz (requested: {}Hz)",
            device_sample_rate,
            self.config.sample_rate
        );

        // Use the device's sample rate if it differs from our request
        let actual_sample_rate = if device_sample_rate != self.config.sample_rate {
            tracing::warn!(
                "Using device sample rate {}Hz instead of requested {}Hz to avoid resampling issues",
                device_sample_rate,
                self.config.sample_rate
            );
            device_sample_rate
        } else {
            self.config.sample_rate
        };

        // Configure the output stream with the actual sample rate
        let config = cpal::StreamConfig {
            channels: self.config.output_channels as u16,
            sample_rate: cpal::SampleRate(actual_sample_rate),
            buffer_size: cpal::BufferSize::Fixed(self.config.block_size as u32),
        };

        tracing::info!("Final stream config: {:?}", config);

        // Create the audio graph with proper configuration
        let mut graph = AudioGraph::with_config(config.sample_rate.0, self.config.block_size);

        // Flag to track if we're running
        let mut is_running = false;

        // Frame position counter for waveform synchronization
        let mut frame_position: u64 = 0;

        // Pre-allocate de-interleaved buffers for audio processing
        // IMPORTANT: Pre-allocated to max block size to avoid allocations in audio callback
        let num_channels = config.channels as usize;
        let max_frames = self.config.block_size;
        let mut channel_buffers_in: Vec<Vec<f32>> = vec![vec![0.0; max_frames]; num_channels];
        let mut channel_buffers_out: Vec<Vec<f32>> = vec![vec![0.0; max_frames]; num_channels];

        // Create the audio callback
        // SAFETY: The closure takes ownership of all captured variables (move semantics).
        // Each variable is accessed exclusively by this callback thread, preventing data races.
        // This is safe because:
        // - `channels`: Lockless ring buffers designed for single producer/single consumer
        // - `graph`, `is_running`, `phase`: Owned exclusively by this closure
        // - No shared mutable state accessed from multiple threads
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Process commands from UI thread (non-blocking)
                while let Ok(cmd) = channels.command_rx.pop() {
                    match cmd {
                        AudioCommand::Start => {
                            // REAL-TIME SAFE: No tracing in audio callback
                            is_running = true;
                            // Note: If event queue is full, we drop the event rather than block.
                            // This is acceptable in real-time audio - we cannot wait.
                            let _ = channels.event_tx.push(AudioEvent::Started);
                        }
                        AudioCommand::Stop => {
                            // REAL-TIME SAFE: No tracing in audio callback
                            is_running = false;
                            let _ = channels.event_tx.push(AudioEvent::Stopped);
                        }
                        AudioCommand::SetParameter(_node_id, _param_id, _value) => {
                            // REAL-TIME SAFE: No tracing in audio callback
                            // TODO: Set parameter on node via graph
                        }
                        AudioCommand::AddNode => {
                            // REAL-TIME SAFETY: Only modify graph when audio is stopped
                            //
                            // AudioGraph::add_node() allocates memory:
                            // - HashMap::insert (graph.rs:220)
                            // - Vec for buffers (graph.rs:363-364)
                            // - Topological sort (graph.rs:381-398)
                            //
                            // To maintain real-time safety, we only process this command
                            // when is_running == false. This PROVES the POC concept:
                            // UI and audio can coexist with strict real-time boundaries.
                            //
                            // Workflow: User stops audio → loads file → starts audio
                            // This is the expected UX for a sampler/player anyway.
                            //
                            // For production, consider triple-buffered graph or pre-allocated pools.
                            if is_running {
                                // Audio is running - reject to maintain real-time safety
                                let _ = channels.plugin_rx.try_recv(); // Drain the plugin
                                let _ = channels.event_tx.push(AudioEvent::Error(
                                    "Cannot add nodes while playing. Stop audio first.".to_string(),
                                ));
                            } else {
                                // Safe to allocate - audio is stopped
                                if let Ok(plugin) = channels.plugin_rx.try_recv()
                                    && let Ok(node_id) =
                                        graph.add_node(plugin, crate::graph::PluginSource::Unknown)
                                {
                                    let _ =
                                        channels.event_tx.push(AudioEvent::NodeAdded { node_id });
                                }
                            }
                        }
                        AudioCommand::RemoveNode(node_id) => {
                            // REAL-TIME SAFETY: Only modify graph when audio is stopped
                            //
                            // AudioGraph::remove_node() deallocates and reallocates:
                            // - HashMap::remove (graph.rs:248, 251, 252)
                            // - update_processing_order (graph.rs:255)
                            if is_running {
                                let _ = channels.event_tx.push(AudioEvent::Error(
                                    "Cannot remove nodes while playing. Stop audio first."
                                        .to_string(),
                                ));
                            } else {
                                graph.remove_node(node_id);
                                let _ = channels.event_tx.push(AudioEvent::NodeRemoved { node_id });
                            }
                        }
                        AudioCommand::Connect { from, to } => {
                            // REAL-TIME SAFETY: Only modify graph when audio is stopped
                            //
                            // AudioGraph::connect() allocates:
                            // - HashSet::insert (graph.rs:296)
                            // - update_processing_order (graph.rs:299)
                            if is_running {
                                let _ = channels.event_tx.push(AudioEvent::Error(
                                    "Cannot modify connections while playing. Stop audio first."
                                        .to_string(),
                                ));
                            } else {
                                let _ = graph.connect(from, to);
                            }
                        }
                        AudioCommand::Disconnect { from, to } => {
                            // REAL-TIME SAFETY: Only modify graph when audio is stopped
                            //
                            // AudioGraph::disconnect() allocates:
                            // - update_processing_order (graph.rs:311)
                            if is_running {
                                let _ = channels.event_tx.push(AudioEvent::Error(
                                    "Cannot modify connections while playing. Stop audio first."
                                        .to_string(),
                                ));
                            } else {
                                graph.disconnect(from, to);
                            }
                        }
                    }
                }

                if is_running {
                    // De-interleave input (silence for now - no audio input yet)
                    let frames_per_buffer = (data.len() / num_channels).min(max_frames);

                    // REAL-TIME SAFE: Only use pre-allocated buffer space
                    // If cpal gives us a larger buffer than expected, we process what fits
                    for ch_buf in &mut channel_buffers_in {
                        ch_buf[..frames_per_buffer].fill(0.0);
                    }
                    for ch_buf in &mut channel_buffers_out {
                        ch_buf[..frames_per_buffer].fill(0.0);
                    }

                    // Process the audio graph
                    // REAL-TIME SAFE: SmallVec uses stack storage for <=8 channels (no heap allocation)
                    // Covers stereo (2ch), 5.1 (6ch), and 7.1 (8ch) without allocating
                    // Scope ensures mutable references are dropped before re-interleaving
                    {
                        let input_refs: SmallVec<[&[f32]; 8]> = channel_buffers_in
                            .iter()
                            .map(|v| &v[..frames_per_buffer])
                            .collect();
                        let mut output_refs: SmallVec<[&mut [f32]; 8]> = channel_buffers_out
                            .iter_mut()
                            .map(|v| &mut v[..frames_per_buffer])
                            .collect();
                        graph.process(&input_refs, &mut output_refs);
                    } // output_refs dropped here, allowing channel_buffers_out to be accessed again

                    // Re-interleave output (only the frames we processed)
                    for (frame_idx, frame) in data
                        .chunks_exact_mut(num_channels)
                        .enumerate()
                        .take(frames_per_buffer)
                    {
                        for (ch_idx, sample) in frame.iter_mut().enumerate() {
                            if let Some(ch_buf) = channel_buffers_out.get(ch_idx) {
                                *sample = *ch_buf.get(frame_idx).unwrap_or(&0.0);
                            } else {
                                *sample = 0.0;
                            }
                        }
                    }

                    // Fill any remaining frames with silence if cpal gave us more than we can handle
                    if frames_per_buffer < data.len() / num_channels {
                        let remaining_start = frames_per_buffer * num_channels;
                        data[remaining_start..].fill(0.0);
                    }

                    // Send waveform samples to UI for visualization
                    // Compute peak values for left and right channels from interleaved data
                    if !data.is_empty() && num_channels >= 2 {
                        // For stereo: data is [L, R, L, R, ...]
                        // Compute peaks for each channel separately
                        let mut left_peak = 0.0_f32;
                        let mut right_peak = 0.0_f32;

                        for frame in data.chunks_exact(num_channels) {
                            left_peak = left_peak.max(frame[0].abs());
                            right_peak = right_peak.max(frame[1].abs());
                        }

                        // Send waveform sample event with position for synchronization
                        // Drop if queue is full - they're informational and will be replaced
                        let _ = channels.event_tx.push(AudioEvent::WaveformSample {
                            position: frame_position,
                            left_peak,
                            right_peak,
                        });
                    }

                    // Increment frame position for next buffer
                    frame_position = frame_position.wrapping_add(frames_per_buffer as u64);
                } else {
                    // Silence when not running
                    data.fill(0.0);
                }
            },
            move |err| {
                tracing::error!("Audio stream error: {}", err);
            },
            None,
        )?;

        // Start the stream
        stream.play()?;
        tracing::info!("Audio stream started");

        // Store the stream (keeps it alive)
        self.stream = Some(stream);

        Ok(())
    }

    /// Stop the audio engine
    pub fn stop(&mut self) -> Result<()> {
        tracing::info!("Audio engine stopping");

        if let Some(stream) = self.stream.take() {
            stream.pause()?;
            drop(stream);
            tracing::info!("Audio stream stopped");
        }

        Ok(())
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            tracing::error!("Error stopping audio engine: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use vvdaw_comms::create_channels;

    /// Helper to check if audio device is available
    /// Returns true if we should skip the test
    fn should_skip_audio_test() -> bool {
        // Try to create a simple test to see if audio is available
        let host = cpal::default_host();
        host.default_output_device().is_none()
    }

    #[test]
    fn test_engine_start_stop() {
        if should_skip_audio_test() {
            eprintln!("Skipping test: No audio device available (CI environment)");
            return;
        }

        // Create engine with default config
        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        // Create communication channels
        let (_ui_channels, audio_channels) = create_channels(256);

        // Start the engine - skip test if audio device can't be opened (CI)
        match engine.start(audio_channels) {
            Ok(()) => {
                // Give it a moment to initialize
                std::thread::sleep(Duration::from_millis(100));

                // Stop the engine
                assert!(engine.stop().is_ok());
            }
            Err(e) => {
                eprintln!("Skipping test: Audio device unavailable - {e}");
            }
        }
    }

    #[test]
    fn test_command_event_flow() {
        if should_skip_audio_test() {
            eprintln!("Skipping test: No audio device available (CI environment)");
            return;
        }

        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        let (mut ui_channels, audio_channels) = create_channels(256);

        // Start the engine - skip test if audio device can't be opened (CI)
        if let Err(e) = engine.start(audio_channels) {
            eprintln!("Skipping test: Audio device unavailable - {e}");
            return;
        }

        // Send Start command
        assert!(ui_channels.command_tx.push(AudioCommand::Start).is_ok());

        // Wait for processing
        std::thread::sleep(Duration::from_millis(200));

        // Check for Started event
        let mut received_started = false;
        while let Ok(event) = ui_channels.event_rx.pop() {
            if matches!(event, AudioEvent::Started) {
                received_started = true;
            }
        }
        assert!(received_started, "Should receive Started event");

        // Send Stop command
        assert!(ui_channels.command_tx.push(AudioCommand::Stop).is_ok());

        // Wait for processing
        std::thread::sleep(Duration::from_millis(200));

        // Check for Stopped event
        let mut received_stopped = false;
        while let Ok(event) = ui_channels.event_rx.pop() {
            if matches!(event, AudioEvent::Stopped) {
                received_stopped = true;
            }
        }
        assert!(received_stopped, "Should receive Stopped event");

        // Clean shutdown
        engine.stop().unwrap();
    }

    #[test]
    fn test_multiple_start_stop_cycles() {
        if should_skip_audio_test() {
            eprintln!("Skipping test: No audio device available (CI environment)");
            return;
        }

        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        let (mut ui_channels, audio_channels) = create_channels(256);

        // Start the engine - skip test if audio device can't be opened (CI)
        if let Err(e) = engine.start(audio_channels) {
            eprintln!("Skipping test: Audio device unavailable - {e}");
            return;
        }

        // Multiple cycles
        for _ in 0..3 {
            ui_channels.command_tx.push(AudioCommand::Start).unwrap();
            std::thread::sleep(Duration::from_millis(100));

            ui_channels.command_tx.push(AudioCommand::Stop).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        }

        engine.stop().unwrap();
    }
}
