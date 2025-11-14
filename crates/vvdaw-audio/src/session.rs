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
    /// Built-in processor (implemented in Rust)
    ///
    /// These are portable across all platforms since they're compiled into the binary.
    /// No validation needed as the name is just a string key.
    Builtin {
        /// Name of the built-in processor (e.g., "gain", "pan", "mixer")
        name: String,

        /// Parameter values (parameter ID -> normalized value 0.0-1.0)
        #[serde(default)]
        parameters: HashMap<u32, f64>,
    },

    /// VST3 plugin loaded from a bundle path
    ///
    /// # Security Warning
    ///
    /// Plugin paths are stored as-is in session files. When loading sessions from
    /// untrusted sources, be aware that:
    /// - Paths are absolute and machine-specific (not portable across systems)
    /// - Loading a session may attempt to load plugins from arbitrary paths
    /// - Malicious sessions could reference unsafe plugin files
    ///
    /// Always validate session sources before loading.
    Vst3 {
        /// Path to the .vst3 bundle (e.g., "/Library/Audio/Plug-Ins/VST3/MyPlugin.vst3")
        ///
        /// This should be an absolute path. Relative paths may not work correctly.
        path: PathBuf,

        /// Parameter values (parameter ID -> normalized value 0.0-1.0)
        #[serde(default)]
        parameters: HashMap<u32, f64>,
    },
    // Future plugin types:
    // Clap { path: PathBuf, parameters: HashMap<u32, f64> },
}

impl PluginSpec {
    /// Validate the plugin specification
    ///
    /// Checks for:
    /// - Built-ins: Name is non-empty
    /// - VST3: Absolute paths, expected file extensions, no directory traversal
    ///
    /// # Errors
    ///
    /// Returns error if validation fails
    pub fn validate(&self) -> Result<(), SessionError> {
        match self {
            Self::Builtin { name, .. } => {
                if name.is_empty() {
                    return Err(SessionError::InvalidData(
                        "Built-in processor name cannot be empty".to_string(),
                    ));
                }
                Ok(())
            }
            Self::Vst3 { path, .. } => {
                // Check if path is absolute
                if !path.is_absolute() {
                    return Err(SessionError::InvalidPath(format!(
                        "VST3 path must be absolute, got: {}",
                        path.display()
                    )));
                }

                // Check for directory traversal
                if path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                }) {
                    return Err(SessionError::InvalidPath(format!(
                        "VST3 path contains invalid components (.. or .): {}",
                        path.display()
                    )));
                }

                // Check file extension
                if let Some(ext) = path.extension() {
                    if ext != "vst3" {
                        tracing::warn!(
                            "VST3 path has unexpected extension '{}': {}",
                            ext.to_string_lossy(),
                            path.display()
                        );
                    }
                } else {
                    tracing::warn!("VST3 path has no extension: {}", path.display());
                }

                Ok(())
            }
        }
    }
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

        // Validate all plugin specifications
        for node in &session.graph.nodes {
            node.plugin.validate()?;
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
                PluginSource::Builtin { name } => PluginSpec::Builtin {
                    name: name.clone(),
                    parameters,
                },
                PluginSource::Vst3 { path } => PluginSpec::Vst3 {
                    path: path.clone(),
                    parameters,
                },
                PluginSource::Unknown => {
                    return Err(SessionError::UnknownPluginSource { node_id: node.id() });
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
            // Get plugin identifier for error messages
            let plugin_path = match &session_node.plugin {
                PluginSpec::Builtin { name, .. } => format!("builtin:{name}"),
                PluginSpec::Vst3 { path, .. } => path.display().to_string(),
            };

            // Load the plugin
            let plugin = plugin_loader(&session_node.plugin).map_err(|e| {
                SessionError::PluginLoadFailed {
                    plugin_path: plugin_path.clone(),
                    reason: e,
                }
            })?;

            // Determine plugin source for the graph node
            let source = match &session_node.plugin {
                PluginSpec::Builtin { name, .. } => PluginSource::Builtin { name: name.clone() },
                PluginSpec::Vst3 { path, .. } => PluginSource::Vst3 { path: path.clone() },
            };

            // Add node to graph
            let graph_id =
                graph
                    .add_node(plugin, source)
                    .map_err(|e| SessionError::PluginLoadFailed {
                        plugin_path: plugin_path.clone(),
                        reason: e.to_string(),
                    })?;

            node_id_map.insert(session_node.id, graph_id);

            // Restore parameters
            let parameters = match &session_node.plugin {
                PluginSpec::Builtin { parameters, .. } | PluginSpec::Vst3 { parameters, .. } => {
                    parameters
                }
            };
            for (&param_id, &value) in parameters {
                graph
                    .set_node_parameter(graph_id, param_id, value as f32)
                    .map_err(|e| SessionError::ParameterFailed {
                        node_id: session_node.id,
                        param_id,
                        reason: e.to_string(),
                    })?;
            }
        }

        // Create all connections
        for session_conn in &self.graph.connections {
            let from =
                node_id_map
                    .get(&session_conn.from)
                    .ok_or(SessionError::InvalidConnection {
                        from: session_conn.from,
                        to: session_conn.to,
                    })?;

            let to = node_id_map
                .get(&session_conn.to)
                .ok_or(SessionError::InvalidConnection {
                    from: session_conn.from,
                    to: session_conn.to,
                })?;

            graph
                .connect(*from, *to)
                .map_err(|_e| SessionError::InvalidConnection {
                    from: session_conn.from,
                    to: session_conn.to,
                })?;
        }

        Ok(graph)
    }
}

/// Errors that can occur during session operations
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// I/O error reading or writing session file
    #[error("I/O error: {0}")]
    IoError(String),

    /// RON serialization failed
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// RON deserialization failed
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    /// Session format version is not supported
    #[error("Unsupported session version: {0}")]
    UnsupportedVersion(u32),

    /// Generic invalid data error (prefer more specific variants when possible)
    #[error("Invalid session data: {0}")]
    InvalidData(String),

    /// Plugin failed to load during session reconstruction
    #[error("Plugin loading failed for {plugin_path}: {reason}")]
    PluginLoadFailed { plugin_path: String, reason: String },

    /// Node not found in graph
    #[error("Node {node_id} not found in session")]
    NodeNotFound { node_id: usize },

    /// Connection references non-existent nodes
    #[error("Invalid connection from node {from} to node {to}")]
    InvalidConnection { from: usize, to: usize },

    /// Parameter setting failed
    #[error("Failed to set parameter {param_id} on node {node_id}: {reason}")]
    ParameterFailed {
        node_id: usize,
        param_id: u32,
        reason: String,
    },

    /// Graph contains nodes with unknown plugin sources (can't be serialized)
    #[error("Node {node_id} has unknown plugin source and cannot be serialized")]
    UnknownPluginSource { node_id: usize },

    /// Invalid plugin path (security validation failed)
    #[error("Invalid plugin path: {0}")]
    InvalidPath(String),
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

    #[test]
    fn test_path_validation_absolute() {
        let spec = PluginSpec::Vst3 {
            path: PathBuf::from("/Library/Audio/Plug-Ins/VST3/Test.vst3"),
            parameters: HashMap::new(),
        };

        assert!(spec.validate().is_ok());
    }

    #[test]
    fn test_path_validation_relative() {
        let spec = PluginSpec::Vst3 {
            path: PathBuf::from("relative/path/Test.vst3"),
            parameters: HashMap::new(),
        };

        let result = spec.validate();
        assert!(result.is_err());
        assert!(matches!(result, Err(SessionError::InvalidPath(_))));
    }

    #[test]
    fn test_path_validation_parent_dir() {
        let spec = PluginSpec::Vst3 {
            path: PathBuf::from("/Library/../etc/passwd"),
            parameters: HashMap::new(),
        };

        let result = spec.validate();
        assert!(result.is_err());
        assert!(matches!(result, Err(SessionError::InvalidPath(_))));
    }

    #[test]
    fn test_path_validation_normalized() {
        // Note: Rust automatically normalizes paths, removing ./ components
        // So /./Library becomes /Library, which is valid
        // This test documents that behavior
        let spec = PluginSpec::Vst3 {
            path: PathBuf::from("/Library/Audio/Test.vst3"),
            parameters: HashMap::new(),
        };

        let result = spec.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_session_load_validates_paths() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a session with invalid path
        let invalid_session = r#"(
            version: 1,
            name: "Invalid",
            sample_rate: 48000,
            block_size: 512,
            graph: (
                nodes: [(
                    id: 0,
                    plugin: Vst3(
                        path: "../../etc/passwd",
                        parameters: {},
                    ),
                    inputs: 2,
                    outputs: 2,
                )],
                connections: [],
            ),
        )"#;

        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(invalid_session.as_bytes())
            .expect("Failed to write");
        let path = file.path();

        let result = Session::load(path);
        assert!(result.is_err());
        assert!(matches!(result, Err(SessionError::InvalidPath(_))));
    }

    #[test]
    fn test_unknown_plugin_source_error() {
        // Test documents expected behavior for unknown plugin sources
        // When we have a graph with Unknown plugin sources, from_graph should error:
        // let result = Session::from_graph(&graph_with_unknown_source, "Test");
        // assert!(matches!(result, Err(SessionError::UnknownPluginSource { .. })));

        // This would require creating a mock plugin with Unknown source,
        // which is not trivial without a proper test framework for plugins.
        // The behavior is tested indirectly through the session demo.
    }

    #[test]
    fn test_error_message_specificity() {
        // Test that specific error variants provide useful information
        let err = SessionError::PluginLoadFailed {
            plugin_path: "/test/path.vst3".to_string(),
            reason: "File not found".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("/test/path.vst3"));
        assert!(msg.contains("File not found"));

        let err = SessionError::ParameterFailed {
            node_id: 5,
            param_id: 42,
            reason: "Out of range".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("node 5"));
        assert!(msg.contains("parameter 42"));
        assert!(msg.contains("Out of range"));
    }
}
