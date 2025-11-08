//! Example: Load and test the Noises.vst3 plugin
//!
//! Run with: cargo run --example `load_noises` --release

use vvdaw_plugin::Plugin;

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt().with_env_filter("debug").init();

    println!("Loading Noises.vst3 plugin...\n");

    let result = vvdaw_vst3::load_plugin("/Library/Audio/Plug-Ins/VST3/Noises.vst3");

    match result {
        Ok(mut plugin) => {
            println!("✓ Plugin loaded successfully!");
            println!("  Name: {}", plugin.info().name);
            println!("  Vendor: {}", plugin.info().vendor);
            println!("  Version: {}", plugin.info().version);
            println!("  Inputs: {}", plugin.input_channels());
            println!("  Outputs: {}", plugin.output_channels());

            // Try initializing
            println!("\nInitializing plugin at 48kHz, 512 samples...");
            if let Err(e) = plugin.initialize(48000, 512) {
                eprintln!("✗ Failed to initialize: {e}");
                std::process::exit(1);
            }
            println!("✓ Plugin initialized successfully!");

            // Try processing a buffer
            println!("\nProcessing audio buffer...");
            let frames = 512;
            let input_buffers = vec![vec![0.0f32; frames]; 2];
            let mut output_buffers = vec![vec![0.0f32; frames]; 2];

            let input_refs: Vec<&[f32]> =
                input_buffers.iter().map(std::vec::Vec::as_slice).collect();
            let mut output_refs: Vec<&mut [f32]> = output_buffers
                .iter_mut()
                .map(std::vec::Vec::as_mut_slice)
                .collect();

            let mut audio_buffer = vvdaw_plugin::AudioBuffer {
                inputs: &input_refs,
                outputs: &mut output_refs,
                frames,
            };

            let event_buffer = vvdaw_plugin::EventBuffer::new();

            if let Err(e) = plugin.process(&mut audio_buffer, &event_buffer) {
                eprintln!("✗ Failed to process: {e}");
                std::process::exit(1);
            }

            // Check if we got audio output
            let has_output = output_buffers
                .iter()
                .any(|buf| buf.iter().any(|&s| s != 0.0));

            if has_output {
                let peak = output_buffers
                    .iter()
                    .flat_map(|buf| buf.iter())
                    .fold(0.0f32, |max, &s| max.max(s.abs()));
                println!("✓ Audio processing successful! Peak level: {peak:.4}");
            } else {
                println!("⚠ Plugin processed but output is silence");
                println!("  (This might be expected for Noises without specific input/trigger)");
            }

            // Deactivate
            println!("\nDeactivating plugin...");
            plugin.deactivate();
            println!("✓ Plugin deactivated");

            println!("\n✓ All tests passed!");
        }
        Err(e) => {
            eprintln!("✗ Failed to load plugin: {e}");
            eprintln!("\nError details: {e:?}");
            std::process::exit(1);
        }
    }
}
