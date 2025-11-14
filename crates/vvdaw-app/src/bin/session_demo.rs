//! Session save/load demonstration
//!
//! Demonstrates saving and loading audio graph sessions with VST3 plugins.

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vvdaw_audio::graph::{AudioGraph, PluginSource};
use vvdaw_audio::session::{PluginSpec, Session};
use vvdaw_plugin::Plugin;
use vvdaw_vst3::MultiProcessPlugin;

/// Session save/load demo
#[derive(Parser, Debug)]
#[command(name = "session-demo")]
#[command(about = "Demonstrate session save/load with VST3 plugins", long_about = None)]
struct Args {
    /// VST3 plugin path (.vst3 bundle)
    #[arg(short, long)]
    plugin: PathBuf,

    /// Session file path (.ron)
    #[arg(short, long, default_value = "session.ron")]
    session: PathBuf,
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vvdaw=info,session_demo=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    println!("=== Session Save/Load Demo ===\n");

    // Step 1: Create an audio graph
    println!("Step 1: Creating audio graph...");
    let mut graph = AudioGraph::with_config(48000, 512);

    // Step 2: Load VST3 plugin
    println!(
        "Step 2: Loading VST3 plugin from {}...",
        args.plugin.display()
    );
    let plugin = MultiProcessPlugin::spawn(&args.plugin)
        .context("Failed to spawn VST3 plugin subprocess")?;

    let plugin_info = plugin.info().clone();
    println!("  Loaded: {} by {}", plugin_info.name, plugin_info.vendor);

    // Step 3: Add plugin to graph
    println!("Step 3: Adding plugin to graph...");
    let source = PluginSource::Vst3 {
        path: args.plugin.clone(),
    };
    let node_id = graph
        .add_node(Box::new(plugin), source)
        .context("Failed to add plugin to graph")?;
    println!("  Added node ID: {node_id}");

    // Step 4: Save session
    println!("\nStep 4: Saving session to {}...", args.session.display());
    let session = Session::from_graph(&graph, "Demo Session")
        .context("Failed to create session from graph")?;

    session
        .save(&args.session)
        .context("Failed to save session")?;
    println!("  ✓ Session saved successfully");

    // Display session contents
    println!("\n--- Session Contents ---");
    let session_text = std::fs::read_to_string(&args.session)?;
    println!("{session_text}");
    println!("--- End Session Contents ---\n");

    // Step 5: Load session
    println!("Step 5: Loading session from {}...", args.session.display());
    let loaded_session = Session::load(&args.session).context("Failed to load session")?;
    println!("  ✓ Session loaded successfully");
    println!("  Name: {}", loaded_session.name);
    println!("  Sample rate: {} Hz", loaded_session.sample_rate);
    println!("  Block size: {} frames", loaded_session.block_size);
    println!("  Nodes: {}", loaded_session.graph.nodes.len());
    println!("  Connections: {}", loaded_session.graph.connections.len());

    // Step 6: Reconstruct graph from session
    println!("\nStep 6: Reconstructing audio graph from session...");
    let reconstructed_graph = loaded_session
        .to_graph(|spec| match spec {
            PluginSpec::Vst3 { path, .. } => {
                println!("  Loading plugin from {}", path.display());
                let plugin = MultiProcessPlugin::spawn(path)
                    .map_err(|e| format!("Failed to spawn plugin: {e}"))?;
                Ok(Box::new(plugin) as Box<dyn vvdaw_plugin::Plugin>)
            }
        })
        .context("Failed to reconstruct graph from session")?;

    println!("  ✓ Graph reconstructed successfully");
    println!("  Nodes in graph: {}", reconstructed_graph.nodes().count());
    println!(
        "  Connections in graph: {}",
        reconstructed_graph.connections().count()
    );

    println!("\n=== Demo Complete ===");
    println!("✓ Successfully demonstrated session save/load cycle");
    println!("✓ Session file: {}", args.session.display());

    Ok(())
}
