//! Generate a test WAV file with a sine wave tone
//!
//! Useful for testing the vvdaw-process tool.

use anyhow::{Context, Result};
use clap::Parser;
use hound::{WavSpec, WavWriter};
use std::f32::consts::PI;
use std::path::PathBuf;

/// Generate a test WAV file with a sine wave
#[derive(Parser, Debug)]
#[command(name = "generate-test-wav")]
#[command(about = "Generate test WAV files for audio processing", long_about = None)]
struct Args {
    /// Output WAV file
    #[arg(short, long)]
    output: PathBuf,

    /// Sample rate (Hz)
    #[arg(short, long, default_value_t = 48000)]
    sample_rate: u32,

    /// Number of channels
    #[arg(short, long, default_value_t = 2)]
    channels: u16,

    /// Duration (seconds)
    #[arg(short, long, default_value_t = 5.0)]
    duration: f32,

    /// Frequency (Hz)
    #[arg(short, long, default_value_t = 440.0)]
    frequency: f32,

    /// Amplitude (0.0-1.0)
    #[arg(short, long, default_value_t = 0.5)]
    amplitude: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Generating test WAV:");
    println!("  Output: {}", args.output.display());
    println!("  Sample rate: {} Hz", args.sample_rate);
    println!("  Channels: {}", args.channels);
    println!("  Duration: {:.1} seconds", args.duration);
    println!("  Frequency: {:.1} Hz", args.frequency);
    println!("  Amplitude: {:.2}", args.amplitude);

    let spec = WavSpec {
        channels: args.channels,
        sample_rate: args.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(&args.output, spec)
        .with_context(|| format!("Failed to create output file: {}", args.output.display()))?;

    let num_samples = (args.sample_rate as f32 * args.duration) as usize;
    let num_frames = num_samples;

    for frame in 0..num_frames {
        let t = frame as f32 / args.sample_rate as f32;
        let sample = args.amplitude * (2.0 * PI * args.frequency * t).sin();

        // Write same sample to all channels
        for _ in 0..args.channels {
            writer
                .write_sample(sample)
                .context("Failed to write sample")?;
        }
    }

    writer.finalize().context("Failed to finalize WAV file")?;

    println!("Successfully wrote {num_frames} frames");
    Ok(())
}
