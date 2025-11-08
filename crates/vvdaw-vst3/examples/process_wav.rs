//! Example: Process a WAV file through a VST3 effect plugin
//!
//! This demonstrates offline processing of audio through VST3 plugins.
//! Run with: cargo run --example `process_wav` --release [`plugin_path`] [input.wav] [output.wav]

use std::io::Write;
use tracing_subscriber::EnvFilter;
use vvdaw_plugin::{AudioBuffer, EventBuffer, Plugin};

#[allow(clippy::too_many_lines)] // Example code prioritizes clarity over brevity
fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    let plugin_path = args.get(1).map_or(
        "/Library/Audio/Plug-Ins/VST3/ThingsCrusher.vst3",
        String::as_str,
    );

    let input_path = args.get(2).map_or("test_data/skillet.wav", String::as_str);

    let output_path = args.get(3).map_or("test_data/output.wav", String::as_str);

    println!("VST3 Offline Audio Processor");
    println!("============================");
    println!("Plugin: {plugin_path}");
    println!("Input:  {input_path}");
    println!("Output: {output_path}");
    println!();

    // Load the VST3 plugin
    println!("Loading plugin...");
    let mut plugin = vvdaw_vst3::load_plugin(plugin_path)?;
    println!("✓ Loaded: {}", plugin.info().name);
    println!("  Vendor: {}", plugin.info().vendor);
    println!(
        "  I/O: {} -> {}",
        plugin.input_channels(),
        plugin.output_channels()
    );

    // Load the input WAV file
    println!("\nLoading input audio...");
    let mut reader = hound::WavReader::open(input_path)?;
    let spec = reader.spec();

    println!("✓ Input WAV:");
    println!("  Sample rate: {} Hz", spec.sample_rate);
    println!("  Channels: {}", spec.channels);
    println!("  Bits per sample: {}", spec.bits_per_sample);
    println!(
        "  Duration: {:.2}s",
        reader.duration() as f32 / spec.sample_rate as f32
    );

    // Read all samples (convert to f32)
    let samples: Vec<f32> = if spec.bits_per_sample == 16 {
        reader
            .samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32768.0)
            .collect()
    } else if spec.bits_per_sample == 24 {
        reader
            .samples::<i32>()
            .map(|s| s.unwrap() as f32 / 8_388_608.0)
            .collect()
    } else {
        anyhow::bail!(
            "Unsupported bit depth: {}. Only 16-bit and 24-bit WAV files are supported.",
            spec.bits_per_sample
        );
    };

    println!("  Loaded {} samples", samples.len());

    // Initialize the plugin with WAV file's sample rate
    let sample_rate = spec.sample_rate;
    let block_size = 512; // Process in chunks of 512 frames

    println!("\nInitializing plugin at {sample_rate} Hz, block size {block_size}...");
    plugin.initialize(sample_rate, block_size)?;
    println!("✓ Plugin initialized");

    // Prepare output WAV writer
    let output_spec = hound::WavSpec {
        channels: plugin.output_channels() as u16,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output_path, output_spec)?;

    // Process audio in chunks
    println!("\nProcessing audio...");
    let input_channels = spec.channels as usize;
    let output_channels = plugin.output_channels();
    let num_frames = samples.len() / input_channels;
    let num_blocks = num_frames.div_ceil(block_size);

    let mut processed_frames = 0;

    for block_idx in 0..num_blocks {
        let start_frame = block_idx * block_size;
        let end_frame = (start_frame + block_size).min(num_frames);
        let frames_in_block = end_frame - start_frame;

        // De-interleave input samples for this block
        let mut input_buffers: Vec<Vec<f32>> = vec![vec![0.0; block_size]; input_channels];
        #[allow(clippy::needless_range_loop)] // Index-based loop is clearer for interleaving
        for frame in 0..frames_in_block {
            let sample_idx = (start_frame + frame) * input_channels;
            for ch in 0..input_channels {
                if sample_idx + ch < samples.len() {
                    input_buffers[ch][frame] = samples[sample_idx + ch];
                }
            }
        }

        // Prepare output buffers
        let mut output_buffers: Vec<Vec<f32>> = vec![vec![0.0; block_size]; output_channels];

        // Create buffer references
        let input_refs: Vec<&[f32]> = input_buffers
            .iter()
            .map(|buf| &buf[..frames_in_block])
            .collect();

        let mut output_refs: Vec<&mut [f32]> = output_buffers
            .iter_mut()
            .map(|buf| &mut buf[..frames_in_block])
            .collect();

        // Process through plugin
        let mut audio_buffer = AudioBuffer {
            inputs: &input_refs,
            outputs: &mut output_refs,
            frames: frames_in_block,
        };

        let event_buffer = EventBuffer::new();
        plugin.process(&mut audio_buffer, &event_buffer)?;

        // Interleave and write output samples
        #[allow(clippy::needless_range_loop)] // Index-based loop is clearer for interleaving
        for frame in 0..frames_in_block {
            for ch in 0..output_channels {
                let sample = output_buffers[ch][frame];
                let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                writer.write_sample(sample_i16)?;
            }
        }

        processed_frames += frames_in_block;

        // Progress indicator
        if block_idx % 100 == 0 {
            let progress = (processed_frames as f32 / num_frames as f32) * 100.0;
            print!("\r  Progress: {progress:.1}%");
            std::io::stdout().flush().unwrap();
        }
    }

    println!("\r  Progress: 100.0%");
    println!(
        "✓ Processed {} frames ({:.2}s)",
        processed_frames,
        processed_frames as f32 / sample_rate as f32
    );

    // Finalize
    plugin.deactivate();
    writer.finalize()?;

    println!("\n✓ Output written to: {output_path}");
    println!("\n✓ Processing complete!");

    Ok(())
}
