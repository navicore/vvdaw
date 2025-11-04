//! Audio engine - manages audio thread and cpal integration.

use crate::{AudioConfig, AudioGraph};
use anyhow::{Context, Result};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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

        // Create the audio graph
        let mut graph = AudioGraph::new();

        // Flag to track if we're running
        let mut is_running = false;

        // Phase accumulator for test tone (prevents discontinuities)
        let mut phase: f32 = 0.0;
        let frequency = 440.0; // A4 note
        let sample_rate_f32 = config.sample_rate.0 as f32;
        let phase_increment = frequency * 2.0 * std::f32::consts::PI / sample_rate_f32;

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
                            tracing::debug!("Audio: Start command received");
                            is_running = true;
                            // Note: If event queue is full, we drop the event rather than block.
                            // This is acceptable in real-time audio - we cannot wait.
                            if channels.event_tx.push(AudioEvent::Started).is_err() {
                                // Can't log here - logging may allocate!
                                // The UI will detect the state change via other means (peak levels)
                            }
                        }
                        AudioCommand::Stop => {
                            tracing::debug!("Audio: Stop command received");
                            is_running = false;
                            if channels.event_tx.push(AudioEvent::Stopped).is_err() {
                                // Event queue full - acceptable to drop in real-time context
                            }
                        }
                        AudioCommand::SetParameter(node_id, param_id, value) => {
                            tracing::trace!(
                                "Audio: SetParameter {} {} {}",
                                node_id,
                                param_id,
                                value
                            );
                            // TODO: Set parameter on node
                        }
                        AudioCommand::AddNode => {
                            tracing::debug!("Audio: AddNode command received");
                            // TODO: Add node to graph
                        }
                        AudioCommand::RemoveNode(node_id) => {
                            tracing::debug!("Audio: RemoveNode {} command received", node_id);
                            // TODO: Remove node from graph
                        }
                        AudioCommand::Connect { from, to } => {
                            tracing::debug!("Audio: Connect {} -> {} command received", from, to);
                            // TODO: Connect nodes in graph
                        }
                        AudioCommand::Disconnect { from, to } => {
                            tracing::debug!(
                                "Audio: Disconnect {} -> {} command received",
                                from,
                                to
                            );
                            // TODO: Disconnect nodes in graph
                        }
                    }
                }

                if is_running {
                    // Process the audio graph
                    graph.process();

                    // Generate test tone (440 Hz sine wave)
                    // Phase is accumulated across buffers to prevent discontinuities
                    for sample in data.iter_mut() {
                        *sample = phase.sin() * 0.1;
                        phase += phase_increment;

                        // Wrap phase to prevent float precision loss over time
                        if phase >= 2.0 * std::f32::consts::PI {
                            phase -= 2.0 * std::f32::consts::PI;
                        }
                    }

                    // Send peak levels to UI (every N samples to avoid flooding)
                    // TODO: Make this more efficient with downsampling
                    if !data.is_empty() {
                        let peak = data
                            .iter()
                            .map(|s| s.abs())
                            .max_by(|a, b| a.partial_cmp(b).unwrap())
                            .unwrap_or(0.0);

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

    #[test]
    fn test_engine_start_stop() {
        // Create engine with default config
        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        // Create communication channels
        let (_ui_channels, audio_channels) = create_channels(256);

        // Start the engine
        assert!(engine.start(audio_channels).is_ok());

        // Give it a moment to initialize
        std::thread::sleep(Duration::from_millis(100));

        // Stop the engine
        assert!(engine.stop().is_ok());
    }

    #[test]
    fn test_command_event_flow() {
        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        let (mut ui_channels, audio_channels) = create_channels(256);

        // Start the engine
        engine.start(audio_channels).unwrap();

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
    fn test_peak_level_events() {
        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        let (mut ui_channels, audio_channels) = create_channels(256);

        engine.start(audio_channels).unwrap();

        // Start audio playback
        ui_channels.command_tx.push(AudioCommand::Start).unwrap();

        // Wait for some audio to be generated
        std::thread::sleep(Duration::from_millis(500));

        // Check for peak level events
        let mut received_peak = false;
        while let Ok(event) = ui_channels.event_rx.pop() {
            if let AudioEvent::PeakLevel { level, .. } = event {
                // Test tone should have non-zero peaks
                if level > 0.0 {
                    received_peak = true;
                }
            }
        }
        assert!(
            received_peak,
            "Should receive peak level events with non-zero values"
        );

        engine.stop().unwrap();
    }

    #[test]
    fn test_multiple_start_stop_cycles() {
        let config = AudioConfig::default();
        let mut engine = AudioEngine::new(config);

        let (mut ui_channels, audio_channels) = create_channels(256);

        engine.start(audio_channels).unwrap();

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
