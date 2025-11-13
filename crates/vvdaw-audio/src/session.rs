//! Session file format for saving/loading audio graphs.
//!
//! Uses RON (Rust Object Notation) for human-readable, version-control-friendly
//! serialization of the audio graph, plugin configurations, and parameters.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::graph::{AudioGraph, PluginSource};
use vvdaw_plugin::Plugin;

/// Specification for how to instantiate a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginSpec {
    /// VST3 plugin loaded from a bundle path
    Vst3 {
        /// Path to the .vst3 bundle (e.g., "/Library/Audio/Plug-Ins/VST3/MyPlugin.vst3")
        path: PathBuf,

        /// Parameter values (parameter ID -> normalized value 0.0-1.0)
        #[serde(default)]
        parameters: HashMap<u32, f64>,
    },
    // Future plugin types:
    // Clap { path: PathBuf, parameters: HashMap<u32, f64> },
    // BuiltIn { plugin_type: String, config: serde_json::Value },
}

/// A node in the session graph (serializable version of `AudioNode`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNode {
    /// Unique node ID within this session
    pub id: usize,

    /// Plugin specification
    pub plugin: PluginSpec,

    /// Number of input channels
    pub inputs: usize,

    /// Number of output channels
    pub outputs: usize,
}

/// Connection between two nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionConnection {
    /// Source node ID
    pub from: usize,

    /// Destination node ID
    pub to: usize,
}

/// The complete audio graph structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGraph {
    /// All nodes in the graph
    pub nodes: Vec<SessionNode>,

    /// Connections between nodes
    pub connections: Vec<SessionConnection>,
}

/// Top-level session structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Format version for future compatibility
    pub version: u32,

    /// Human-readable session name
    pub name: String,

    /// Sample rate (Hz)
    pub sample_rate: u32,

    /// Processing block size (frames)
    pub block_size: usize,

    /// The audio graph
    pub graph: SessionGraph,
}

impl Session {
    /// Create a new empty session
    #[must_use]
    pub fn new(name: impl Into<String>, sample_rate: u32, block_size: usize) -> Self {
        Self {
            version: 1,
            name: name.into(),
            sample_rate,
            block_size,
            graph: SessionGraph {
                nodes: Vec::new(),
                connections: Vec::new(),
            },
        }
    }

    /// Save session to a RON file
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be written or serialization fails
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let ron_string = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| SessionError::SerializationFailed(e.to_string()))?;

        std::fs::write(path.as_ref(), ron_string)
            .map_err(|e| SessionError::IoError(e.to_string()))?;

        Ok(())
    }

    /// Load session from a RON file
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be read or deserialization fails
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let ron_string = std::fs::read_to_string(path.as_ref())
            .map_err(|e| SessionError::IoError(e.to_string()))?;

        let session: Self = ron::from_str(&ron_string)
            .map_err(|e| SessionError::DeserializationFailed(e.to_string()))?;

        // Validate version
        if session.version > 1 {
            return Err(SessionError::UnsupportedVersion(session.version));
        }

        Ok(session)
    }

    /// Create a session from an existing audio graph
    ///
    /// # Errors
    ///
    /// Returns error if any node has an unknown plugin source (cannot be serialized)
    pub fn from_graph(graph: &AudioGraph, name: impl Into<String>) -> Result<Self, SessionError> {
        let mut nodes = Vec::new();
        let mut connections = Vec::new();

        // Convert nodes
        for node in graph.nodes() {
            // Extract parameter values
            let mut parameters = HashMap::new();
            for param in node.plugin().parameters() {
                if let Ok(value) = node.plugin().get_parameter(param.id) {
                    parameters.insert(param.id, f64::from(value));
                }
            }

            // Convert plugin source to spec
            let plugin_spec = match node.source() {
                PluginSource::Vst3 { path } => PluginSpec::Vst3 {
                    path: path.clone(),
                    parameters,
                },
                PluginSource::Unknown => {
                    return Err(SessionError::InvalidData(format!(
                        "Node {} has unknown plugin source and cannot be serialized",
                        node.id()
                    )));
                }
            };

            nodes.push(SessionNode {
                id: node.id(),
                plugin: plugin_spec,
                inputs: node.inputs(),
                outputs: node.outputs(),
            });
        }

        // Convert connections
        for conn in graph.connections() {
            connections.push(SessionConnection {
                from: conn.from,
                to: conn.to,
            });
        }

        Ok(Self {
            version: 1,
            name: name.into(),
            sample_rate: graph.sample_rate(),
            block_size: graph.block_size(),
            graph: SessionGraph { nodes, connections },
        })
    }

    /// Reconstruct an audio graph from this session
    ///
    /// The `plugin_loader` callback is responsible for instantiating plugins
    /// based on their `PluginSpec`. This avoids circular dependencies between crates.
    ///
    /// # Errors
    ///
    /// Returns error if plugin instantiation fails or graph construction fails
    pub fn to_graph<F>(&self, mut plugin_loader: F) -> Result<AudioGraph, SessionError>
    where
        F: FnMut(&PluginSpec) -> Result<Box<dyn Plugin>, String>,
    {
        let mut graph = AudioGraph::with_config(self.sample_rate, self.block_size);
        let mut node_id_map = HashMap::new(); // Session ID -> Graph ID

        // Create all nodes
        for session_node in &self.graph.nodes {
            // Load the plugin
            let plugin = plugin_loader(&session_node.plugin)
                .map_err(|e| SessionError::InvalidData(format!("Failed to load plugin: {e}")))?;

            // Determine plugin source for the graph node
            let source = match &session_node.plugin {
                PluginSpec::Vst3 { path, .. } => PluginSource::Vst3 { path: path.clone() },
            };

            // Add node to graph
            let graph_id = graph
                .add_node(plugin, source)
                .map_err(|e| SessionError::InvalidData(format!("Failed to add node: {e}")))?;

            node_id_map.insert(session_node.id, graph_id);

            // Set parameters
            // TODO: Add graph method to set parameter on node
            // For now, we rely on the plugin_loader to handle parameter initialization
            let _ = &session_node.plugin; // Suppress unused warning
        }

        // Create all connections
        for session_conn in &self.graph.connections {
            let from = node_id_map.get(&session_conn.from).ok_or_else(|| {
                SessionError::InvalidData(format!(
                    "Connection from unknown node {}",
                    session_conn.from
                ))
            })?;

            let to = node_id_map.get(&session_conn.to).ok_or_else(|| {
                SessionError::InvalidData(format!("Connection to unknown node {}", session_conn.to))
            })?;

            graph
                .connect(*from, *to)
                .map_err(|e| SessionError::InvalidData(format!("Failed to connect nodes: {e}")))?;
        }

        Ok(graph)
    }
}

/// Errors that can occur during session operations
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Unsupported session version: {0}")]
    UnsupportedVersion(u32),

    #[error("Invalid session data: {0}")]
    InvalidData(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session::new("Test Session", 48000, 512);
        assert_eq!(session.version, 1);
        assert_eq!(session.name, "Test Session");
        assert_eq!(session.sample_rate, 48000);
        assert_eq!(session.block_size, 512);
        assert!(session.graph.nodes.is_empty());
        assert!(session.graph.connections.is_empty());
    }

    #[test]
    fn test_session_serialization() {
        let mut session = Session::new("Test", 48000, 512);

        // Add a VST3 node
        session.graph.nodes.push(SessionNode {
            id: 0,
            plugin: PluginSpec::Vst3 {
                path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/TestPlugin.vst3"),
                parameters: HashMap::from([(0, 0.5), (1, 0.75)]),
            },
            inputs: 2,
            outputs: 2,
        });

        session.graph.nodes.push(SessionNode {
            id: 1,
            plugin: PluginSpec::Vst3 {
                path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/Reverb.vst3"),
                parameters: HashMap::new(),
            },
            inputs: 2,
            outputs: 2,
        });

        session
            .graph
            .connections
            .push(SessionConnection { from: 0, to: 1 });

        // Serialize to RON
        let ron_string = ron::ser::to_string_pretty(&session, ron::ser::PrettyConfig::default())
            .expect("Serialization should succeed");

        println!("Serialized session:\n{ron_string}");

        // Deserialize back
        let deserialized: Session =
            ron::from_str(&ron_string).expect("Deserialization should succeed");

        assert_eq!(deserialized.name, session.name);
        assert_eq!(deserialized.graph.nodes.len(), 2);
        assert_eq!(deserialized.graph.connections.len(), 1);
    }
}
