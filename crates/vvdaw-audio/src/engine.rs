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

        // Create the audio callback
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Process commands from UI thread (non-blocking)
                while let Ok(cmd) = channels.command_rx.pop() {
                    match cmd {
                        AudioCommand::Start => {
                            tracing::debug!("Audio: Start command received");
                            is_running = true;
                            let _ = channels.event_tx.push(AudioEvent::Started);
                        }
                        AudioCommand::Stop => {
                            tracing::debug!("Audio: Stop command received");
                            is_running = false;
                            let _ = channels.event_tx.push(AudioEvent::Stopped);
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

                    // For now, generate a simple test tone (440 Hz sine wave)
                    // This will be replaced with actual graph output
                    let sample_rate = config.sample_rate.0 as f32;
                    for (i, sample) in data.iter_mut().enumerate() {
                        let t = (i as f32) / sample_rate;
                        *sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.1;
                    }

                    // Send peak levels to UI (every N samples to avoid flooding)
                    // TODO: Make this more efficient with downsampling
                    if !data.is_empty() {
                        let peak = data
                            .iter()
                            .map(|s| s.abs())
                            .max_by(|a, b| a.partial_cmp(b).unwrap())
                            .unwrap_or(0.0);

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
