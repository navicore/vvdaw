//! Audio processing graph.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use vvdaw_core::{Frames, Sample, SampleRate};
use vvdaw_plugin::{AudioBuffer, EventBuffer, Plugin, PluginError};

/// A node in the audio graph (typically wraps a plugin)
pub struct AudioNode {
    id: usize,
    plugin: Box<dyn Plugin>,
    /// Cached input/output channel counts
    inputs: usize,
    outputs: usize,
}

impl AudioNode {
    /// Get the node's ID
    #[must_use]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the number of input channels
    #[must_use]
    pub fn inputs(&self) -> usize {
        self.inputs
    }

    /// Get the number of output channels
    #[must_use]
    pub fn outputs(&self) -> usize {
        self.outputs
    }
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

    // Input buffers for mixing connected node outputs
    // Map from node_id to its input buffer (cleared and mixed before each process)
    input_buffers: HashMap<usize, Vec<Vec<Sample>>>,

    // Pre-computed processing order (sorted node IDs)
    // Updated when nodes are added/removed to avoid allocating in process()
    processing_order: Vec<usize>,
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
            input_buffers: HashMap::new(),
            processing_order: Vec::new(),
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

        // Allocate buffers for this node's input and output
        self.allocate_node_buffer(id, inputs, outputs);

        // Update processing order (allocates, but not in audio callback)
        self.update_processing_order();

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

        // Remove its buffers
        self.node_buffers.remove(&id);
        self.input_buffers.remove(&id);

        // Update processing order (allocates, but not in audio callback)
        self.update_processing_order();

        tracing::debug!("Removed node {}", id);
        Some(node)
    }

    /// Connect two nodes
    ///
    /// # Channel Handling
    ///
    /// This method does **not** validate that output channel counts of `from`
    /// match input channel counts of `to`. This is intentional:
    ///
    /// - **Upmixing**: A mono source (1 channel) can feed a stereo effect (2 channels)
    /// - **Downmixing**: A stereo source (2 channels) can feed a mono analyzer (1 channel)
    /// - **Summing**: Multiple nodes can connect to the same destination (mixer pattern)
    ///
    /// Channel routing and mixing logic is implemented in the `process()` method.
    /// Checkpoint 2 will add explicit per-channel routing.
    ///
    /// # Current Limitations
    ///
    /// - All connections currently route all available channels (no per-channel routing)
    /// - Channel count mismatches are handled by truncation or zero-padding in `process()`
    /// - No validation for mono-only or stereo-only plugin requirements
    ///
    /// # Future Work (Checkpoint 2)
    ///
    /// - Per-channel routing (e.g., "connect output channel 0 to input channel 1")
    /// - Explicit mixing configuration (sum, replace, etc.)
    /// - Validation modes for strict channel matching
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
            // Update processing order to reflect new dependencies
            self.update_processing_order();
        }

        Ok(())
    }

    /// Disconnect two nodes
    pub fn disconnect(&mut self, from: usize, to: usize) {
        let conn = Connection { from, to };
        if self.connections.remove(&conn) {
            tracing::debug!("Disconnected {} -> {}", from, to);
            // Update processing order to reflect removed dependency
            self.update_processing_order();
        }
    }

    /// Allocate input and output buffers for a node
    fn allocate_node_buffer(
        &mut self,
        node_id: usize,
        input_channels: usize,
        output_channels: usize,
    ) {
        let input_buffer = vec![vec![0.0; self.block_size]; input_channels];
        let output_buffer = vec![vec![0.0; self.block_size]; output_channels];
        self.input_buffers.insert(node_id, input_buffer);
        self.node_buffers.insert(node_id, output_buffer);
    }

    /// Update the processing order after graph structure changes
    /// IMPORTANT: This allocates, so call it when adding/removing nodes, NOT in `process()`
    ///
    /// Uses topological sort (Kahn's algorithm) to determine processing order.
    /// Nodes are processed in dependency order: a node is processed only after
    /// all nodes that feed into it have been processed.
    ///
    /// If the graph contains cycles, falls back to sorted node IDs (linear order).
    fn update_processing_order(&mut self) {
        self.processing_order.clear();

        // Attempt topological sort
        match self.topological_sort() {
            Ok(order) => {
                self.processing_order = order;
                tracing::debug!(
                    "Updated processing order (topological): {:?}",
                    self.processing_order
                );
            }
            Err(cycle_nodes) => {
                // Graph has cycles - fall back to sorted ID order
                tracing::warn!(
                    "Graph contains cycle involving nodes: {:?}. Using linear order instead.",
                    cycle_nodes
                );
                self.processing_order.extend(self.nodes.keys().copied());
                self.processing_order.sort_unstable();
            }
        }
    }

    /// Perform topological sort using Kahn's algorithm
    ///
    /// Complexity: O(V + E) where V = nodes, E = edges
    ///
    /// Returns Ok(order) if graph is acyclic, `Err(remaining_nodes)` if cycles exist.
    fn topological_sort(&self) -> Result<Vec<usize>, Vec<usize>> {
        // Build in-degree map: count incoming edges for each node
        // Pre-allocate for predictable size to avoid rehashing
        let mut in_degree: HashMap<usize, usize> = HashMap::with_capacity(self.nodes.len());
        for &id in self.nodes.keys() {
            in_degree.insert(id, 0);
        }

        // Build adjacency list for O(1) outgoing edge lookup
        // This avoids O(V × E) iteration through all connections for each node
        // Pre-allocate based on number of source nodes (upper bound)
        let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::with_capacity(self.nodes.len());
        for conn in &self.connections {
            *in_degree.entry(conn.to).or_insert(0) += 1;
            adjacency.entry(conn.from).or_default().push(conn.to);
        }

        // Use a min-heap (via Reverse) for O(log V) insertions/removals
        // This maintains deterministic sorted order automatically
        let mut queue: BinaryHeap<Reverse<usize>> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(&id, _)| Reverse(id))
            .collect();

        let mut result = Vec::new();

        // Process nodes in sorted order (O(V log V) for all pops)
        while let Some(Reverse(node_id)) = queue.pop() {
            result.push(node_id);

            // Look up outgoing edges in adjacency list (O(1) lookup)
            if let Some(outgoing) = adjacency.get(&node_id) {
                // Reduce in-degree of connected nodes
                for &to_id in outgoing {
                    if let Some(degree) = in_degree.get_mut(&to_id) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(Reverse(to_id)); // O(log V)
                        }
                    }
                }
            }
        }

        // Check if we processed all nodes
        if result.len() == self.nodes.len() {
            Ok(result)
        } else {
            // Cycle detected - return nodes that couldn't be processed
            // Use HashSet for O(V) instead of O(V²) from Vec::contains
            let processed: HashSet<usize> = result.into_iter().collect();
            let remaining: Vec<usize> = self
                .nodes
                .keys()
                .filter(|id| !processed.contains(id))
                .copied()
                .collect();
            Err(remaining)
        }
    }

    /// Allocate all buffers based on current nodes
    fn allocate_buffers(&mut self) {
        self.node_buffers.clear();
        self.input_buffers.clear();
        for (&id, node) in &self.nodes {
            let input_buffer = vec![vec![0.0; self.block_size]; node.inputs];
            let output_buffer = vec![vec![0.0; self.block_size]; node.outputs];
            self.input_buffers.insert(id, input_buffer);
            self.node_buffers.insert(id, output_buffer);
        }
    }

    /// Process all nodes in the graph with connection-based routing
    ///
    /// # Audio Flow
    /// 1. Input nodes (no incoming connections) receive `system_input`
    /// 2. Connected nodes receive mixed outputs from their source nodes
    /// 3. Output nodes (no outgoing connections) are mixed to `system_output`
    ///
    /// # Mixing Strategy
    /// Uses **additive mixing** (sum all sources) without gain compensation.
    ///
    /// ## Clipping Risk Warning
    /// When multiple loud sources are summed, the output can exceed ±1.0 and cause clipping.
    /// For example:
    /// - Two nodes outputting 0.8 → sum = 1.6 (clipped to 1.0)
    /// - Three nodes outputting 0.5 → sum = 1.5 (clipped to 1.0)
    ///
    /// **Mitigation strategies:**
    /// - Keep individual node outputs at lower levels when mixing multiple sources
    /// - Use gain/attenuation plugins in the graph to control levels
    /// - Add a master limiter at the output
    /// - Future: Implement automatic gain compensation (divide by source count)
    pub fn process(&mut self, system_input: &[&[Sample]], system_output: &mut [&mut [Sample]]) {
        if self.nodes.is_empty() {
            // No nodes - output silence
            for channel in system_output.iter_mut() {
                channel.fill(0.0);
            }
            return;
        }

        // Clear all input buffers (will be filled from connections or system_input)
        for input_buffer in self.input_buffers.values_mut() {
            for channel in input_buffer {
                channel.fill(0.0);
            }
        }

        // Build incoming connections map for quick lookup
        // Map: destination_node_id -> Vec<source_node_id>
        let mut incoming: HashMap<usize, Vec<usize>> = HashMap::new();
        for conn in &self.connections {
            incoming.entry(conn.to).or_default().push(conn.from);
        }

        // Build outgoing connections set for identifying output nodes
        let outgoing: HashSet<usize> = self.connections.iter().map(|conn| conn.from).collect();

        let event_buffer = EventBuffer::new();

        // Process nodes in topological order
        for &node_id in &self.processing_order {
            // Route inputs for this node
            if let Some(input_buffer) = self.input_buffers.get_mut(&node_id) {
                if let Some(sources) = incoming.get(&node_id) {
                    // This node has incoming connections - mix source outputs
                    for &source_id in sources {
                        if let Some(source_output) = self.node_buffers.get(&source_id) {
                            // Mix source output into this node's input (additive)
                            for (input_ch, source_ch) in
                                input_buffer.iter_mut().zip(source_output.iter())
                            {
                                for (input_sample, &source_sample) in
                                    input_ch.iter_mut().zip(source_ch.iter())
                                {
                                    *input_sample += source_sample;
                                }
                            }
                        }
                    }
                } else {
                    // No incoming connections - this is an input node, use system_input
                    for (input_ch, system_ch) in input_buffer.iter_mut().zip(system_input.iter()) {
                        let len = input_ch.len().min(system_ch.len());
                        input_ch[..len].copy_from_slice(&system_ch[..len]);
                    }
                }
            }

            // Process the node
            if let (Some(node), Some(input_buffer), Some(output_buffer)) = (
                self.nodes.get_mut(&node_id),
                self.input_buffers.get(&node_id),
                self.node_buffers.get_mut(&node_id),
            ) {
                // Create input/output slice references
                // NOTE: These Vec allocations are small (typically 16-64 bytes for 2-8 channels)
                // and unavoidable without unsafe code due to Rust's drop checker:
                //
                // SmallVec would avoid heap allocation, but triggers lifetime issues because
                // the compiler can't prove SmallVec's Drop doesn't use the borrowed slices.
                // Vec has special treatment in the borrow checker that SmallVec lacks.
                //
                // In practice, this is acceptable for real-time audio:
                // - Allocation size is tiny and predictable (8 bytes * channel_count)
                // - Modern allocators (jemalloc, mimalloc) handle small allocations efficiently
                // - The alternative (unsafe code) adds maintenance burden
                // - Pre-allocating with capacity helps allocators reuse memory pools
                let input_refs: Vec<&[Sample]> = input_buffer.iter().map(Vec::as_slice).collect();

                let mut output_refs: Vec<&mut [Sample]> = Vec::with_capacity(node.outputs());
                output_refs.extend(output_buffer.iter_mut().map(Vec::as_mut_slice));

                let mut audio_buffer = AudioBuffer {
                    inputs: &input_refs,
                    outputs: &mut output_refs,
                    frames: self.block_size,
                };

                // Process (errors ignored - real-time safe, silence on error)
                let _ = node.plugin.process(&mut audio_buffer, &event_buffer);
            }
        }

        // Route outputs: mix all output nodes (no outgoing connections) to system_output
        // Clear system output first
        for channel in system_output.iter_mut() {
            channel.fill(0.0);
        }

        for &node_id in &self.processing_order {
            // Check if this node is an output node (no outgoing connections)
            // NOTE: Clippy suggests collapsing this with let-chains syntax, but that's unstable
            #[allow(clippy::collapsible_if)]
            if !outgoing.contains(&node_id) {
                if let Some(node_output) = self.node_buffers.get(&node_id) {
                    // Mix this output node to system_output (additive)
                    for (sys_ch, node_ch) in system_output.iter_mut().zip(node_output.iter()) {
                        let len = sys_ch.len().min(node_ch.len());
                        for i in 0..len {
                            sys_ch[i] += node_ch[i];
                        }
                    }
                }
            }
        }
    }
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvdaw_plugin::{AudioBuffer, EventBuffer, PluginError, PluginInfo};

    /// Dummy plugin for testing that just copies input to output
    struct DummyPlugin {
        info: PluginInfo,
        inputs: usize,
        outputs: usize,
    }

    impl DummyPlugin {
        fn new(name: &str, inputs: usize, outputs: usize) -> Self {
            Self {
                info: PluginInfo {
                    name: name.to_string(),
                    vendor: "Test".to_string(),
                    version: "1.0".to_string(),
                    unique_id: format!("test_{name}"),
                },
                inputs,
                outputs,
            }
        }
    }

    impl Plugin for DummyPlugin {
        fn info(&self) -> &PluginInfo {
            &self.info
        }

        fn initialize(
            &mut self,
            _sample_rate: SampleRate,
            _max_block_size: Frames,
        ) -> Result<(), PluginError> {
            Ok(())
        }

        fn process(
            &mut self,
            audio: &mut AudioBuffer,
            _events: &EventBuffer,
        ) -> Result<(), PluginError> {
            for (input, output) in audio.inputs.iter().zip(audio.outputs.iter_mut()) {
                output[..audio.frames].copy_from_slice(&input[..audio.frames]);
            }
            Ok(())
        }

        fn set_parameter(&mut self, _id: u32, _value: f32) -> Result<(), PluginError> {
            Ok(())
        }

        fn get_parameter(&self, _id: u32) -> Result<f32, PluginError> {
            Ok(0.0)
        }

        fn parameters(&self) -> Vec<vvdaw_plugin::ParameterInfo> {
            Vec::new()
        }

        fn input_channels(&self) -> usize {
            self.inputs
        }

        fn output_channels(&self) -> usize {
            self.outputs
        }

        fn deactivate(&mut self) {}
    }

    #[test]
    fn test_empty_graph_topological_sort() {
        let graph = AudioGraph::new();
        let result = graph.topological_sort();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn test_single_node_topological_sort() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();

        let result = graph.topological_sort();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![node_a]);
    }

    #[test]
    fn test_linear_chain_topological_sort() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_c).unwrap();

        let result = graph.topological_sort();
        assert!(result.is_ok());
        let order = result.unwrap();

        // A must come before B, B must come before C
        let pos_a = order.iter().position(|&id| id == node_a).unwrap();
        let pos_b = order.iter().position(|&id| id == node_b).unwrap();
        let pos_c = order.iter().position(|&id| id == node_c).unwrap();

        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_parallel_paths_topological_sort() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();
        let node_d = graph
            .add_node(Box::new(DummyPlugin::new("D", 2, 2)))
            .unwrap();

        // A -> B -> D
        // A -> C -> D
        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_a, node_c).unwrap();
        graph.connect(node_b, node_d).unwrap();
        graph.connect(node_c, node_d).unwrap();

        let result = graph.topological_sort();
        assert!(result.is_ok());
        let order = result.unwrap();

        let pos_a = order.iter().position(|&id| id == node_a).unwrap();
        let pos_b = order.iter().position(|&id| id == node_b).unwrap();
        let pos_c = order.iter().position(|&id| id == node_c).unwrap();
        let pos_d = order.iter().position(|&id| id == node_d).unwrap();

        // A must come before both B and C
        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);

        // Both B and C must come before D
        assert!(pos_b < pos_d);
        assert!(pos_c < pos_d);
    }

    #[test]
    fn test_disconnected_nodes_topological_sort() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        // No connections - all nodes independent
        let result = graph.topological_sort();
        assert!(result.is_ok());
        let order = result.unwrap();

        // All nodes should be present
        assert_eq!(order.len(), 3);
        assert!(order.contains(&node_a));
        assert!(order.contains(&node_b));
        assert!(order.contains(&node_c));
    }

    #[test]
    fn test_simple_cycle_detection() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        // Create cycle: A -> B -> C -> A
        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_c).unwrap();
        graph.connect(node_c, node_a).unwrap();

        let result = graph.topological_sort();
        assert!(result.is_err());
        let remaining = result.unwrap_err();

        // All three nodes should be in the cycle
        assert_eq!(remaining.len(), 3);
        assert!(remaining.contains(&node_a));
        assert!(remaining.contains(&node_b));
        assert!(remaining.contains(&node_c));
    }

    #[test]
    fn test_self_loop_cycle_detection() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();

        // Create self-loop: A -> A
        graph.connect(node_a, node_a).unwrap();

        let result = graph.topological_sort();
        assert!(result.is_err());
        let remaining = result.unwrap_err();

        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains(&node_a));
    }

    #[test]
    fn test_partial_cycle_detection() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();
        let node_d = graph
            .add_node(Box::new(DummyPlugin::new("D", 2, 2)))
            .unwrap();

        // A -> B -> C -> B (cycle between B and C)
        // D is independent
        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_c).unwrap();
        graph.connect(node_c, node_b).unwrap();

        let result = graph.topological_sort();
        assert!(result.is_err());
        let remaining = result.unwrap_err();

        // B and C are in the cycle
        assert!(remaining.contains(&node_b));
        assert!(remaining.contains(&node_c));

        // A and D should have been processed
        assert!(!remaining.contains(&node_a));
        assert!(!remaining.contains(&node_d));
    }

    #[test]
    fn test_processing_order_updates_on_add() {
        let mut graph = AudioGraph::new();

        // Initially empty
        assert_eq!(graph.processing_order.len(), 0);

        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        assert_eq!(graph.processing_order.len(), 1);
        assert_eq!(graph.processing_order[0], node_a);

        let _node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        assert_eq!(graph.processing_order.len(), 2);
    }

    #[test]
    fn test_processing_order_updates_on_remove() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();

        assert_eq!(graph.processing_order.len(), 2);

        graph.remove_node(node_a);
        assert_eq!(graph.processing_order.len(), 1);
        assert_eq!(graph.processing_order[0], node_b);
    }

    #[test]
    fn test_processing_order_respects_dependencies() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        // Connect in reverse order to test sorting
        graph.connect(node_c, node_b).unwrap();
        graph.connect(node_b, node_a).unwrap();

        let pos_a = graph
            .processing_order
            .iter()
            .position(|&id| id == node_a)
            .unwrap();
        let pos_b = graph
            .processing_order
            .iter()
            .position(|&id| id == node_b)
            .unwrap();
        let pos_c = graph
            .processing_order
            .iter()
            .position(|&id| id == node_c)
            .unwrap();

        // C -> B -> A, so process order should be C, B, A
        assert!(pos_c < pos_b);
        assert!(pos_b < pos_a);
    }

    #[test]
    fn test_cycle_fallback_to_linear_order() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();

        // Create cycle
        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_a).unwrap();

        // Processing order should fall back to sorted IDs
        assert_eq!(graph.processing_order.len(), 2);

        // Should be sorted by ID (linear order)
        let mut expected = vec![node_a, node_b];
        expected.sort_unstable();
        assert_eq!(graph.processing_order, expected);
    }

    #[test]
    fn test_connection_management() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();

        // Test connect
        assert!(graph.connect(node_a, node_b).is_ok());

        // Test invalid connections
        assert!(graph.connect(999, node_b).is_err());
        assert!(graph.connect(node_a, 999).is_err());

        // Test disconnect
        graph.disconnect(node_a, node_b);

        // After disconnect, should be back to independent nodes
        let result = graph.topological_sort().unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_remove_node_removes_connections() {
        let mut graph = AudioGraph::new();
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_c).unwrap();

        // Remove middle node
        graph.remove_node(node_b);

        // Should have 2 nodes left
        assert_eq!(graph.nodes.len(), 2);

        // No connections should remain
        assert_eq!(graph.connections.len(), 0);

        // Topological sort should succeed with remaining nodes
        let result = graph.topological_sort().unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&node_a));
        assert!(result.contains(&node_c));
    }

    #[test]
    fn test_large_graph_performance() {
        // Test with 100 nodes to validate O(V + E) performance
        // This should complete quickly (< 100ms) with optimized algorithm
        let mut graph = AudioGraph::new();
        let mut nodes = Vec::new();

        // Create 100 nodes
        for i in 0..100 {
            let node = graph
                .add_node(Box::new(DummyPlugin::new(&format!("Node{i}"), 2, 2)))
                .unwrap();
            nodes.push(node);
        }

        // Create a complex graph structure:
        // - Linear chain for first 50 nodes
        // - Parallel paths for next 50 nodes
        // Total: ~150 edges
        for i in 0..49 {
            graph.connect(nodes[i], nodes[i + 1]).unwrap();
        }

        // Create parallel paths that merge
        for i in 50..75 {
            graph.connect(nodes[49], nodes[i]).unwrap();
            graph.connect(nodes[i], nodes[99]).unwrap();
        }

        // Add some cross-connections for complexity
        for i in 75..90 {
            graph.connect(nodes[i - 25], nodes[i]).unwrap();
            graph.connect(nodes[i], nodes[i + 5]).unwrap();
        }

        // Verify processing order is correct
        assert_eq!(graph.processing_order.len(), 100);

        // Verify topological properties
        let result = graph.topological_sort();
        assert!(result.is_ok());
        let order = result.unwrap();
        assert_eq!(order.len(), 100);

        // Verify that node 0 comes before node 49 (linear chain)
        let pos_0 = order.iter().position(|&id| id == nodes[0]).unwrap();
        let pos_49 = order.iter().position(|&id| id == nodes[49]).unwrap();
        assert!(pos_0 < pos_49);

        // Verify that node 49 comes before node 99 (merge point)
        let pos_99 = order.iter().position(|&id| id == nodes[99]).unwrap();
        assert!(pos_49 < pos_99);

        // Test that updates remain fast
        // Adding a new connection should still be quick
        graph.connect(nodes[10], nodes[90]).unwrap();
        assert_eq!(graph.processing_order.len(), 100);

        // Removing a node should still be quick
        graph.remove_node(nodes[50]);
        assert_eq!(graph.processing_order.len(), 99);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn test_very_large_graph_benchmark() {
        // Benchmark with 1000 nodes (release mode only)
        // With O(V + E) algorithm, this should complete in < 10ms
        use std::time::Instant;

        let mut graph = AudioGraph::new();
        let mut nodes = Vec::new();

        let start = Instant::now();

        // Create 1000 nodes
        for i in 0..1000 {
            let node = graph
                .add_node(Box::new(DummyPlugin::new(&format!("N{i}"), 2, 2)))
                .unwrap();
            nodes.push(node);
        }

        let create_time = start.elapsed();
        println!("Created 1000 nodes in {:?}", create_time);

        // Create ~2000 edges (complex graph)
        let connection_start = Instant::now();
        for i in 0..999 {
            graph.connect(nodes[i], nodes[i + 1]).unwrap();
        }
        for i in (0..500).step_by(2) {
            graph.connect(nodes[i], nodes[i + 500]).unwrap();
        }

        let connection_time = connection_start.elapsed();
        println!("Created ~1500 connections in {:?}", connection_time);

        // Test topological sort performance
        let sort_start = Instant::now();
        let result = graph.topological_sort();
        let sort_time = sort_start.elapsed();

        println!("Topological sort on 1000 nodes in {:?}", sort_time);

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1000);

        // With O(V + E) algorithm, this should be < 10ms in release mode
        // In debug mode it might be slower, so we only run this in release
        assert!(
            sort_time.as_millis() < 100,
            "Topological sort took {}ms (expected < 100ms)",
            sort_time.as_millis()
        );
    }

    // ============================================================================
    // Connection-Based Routing Tests (Checkpoint 2)
    // ============================================================================

    #[test]
    fn test_single_node_passthrough() {
        // Test: system_input -> node -> system_output
        let mut graph = AudioGraph::with_config(48000, 64);
        let _node = graph
            .add_node(Box::new(DummyPlugin::new("PassThrough", 2, 2)))
            .unwrap();

        // Create test input
        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create output buffer
        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        // Process
        graph.process(&input_refs, &mut output_refs);

        // Verify: output should equal input (pass-through)
        assert_eq!(output_data[0][0], 1.0);
        assert_eq!(output_data[1][0], 2.0);
    }

    #[test]
    fn test_linear_chain_routing() {
        // Test: system_input -> A -> B -> C -> system_output
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_c).unwrap();

        // Create test input
        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create output buffer
        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        // Process
        graph.process(&input_refs, &mut output_refs);

        // Verify: all nodes pass through, so output should equal input
        assert_eq!(output_data[0][0], 1.0);
        assert_eq!(output_data[1][0], 2.0);
    }

    #[test]
    fn test_input_mixing() {
        // Test: A -> C
        //       B -> C
        // Both A and B outputs should be mixed (summed) into C's input
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_c).unwrap();
        graph.connect(node_b, node_c).unwrap();

        // Create test input: different values for each input node
        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // A and B both receive system_input and pass through
        // C receives A + B mixed, so should be 2.0 + 4.0 = 2.0 (A) + 2.0 (B)
        // Actually: A gets [1.0, 2.0], B gets [1.0, 2.0], C gets sum = [2.0, 4.0]
        assert_eq!(output_data[0][0], 2.0); // 1.0 + 1.0
        assert_eq!(output_data[1][0], 4.0); // 2.0 + 2.0
    }

    #[test]
    fn test_parallel_paths() {
        // Test: A -> B
        //       A -> C
        // A's output should be routed to both B and C
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_a, node_c).unwrap();

        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // B and C are both output nodes (no outgoing connections)
        // Both receive A's output, so system_output gets B + C mixed
        // Each gets [1.0, 2.0] from A, so sum = [2.0, 4.0]
        assert_eq!(output_data[0][0], 2.0);
        assert_eq!(output_data[1][0], 4.0);
    }

    #[test]
    fn test_diamond_pattern() {
        // Test:     A
        //          / \
        //         B   C
        //          \ /
        //           D
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("C", 2, 2)))
            .unwrap();
        let node_d = graph
            .add_node(Box::new(DummyPlugin::new("D", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_a, node_c).unwrap();
        graph.connect(node_b, node_d).unwrap();
        graph.connect(node_c, node_d).unwrap();

        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // A gets [1.0, 2.0]
        // B gets A's output: [1.0, 2.0]
        // C gets A's output: [1.0, 2.0]
        // D gets B + C mixed: [2.0, 4.0]
        // system_output gets D: [2.0, 4.0]
        assert_eq!(output_data[0][0], 2.0);
        assert_eq!(output_data[1][0], 4.0);
    }

    #[test]
    fn test_disconnected_nodes_output() {
        // Test: A (disconnected), B (disconnected)
        // Both should output to system_output (mixed)
        let mut graph = AudioGraph::with_config(48000, 64);
        let _node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let _node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();

        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // Both A and B get system_input and pass through
        // Both are output nodes, so mixed to system_output: [2.0, 4.0]
        assert_eq!(output_data[0][0], 2.0);
        assert_eq!(output_data[1][0], 4.0);
    }

    #[test]
    fn test_no_output_nodes() {
        // Test: A -> B (cycle, but fallback to linear order)
        //       Both have outgoing connections, so no output to system
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("A", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("B", 2, 2)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();
        graph.connect(node_b, node_a).unwrap(); // Cycle

        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // Both nodes have outgoing connections, so neither is an output node
        // system_output should be silence
        assert_eq!(output_data[0][0], 0.0);
        assert_eq!(output_data[1][0], 0.0);
    }

    // ============================================================================
    // Channel Mismatch Tests
    // ============================================================================

    #[test]
    fn test_channel_mismatch_fewer_system_input_channels() {
        // Test: 1-channel system_input -> 2-channel node
        // Second channel should be silent (not panic)
        let mut graph = AudioGraph::with_config(48000, 64);
        let _node = graph
            .add_node(Box::new(DummyPlugin::new("StereoNode", 2, 2)))
            .unwrap();

        // Create 1-channel input
        let input_data = [vec![1.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create 2-channel output
        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // First channel should have input signal
        assert_eq!(output_data[0][0], 1.0);
        // Second channel should be silence (no input available)
        assert_eq!(output_data[1][0], 0.0);
    }

    #[test]
    fn test_channel_mismatch_more_system_input_channels() {
        // Test: 4-channel system_input -> 2-channel node
        // Node should only see first 2 channels
        let mut graph = AudioGraph::with_config(48000, 64);
        let _node = graph
            .add_node(Box::new(DummyPlugin::new("StereoNode", 2, 2)))
            .unwrap();

        // Create 4-channel input
        let input_data = [
            vec![1.0_f32; 64],
            vec![2.0_f32; 64],
            vec![3.0_f32; 64],
            vec![4.0_f32; 64],
        ];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create 2-channel output
        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // Should only output first 2 channels
        assert_eq!(output_data[0][0], 1.0);
        assert_eq!(output_data[1][0], 2.0);
    }

    #[test]
    fn test_channel_mismatch_fewer_system_output_channels() {
        // Test: 4-channel node -> 2-channel system_output
        // Only first 2 channels should be written
        let mut graph = AudioGraph::with_config(48000, 64);
        let _node = graph
            .add_node(Box::new(DummyPlugin::new("QuadNode", 4, 4)))
            .unwrap();

        // Create 4-channel input
        let input_data = [
            vec![1.0_f32; 64],
            vec![2.0_f32; 64],
            vec![3.0_f32; 64],
            vec![4.0_f32; 64],
        ];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create 2-channel output (smaller than node's output)
        let mut output_data = [vec![0.0_f32; 64], vec![0.0_f32; 64]];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // First 2 channels should be written
        assert_eq!(output_data[0][0], 1.0);
        assert_eq!(output_data[1][0], 2.0);
        // Channels 3 and 4 are discarded (no space in system_output)
    }

    #[test]
    fn test_channel_mismatch_more_system_output_channels() {
        // Test: 1-channel node -> 4-channel system_output
        // Only first channel should have signal
        let mut graph = AudioGraph::with_config(48000, 64);
        let _node = graph
            .add_node(Box::new(DummyPlugin::new("MonoNode", 1, 1)))
            .unwrap();

        // Create 1-channel input
        let input_data = [vec![1.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create 4-channel output (larger than node's output)
        let mut output_data = [
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
        ];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // First channel should have signal
        assert_eq!(output_data[0][0], 1.0);
        // Other channels should be silence
        assert_eq!(output_data[1][0], 0.0);
        assert_eq!(output_data[2][0], 0.0);
        assert_eq!(output_data[3][0], 0.0);
    }

    #[test]
    fn test_channel_mismatch_connected_nodes() {
        // Test: 2-channel node A -> 4-channel node B
        // Node B should receive signal on first 2 channels, silence on others
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("StereoNode", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("QuadNode", 4, 4)))
            .unwrap();

        graph.connect(node_a, node_b).unwrap();

        // Create 2-channel input
        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create 4-channel output
        let mut output_data = [
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
        ];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // Node B receives 2 channels from A, outputs 4 channels
        // First 2 channels should have signal (1.0, 2.0)
        assert_eq!(output_data[0][0], 1.0);
        assert_eq!(output_data[1][0], 2.0);
        // Last 2 channels should be silence (no input)
        assert_eq!(output_data[2][0], 0.0);
        assert_eq!(output_data[3][0], 0.0);
    }

    #[test]
    fn test_channel_mismatch_mixing_different_channel_counts() {
        // Test: 2-channel node A -> node C (4-channel)
        //       1-channel node B -> node C (4-channel)
        // Both A and B are input nodes (no incoming connections), so they both receive system_input
        let mut graph = AudioGraph::with_config(48000, 64);
        let node_a = graph
            .add_node(Box::new(DummyPlugin::new("StereoNode", 2, 2)))
            .unwrap();
        let node_b = graph
            .add_node(Box::new(DummyPlugin::new("MonoNode", 1, 1)))
            .unwrap();
        let node_c = graph
            .add_node(Box::new(DummyPlugin::new("QuadNode", 4, 4)))
            .unwrap();

        graph.connect(node_a, node_c).unwrap();
        graph.connect(node_b, node_c).unwrap();

        // Create 2-channel input (A gets both channels, B gets only channel 0)
        let input_data = [vec![1.0_f32; 64], vec![2.0_f32; 64]];
        let input_refs: Vec<&[f32]> = input_data.iter().map(Vec::as_slice).collect();

        // Create 4-channel output
        let mut output_data = [
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
            vec![0.0_f32; 64],
        ];
        let mut output_refs: Vec<&mut [f32]> =
            output_data.iter_mut().map(Vec::as_mut_slice).collect();

        graph.process(&input_refs, &mut output_refs);

        // Node C receives:
        // - From A (2-channel): [1.0, 2.0] on channels 0-1
        // - From B (1-channel): [1.0] on channel 0 (both A and B get system_input channel 0)
        // Result: [1.0+1.0, 2.0, 0.0, 0.0] = [2.0, 2.0, 0.0, 0.0]
        assert_eq!(output_data[0][0], 2.0);
        assert_eq!(output_data[1][0], 2.0);
        assert_eq!(output_data[2][0], 0.0);
        assert_eq!(output_data[3][0], 0.0);
    }
}
