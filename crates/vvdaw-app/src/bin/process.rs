//! Offline WAV file processor
//!
//! Processes WAV files through VST3 plugins in offline mode (non-real-time).
//! This is useful for testing, validation, and batch processing.

use anyhow::{Context, Result};
use clap::Parser;
use hound::{WavReader, WavWriter};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vvdaw_plugin::{AudioBuffer, EventBuffer, Plugin};
use vvdaw_vst3::MultiProcessPlugin;

/// Offline WAV file processor
#[derive(Parser, Debug)]
#[command(name = "vvdaw-process")]
#[command(about = "Process WAV files through VST3 plugins", long_about = None)]
struct Args {
    /// Input WAV file
    #[arg(short, long, required_unless_present = "inspect")]
    input: Option<PathBuf>,

    /// Output WAV file
    #[arg(short, long, required_unless_present = "inspect")]
    output: Option<PathBuf>,

    /// VST3 plugin path (.vst3 bundle)
    #[arg(short, long)]
    plugin: PathBuf,

    /// Processing block size (default: 512)
    #[arg(short, long, default_value_t = 512)]
    block_size: usize,

    /// Inspect plugin parameters and info (don't process audio)
    #[arg(long, conflicts_with_all = ["input", "output"])]
    inspect: bool,
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vvdaw=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    // Route to inspect or process mode
    if args.inspect {
        inspect_plugin(&args.plugin)
    } else {
        process_audio_file(&args)
    }
}

/// Inspect plugin parameters and information
fn inspect_plugin(plugin_path: &PathBuf) -> Result<()> {
    println!("Inspecting plugin: {}\n", plugin_path.display());

    // Load VST3 plugin
    let mut plugin =
        MultiProcessPlugin::spawn(plugin_path).context("Failed to spawn plugin subprocess")?;

    let info = plugin.info();

    // Print plugin info
    println!("Plugin Information:");
    println!("  Name:      {}", info.name);
    println!("  Vendor:    {}", info.vendor);
    println!("  Version:   {}", info.version);
    println!("  Unique ID: {}", info.unique_id);
    println!(
        "  Channels:  {} in, {} out",
        plugin.input_channels(),
        plugin.output_channels()
    );
    println!();

    // Initialize plugin (some plugins only report parameters after initialization)
    println!("Initializing plugin...");
    plugin
        .initialize(48000, 512)
        .context("Failed to initialize plugin")?;

    // Get parameters
    let parameters = plugin.parameters();

    if parameters.is_empty() {
        println!("No parameters available.");
    } else {
        println!("Parameters ({} total):", parameters.len());
        println!("{}", "=".repeat(80));

        for param in &parameters {
            println!("  [{:3}] {}", param.id, param.name);
            println!(
                "        Range:   {:.3} to {:.3}",
                param.min_value, param.max_value
            );
            println!("        Default: {:.3}", param.default_value);

            // Try to get current value
            match plugin.get_parameter(param.id) {
                Ok(value) => {
                    println!("        Current: {value:.3}");
                }
                Err(e) => {
                    println!("        Current: <unavailable> ({e})");
                }
            }
            println!();
        }
    }

    Ok(())
}

/// Process audio file through plugin
fn process_audio_file(args: &Args) -> Result<()> {
    let input = args.input.as_ref().unwrap();
    let output = args.output.as_ref().unwrap();

    tracing::info!("vvdaw-process: Offline WAV processor");
    tracing::info!("Input:  {:?}", input);
    tracing::info!("Output: {:?}", output);
    tracing::info!("Plugin: {:?}", args.plugin);
    tracing::info!("Block size: {}", args.block_size);

    // Read input WAV file
    tracing::info!("Reading input WAV file...");
    let mut reader = WavReader::open(input)
        .with_context(|| format!("Failed to open input file: {}", input.display()))?;

    let spec = reader.spec();
    tracing::info!(
        "Input format: {} Hz, {} channels, {} bits, {:?}",
        spec.sample_rate,
        spec.channels,
        spec.bits_per_sample,
        spec.sample_format
    );

    // Read samples and convert to f32
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            tracing::info!("Reading float samples...");
            reader
                .samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to read float samples")?
        }
        hound::SampleFormat::Int => {
            tracing::info!("Reading integer samples and converting to float...");
            match spec.bits_per_sample {
                16 => reader
                    .samples::<i16>()
                    .map(|s| s.map(|sample| f32::from(sample) / f32::from(i16::MAX)))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read 16-bit samples")?,
                24 => {
                    reader
                        .samples::<i32>()
                        .map(|s| s.map(|sample| sample as f32 / 8_388_608.0)) // 2^23
                        .collect::<Result<Vec<_>, _>>()
                        .context("Failed to read 24-bit samples")?
                }
                32 => reader
                    .samples::<i32>()
                    .map(|s| s.map(|sample| sample as f32 / i32::MAX as f32))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read 32-bit samples")?,
                bits => anyhow::bail!("Unsupported bit depth: {bits}"),
            }
        }
    };

    let channel_count = spec.channels as usize;
    let frame_count = samples.len() / channel_count;

    tracing::info!("Read {} frames ({} samples)", frame_count, samples.len());

    // Load VST3 plugin
    tracing::info!("Loading VST3 plugin...");
    let mut plugin =
        MultiProcessPlugin::spawn(&args.plugin).context("Failed to spawn plugin subprocess")?;

    plugin
        .initialize(spec.sample_rate, args.block_size)
        .context("Failed to initialize plugin")?;

    tracing::info!("Plugin loaded: {}", plugin.info().name);
    tracing::info!(
        "Plugin channels: {} in, {} out",
        plugin.input_channels(),
        plugin.output_channels()
    );

    // Process audio in blocks
    tracing::info!("Processing audio...");
    let output_samples = process_audio(&samples, channel_count, &mut plugin, args.block_size)?;

    // Write output WAV
    tracing::info!("Writing output WAV file...");
    write_wav(output, &output_samples, spec)?;

    tracing::info!("Done! Output written to {:?}", output);

    Ok(())
}

/// Process audio through the plugin in offline mode
fn process_audio(
    input_samples: &[f32],
    channel_count: usize,
    plugin: &mut dyn Plugin,
    block_size: usize,
) -> Result<Vec<f32>> {
    let frame_count = input_samples.len() / channel_count;
    let mut output_samples = vec![0.0_f32; input_samples.len()];

    // Deinterleave input samples into per-channel buffers
    let mut input_buffers: Vec<Vec<f32>> = vec![vec![0.0; block_size]; channel_count];
    let mut output_buffers: Vec<Vec<f32>> = vec![vec![0.0; block_size]; channel_count];

    let event_buffer = EventBuffer::new();

    let mut frames_processed = 0;
    while frames_processed < frame_count {
        let frames_remaining = frame_count - frames_processed;
        let current_block_size = frames_remaining.min(block_size);

        // Deinterleave input block
        for frame in 0..current_block_size {
            let sample_offset = (frames_processed + frame) * channel_count;
            for (ch, buf) in input_buffers.iter_mut().enumerate() {
                buf[frame] = input_samples
                    .get(sample_offset + ch)
                    .copied()
                    .unwrap_or(0.0);
            }
        }

        // Clear remaining samples in block if not full
        if current_block_size < block_size {
            for buf in &mut input_buffers {
                buf[current_block_size..].fill(0.0);
            }
        }

        // Prepare input/output references
        let input_refs: Vec<&[f32]> = input_buffers.iter().map(Vec::as_slice).collect();
        let mut output_refs: Vec<&mut [f32]> =
            output_buffers.iter_mut().map(Vec::as_mut_slice).collect();

        let mut audio = AudioBuffer {
            inputs: &input_refs,
            outputs: &mut output_refs,
            frames: current_block_size,
        };

        // Process through plugin
        plugin.process(&mut audio, &event_buffer)?;

        // Interleave output block
        for frame in 0..current_block_size {
            let sample_offset = (frames_processed + frame) * channel_count;
            for (ch, buf) in output_buffers.iter().enumerate() {
                if sample_offset + ch < output_samples.len() {
                    output_samples[sample_offset + ch] = buf[frame];
                }
            }
        }

        frames_processed += current_block_size;

        if frames_processed % (block_size * 100) == 0 {
            tracing::debug!("Processed {}/{} frames", frames_processed, frame_count);
        }
    }

    tracing::info!("Processed {} frames total", frames_processed);
    Ok(output_samples)
}

/// Write interleaved samples to WAV file
fn write_wav(path: &PathBuf, samples: &[f32], spec: hound::WavSpec) -> Result<()> {
    let mut writer = WavWriter::create(path, spec)
        .with_context(|| format!("Failed to create output file: {}", path.display()))?;

    match spec.sample_format {
        hound::SampleFormat::Float => {
            for &sample in samples {
                writer
                    .write_sample(sample)
                    .context("Failed to write float sample")?;
            }
        }
        hound::SampleFormat::Int => {
            match spec.bits_per_sample {
                16 => {
                    for &sample in samples {
                        let int_sample = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                        writer
                            .write_sample(int_sample)
                            .context("Failed to write 16-bit sample")?;
                    }
                }
                24 => {
                    for &sample in samples {
                        let int_sample = (sample.clamp(-1.0, 1.0) * 8_388_607.0) as i32; // 2^23 - 1
                        writer
                            .write_sample(int_sample)
                            .context("Failed to write 24-bit sample")?;
                    }
                }
                32 => {
                    for &sample in samples {
                        let int_sample = (sample.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
                        writer
                            .write_sample(int_sample)
                            .context("Failed to write 32-bit sample")?;
                    }
                }
                bits => anyhow::bail!("Unsupported output bit depth: {bits}"),
            }
        }
    }

    writer.finalize().context("Failed to finalize WAV file")?;
    Ok(())
}
