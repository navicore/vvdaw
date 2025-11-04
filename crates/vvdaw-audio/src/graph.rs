//! Audio processing graph.

use vvdaw_plugin::Plugin;

/// A node in the audio graph (typically wraps a plugin)
pub struct AudioNode {
    pub id: usize,
    pub plugin: Box<dyn Plugin>,
}

/// The audio processing graph
pub struct AudioGraph {
    nodes: Vec<AudioNode>,
    next_id: usize,
}

impl AudioGraph {
    /// Create a new empty audio graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            next_id: 0,
        }
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, plugin: Box<dyn Plugin>) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        self.nodes.push(AudioNode { id, plugin });
        id
    }

    /// Remove a node from the graph
    pub fn remove_node(&mut self, id: usize) -> Option<AudioNode> {
        self.nodes
            .iter()
            .position(|n| n.id == id)
            .map(|pos| self.nodes.remove(pos))
    }

    /// Process all nodes in the graph
    pub fn process(&mut self) {
        // TODO: Implement topological sort and processing
        tracing::trace!("Processing {} nodes", self.nodes.len());
    }
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}
