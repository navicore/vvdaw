//! Audio processing graph.

use std::collections::{HashMap, HashSet};
use vvdaw_core::{Frames, Sample, SampleRate};
use vvdaw_plugin::{AudioBuffer, EventBuffer, Plugin, PluginError};

/// A node in the audio graph (typically wraps a plugin)
pub struct AudioNode {
    pub id: usize,
    pub plugin: Box<dyn Plugin>,
    /// Cached input/output channel counts
    pub inputs: usize,
    pub outputs: usize,
}

/// Connection between two nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Connection {
    pub from: usize,
    pub to: usize,
}

/// The audio processing graph
pub struct AudioGraph {
    nodes: HashMap<usize, AudioNode>,
    connections: HashSet<Connection>,
    next_id: usize,

    // Processing state
    sample_rate: SampleRate,
    block_size: Frames,

    // Audio buffers for inter-node routing
    // Map from node_id to its output buffer
    node_buffers: HashMap<usize, Vec<Vec<Sample>>>,
}

impl AudioGraph {
    /// Create a new empty audio graph
    pub fn new() -> Self {
        Self::with_config(48000, 512)
    }

    /// Create a new audio graph with specific sample rate and block size
    pub fn with_config(sample_rate: SampleRate, block_size: Frames) -> Self {
        Self {
            nodes: HashMap::new(),
            connections: HashSet::new(),
            next_id: 0,
            sample_rate,
            block_size,
            node_buffers: HashMap::new(),
        }
    }

    /// Initialize or update the graph configuration
    pub fn set_config(&mut self, sample_rate: SampleRate, block_size: Frames) {
        self.sample_rate = sample_rate;
        self.block_size = block_size;

        // Reinitialize all plugins with new config
        for node in self.nodes.values_mut() {
            if let Err(e) = node.plugin.initialize(sample_rate, block_size) {
                tracing::error!("Failed to reinitialize plugin {}: {}", node.id, e);
            }
        }

        // Reallocate buffers
        self.allocate_buffers();
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, mut plugin: Box<dyn Plugin>) -> Result<usize, PluginError> {
        let id = self.next_id;
        self.next_id += 1;

        // Initialize the plugin
        plugin.initialize(self.sample_rate, self.block_size)?;

        let inputs = plugin.input_channels();
        let outputs = plugin.output_channels();

        self.nodes.insert(
            id,
            AudioNode {
                id,
                plugin,
                inputs,
                outputs,
            },
        );

        // Allocate buffer for this node's output
        self.allocate_node_buffer(id, outputs);

        tracing::debug!("Added node {} ({} inputs, {} outputs)", id, inputs, outputs);
        Ok(id)
    }

    /// Remove a node from the graph
    pub fn remove_node(&mut self, id: usize) -> Option<AudioNode> {
        // Remove all connections involving this node
        self.connections
            .retain(|conn| conn.from != id && conn.to != id);

        // Remove the node
        let node = self.nodes.remove(&id)?;

        // Remove its buffer
        self.node_buffers.remove(&id);

        tracing::debug!("Removed node {}", id);
        Some(node)
    }

    /// Connect two nodes
    pub fn connect(&mut self, from: usize, to: usize) -> Result<(), String> {
        if !self.nodes.contains_key(&from) {
            return Err(format!("Source node {from} not found"));
        }
        if !self.nodes.contains_key(&to) {
            return Err(format!("Destination node {to} not found"));
        }

        let conn = Connection { from, to };
        if self.connections.insert(conn) {
            tracing::debug!("Connected {} -> {}", from, to);
        }

        Ok(())
    }

    /// Disconnect two nodes
    pub fn disconnect(&mut self, from: usize, to: usize) {
        let conn = Connection { from, to };
        if self.connections.remove(&conn) {
            tracing::debug!("Disconnected {} -> {}", from, to);
        }
    }

    /// Allocate buffer for a node's output
    fn allocate_node_buffer(&mut self, node_id: usize, channel_count: usize) {
        let buffer = vec![vec![0.0; self.block_size]; channel_count];
        self.node_buffers.insert(node_id, buffer);
    }

    /// Allocate all buffers based on current nodes
    fn allocate_buffers(&mut self) {
        self.node_buffers.clear();
        for (&id, node) in &self.nodes {
            let buffer = vec![vec![0.0; self.block_size]; node.outputs];
            self.node_buffers.insert(id, buffer);
        }
    }

    /// Process all nodes in the graph
    pub fn process(&mut self, system_input: &[&[Sample]], system_output: &mut [&mut [Sample]]) {
        if self.nodes.is_empty() {
            // No nodes - pass through silence
            for channel in system_output.iter_mut() {
                channel.fill(0.0);
            }
            return;
        }

        // For now, simple linear processing (no topological sort)
        // TODO: Implement proper topological sort for complex graphs
        let event_buffer = EventBuffer::new();

        for (node_id, node) in &mut self.nodes {
            // Prepare input buffer (for now, use system input)
            // TODO: Mix inputs from connected nodes
            let node_buffer = self.node_buffers.get_mut(node_id).unwrap();

            // Create mutable references for AudioBuffer
            let mut output_refs: Vec<&mut [Sample]> =
                node_buffer.iter_mut().map(Vec::as_mut_slice).collect();

            let mut audio_buffer = AudioBuffer {
                inputs: system_input,
                outputs: &mut output_refs,
                frames: self.block_size,
            };

            // Process the node
            if let Err(e) = node.plugin.process(&mut audio_buffer, &event_buffer) {
                tracing::error!("Plugin {} processing error: {}", node_id, e);
                // Fill output with silence on error
                for channel in node_buffer.iter_mut() {
                    channel.fill(0.0);
                }
            }
        }

        // Copy last node output to system output
        // TODO: Implement proper output routing
        if let Some((_, node_buffer)) = self.node_buffers.iter().next() {
            for (out_ch, node_ch) in system_output.iter_mut().zip(node_buffer.iter()) {
                let len = out_ch.len().min(node_ch.len());
                out_ch[..len].copy_from_slice(&node_ch[..len]);
            }
        }
    }
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}
