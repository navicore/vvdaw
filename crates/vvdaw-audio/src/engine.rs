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

        // Configure the output stream
        let config = cpal::StreamConfig {
            channels: self.config.output_channels as u16,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Fixed(self.config.block_size as u32),
        };

        tracing::debug!("Stream config: {:?}", config);

        // Create the audio graph with proper configuration
        let mut graph = AudioGraph::with_config(config.sample_rate.0, self.config.block_size);

        // Flag to track if we're running
        let mut is_running = false;

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
                            // REAL-TIME SAFE: No tracing in audio callback
                            // Receive the plugin instance from the plugin channel
                            if let Ok(plugin) = channels.plugin_rx.try_recv() {
                                // Ignore errors - can't log in audio callback
                                let _ = graph.add_node(plugin);
                            }
                        }
                        AudioCommand::RemoveNode(node_id) => {
                            // REAL-TIME SAFE: No tracing in audio callback
                            graph.remove_node(node_id);
                        }
                        AudioCommand::Connect { from, to } => {
                            // REAL-TIME SAFE: No tracing in audio callback
                            // Ignore errors - can't log in audio callback
                            let _ = graph.connect(from, to);
                        }
                        AudioCommand::Disconnect { from, to } => {
                            // REAL-TIME SAFE: No tracing in audio callback
                            graph.disconnect(from, to);
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

                    // Send peak levels to UI (every N samples to avoid flooding)
                    // TODO: Make this more efficient with downsampling
                    if !data.is_empty() {
                        // Use fold instead of max_by to avoid panic on NaN
                        let peak = data.iter().fold(0.0_f32, |max, &s| max.max(s.abs()));

                        // Drop peak events if queue is full - they're informational only
                        // and will be replaced by the next buffer's peaks anyway
                        let _ = channels.event_tx.push(AudioEvent::PeakLevel {
                            channel: 0,
                            level: peak,
                        });
                    }
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
