//! Offline WAV file processor
//!
//! Processes WAV files through VST3 plugins in offline mode (non-real-time).
//! This is useful for testing, validation, and batch processing.

use anyhow::{Context, Result};
use clap::Parser;
use hound::{WavReader, WavWriter};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vvdaw_audio::graph::{AudioGraph, PluginSource};
use vvdaw_audio::session::Session;
use vvdaw_plugin::{AudioBuffer, EventBuffer, Plugin};
use vvdaw_vst3::MultiProcessPlugin;

/// Offline WAV file processor
#[derive(Parser, Debug)]
#[command(name = "vvdaw-process")]
#[command(about = "Process WAV files through VST3 plugins", long_about = None)]
struct Args {
    /// Input WAV file
    #[arg(short, long, required_unless_present_any = ["inspect", "save_session"])]
    input: Option<PathBuf>,

    /// Output WAV file
    #[arg(short, long, required_unless_present_any = ["inspect", "save_session"])]
    output: Option<PathBuf>,

    /// VST3 plugin path (.vst3 bundle)
    #[arg(short, long, required_unless_present_any = ["inspect", "session"])]
    plugin: Option<PathBuf>,

    /// Processing block size (default: 512)
    #[arg(short, long, default_value_t = 512)]
    block_size: usize,

    /// Set plugin parameters (format: "ParamName=value")
    /// Values should be in the parameter's natural range (e.g., 0.0-1.0 for normalized params).
    /// The value will be clamped to the parameter's min/max range if out of bounds.
    /// Can be specified multiple times.
    #[arg(long = "param")]
    params: Vec<String>,

    /// Load session file (.ron) instead of specifying plugin
    #[arg(short, long, conflicts_with = "plugin")]
    session: Option<PathBuf>,

    /// Save current configuration as a session file
    #[arg(long)]
    save_session: Option<PathBuf>,

    /// Inspect plugin parameters and info (don't process audio)
    #[arg(long, conflicts_with_all = ["input", "output", "session"])]
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

    // Route to appropriate mode
    if args.inspect {
        let plugin_path = args
            .plugin
            .as_ref()
            .context("--plugin required for --inspect")?;
        inspect_plugin(plugin_path)
    } else if args.save_session.is_some() {
        save_session_mode(&args)
    } else if args.session.is_some() {
        process_with_session(&args)
    } else {
        process_with_plugin(&args)
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

/// Apply parameter settings to plugin
fn apply_parameters(plugin: &mut dyn Plugin, param_specs: &[String]) -> Result<()> {
    // Get all available parameters
    let parameters = plugin.parameters();

    for spec in param_specs {
        // Parse "Name=value" format
        let parts: Vec<&str> = spec.splitn(2, '=').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid parameter format '{spec}'. Expected 'Name=value'");
        }

        let param_name = parts[0].trim();
        let value_str = parts[1].trim();

        // Parse value as f32
        let value: f32 = value_str
            .parse()
            .with_context(|| format!("Invalid parameter value '{value_str}' (must be a number)"))?;

        // Find parameter by name
        let param = parameters
            .iter()
            .find(|p| p.name == param_name)
            .with_context(|| format!("Parameter '{param_name}' not found in plugin"))?;

        // Validate value is in range
        if value < param.min_value || value > param.max_value {
            tracing::warn!(
                "Parameter '{}' value {:.3} is outside range [{:.3}, {:.3}], clamping",
                param_name,
                value,
                param.min_value,
                param.max_value
            );
        }

        let clamped_value = value.clamp(param.min_value, param.max_value);

        // Set the parameter
        plugin
            .set_parameter(param.id, clamped_value)
            .with_context(|| format!("Failed to set parameter '{param_name}'"))?;

        tracing::info!(
            "Set parameter '{}' (ID {}) = {:.3}",
            param_name,
            param.id,
            clamped_value
        );
    }

    Ok(())
}

/// Save session mode - configure plugin and save session without processing
fn save_session_mode(args: &Args) -> Result<()> {
    let plugin_path = args
        .plugin
        .as_ref()
        .context("--plugin required when using --save-session")?;
    let session_path = args
        .save_session
        .as_ref()
        .context("--save-session path required")?;

    tracing::info!("Creating session from plugin configuration");
    tracing::info!("Plugin: {}", plugin_path.display());
    tracing::info!("Session: {}", session_path.display());

    // Load plugin
    let mut plugin =
        MultiProcessPlugin::spawn(plugin_path).context("Failed to spawn plugin subprocess")?;

    plugin
        .initialize(48000, args.block_size)
        .context("Failed to initialize plugin")?;

    tracing::info!("Plugin loaded: {}", plugin.info().name);

    // Apply parameters if specified
    if !args.params.is_empty() {
        tracing::info!("Setting {} parameter(s)...", args.params.len());
        apply_parameters(&mut plugin, &args.params)?;
    }

    // Create graph with single plugin
    let mut graph = AudioGraph::with_config(48000, args.block_size);
    let source = PluginSource::Vst3 {
        path: plugin_path.clone(),
    };

    graph
        .add_node(Box::new(plugin), source)
        .context("Failed to add plugin to graph")?;

    // Create and save session
    let session = Session::from_graph(&graph, "Saved Configuration")
        .context("Failed to create session from graph")?;

    session
        .save(session_path)
        .context("Failed to save session")?;

    tracing::info!("✓ Session saved to {}", session_path.display());
    println!("Session saved successfully: {}", session_path.display());

    Ok(())
}

/// Process audio using a session file
fn process_with_session(args: &Args) -> Result<()> {
    let input = args
        .input
        .as_ref()
        .context("--input required for processing")?;
    let output = args
        .output
        .as_ref()
        .context("--output required for processing")?;
    let session_path = args.session.as_ref().context("--session path required")?;

    tracing::info!("vvdaw-process: Processing with session");
    tracing::info!("Input:   {}", input.display());
    tracing::info!("Output:  {}", output.display());
    tracing::info!("Session: {}", session_path.display());

    // Load session
    let session = Session::load(session_path).context("Failed to load session")?;

    tracing::info!("Session loaded: {}", session.name);
    tracing::info!(
        "  Sample rate: {} Hz, Block size: {} frames",
        session.sample_rate,
        session.block_size
    );
    tracing::info!("  {} node(s) in graph", session.graph.nodes.len());

    // Read input WAV
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

    // Read samples
    let samples = read_wav_samples(&mut reader, &spec)?;
    let channel_count = spec.channels as usize;

    // Reconstruct graph from session
    tracing::info!("Reconstructing audio graph from session...");
    let mut graph = session
        .to_graph(|plugin_spec| {
            use vvdaw_audio::session::PluginSpec;
            match plugin_spec {
                PluginSpec::Vst3 { path, .. } => {
                    tracing::info!("  Loading plugin: {}", path.display());
                    let plugin = MultiProcessPlugin::spawn(path)
                        .map_err(|e| format!("Failed to spawn plugin: {e}"))?;
                    Ok(Box::new(plugin) as Box<dyn Plugin>)
                }
            }
        })
        .context("Failed to reconstruct graph from session")?;

    tracing::info!("Graph reconstructed with {} node(s)", graph.nodes().count());

    // Process audio
    tracing::info!("Processing audio...");
    let output_samples =
        process_audio_with_graph(&samples, channel_count, &mut graph, args.block_size);

    // Write output
    tracing::info!("Writing output WAV file...");
    write_wav(output, &output_samples, spec)?;

    tracing::info!("✓ Done! Output written to {}", output.display());
    println!("Processing complete: {}", output.display());

    Ok(())
}

/// Process audio file through single plugin (original mode)
fn process_with_plugin(args: &Args) -> Result<()> {
    let input = args
        .input
        .as_ref()
        .context("--input required for processing")?;
    let output = args
        .output
        .as_ref()
        .context("--output required for processing")?;
    let plugin_path = args
        .plugin
        .as_ref()
        .context("--plugin required when not using --session")?;

    tracing::info!("vvdaw-process: Offline WAV processor");
    tracing::info!("Input:  {}", input.display());
    tracing::info!("Output: {}", output.display());
    tracing::info!("Plugin: {}", plugin_path.display());
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

    // Read samples
    let samples = read_wav_samples(&mut reader, &spec)?;
    let channel_count = spec.channels as usize;
    let frame_count = samples.len() / channel_count;

    tracing::info!("Read {} frames ({} samples)", frame_count, samples.len());

    // Load VST3 plugin
    tracing::info!("Loading VST3 plugin...");
    let mut plugin =
        MultiProcessPlugin::spawn(plugin_path).context("Failed to spawn plugin subprocess")?;

    plugin
        .initialize(spec.sample_rate, args.block_size)
        .context("Failed to initialize plugin")?;

    tracing::info!("Plugin loaded: {}", plugin.info().name);
    tracing::info!(
        "Plugin channels: {} in, {} out",
        plugin.input_channels(),
        plugin.output_channels()
    );

    // Apply parameter settings
    if !args.params.is_empty() {
        tracing::info!("Setting {} parameter(s)...", args.params.len());
        apply_parameters(&mut plugin, &args.params)?;
    }

    // Process audio in blocks
    tracing::info!("Processing audio...");
    let output_samples = process_audio(&samples, channel_count, &mut plugin, args.block_size)?;

    // Write output WAV
    tracing::info!("Writing output WAV file...");
    write_wav(output, &output_samples, spec)?;

    tracing::info!("Done! Output written to {:?}", output);

    Ok(())
}

/// Read WAV samples and convert to f32
fn read_wav_samples(
    reader: &mut WavReader<std::io::BufReader<std::fs::File>>,
    spec: &hound::WavSpec,
) -> Result<Vec<f32>> {
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

    Ok(samples)
}

/// Process audio through an `AudioGraph` in offline mode
fn process_audio_with_graph(
    input_samples: &[f32],
    channel_count: usize,
    graph: &mut AudioGraph,
    block_size: usize,
) -> Vec<f32> {
    let frame_count = input_samples.len() / channel_count;
    let mut output_samples = vec![0.0_f32; input_samples.len()];

    // Deinterleave input samples into per-channel buffers
    let mut input_buffers: Vec<Vec<f32>> = vec![vec![0.0; block_size]; channel_count];
    let mut output_buffers: Vec<Vec<f32>> = vec![vec![0.0; block_size]; channel_count];

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

        // Prepare input/output references (take only current_block_size)
        let input_refs: Vec<&[f32]> = input_buffers
            .iter()
            .map(|buf| &buf[..current_block_size])
            .collect();
        let mut output_refs: Vec<&mut [f32]> = output_buffers
            .iter_mut()
            .map(|buf| &mut buf[..current_block_size])
            .collect();

        // Process through graph
        graph.process(&input_refs, &mut output_refs);

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
    output_samples
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
