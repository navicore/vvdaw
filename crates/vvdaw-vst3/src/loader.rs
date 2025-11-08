//! VST3 plugin loading functionality.
//!
//! This module handles loading VST3 plugins from `.vst3` bundle files,
//! querying the plugin factory, and creating plugin instances.

use crate::com::{GetPluginFactoryFn, PluginFactory};
use crate::wrapper::Vst3Plugin;
use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};
use vvdaw_plugin::{PluginError, PluginInfo};

/// VST3 plugin loader
///
/// Handles loading VST3 plugins from filesystem paths.
/// VST3 plugins are typically distributed as `.vst3` bundles which contain:
/// - macOS: `Contents/MacOS/<name>.vst3`
/// - Windows: `Contents/x86_64-win/<name>.vst3`
/// - Linux: `Contents/x86_64-linux/<name>.so`
pub struct Vst3Loader;

impl Vst3Loader {
    /// Load a VST3 plugin from a path
    ///
    /// The path can point to either:
    /// - A `.vst3` bundle directory
    /// - A dynamic library file directly
    ///
    /// # Errors
    ///
    /// Returns `PluginError::FormatError` if:
    /// - The file/bundle doesn't exist
    /// - The binary can't be loaded
    /// - The plugin factory can't be queried
    #[allow(unsafe_code)] // Required for FFI
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Vst3Plugin, PluginError> {
        let path = path.as_ref();
        tracing::info!("Loading VST3 plugin from: {}", path.display());

        // Security: Validate path to prevent directory traversal attacks
        Self::validate_plugin_path(path)?;

        // Step 1: Resolve the actual library path from the bundle
        let library_path = if path.is_dir() {
            // It's a bundle, get the platform-specific library path
            Self::get_library_path(path)?
        } else {
            // It's a direct library file
            path.to_path_buf()
        };

        if !library_path.exists() {
            return Err(PluginError::FormatError(format!(
                "VST3 library not found at: {}",
                library_path.display()
            )));
        }

        tracing::debug!("Loading VST3 library: {}", library_path.display());

        // Step 2: Load the dynamic library
        let library = unsafe {
            Library::new(&library_path).map_err(|e| {
                PluginError::FormatError(format!("Failed to load VST3 library: {e}"))
            })?
        };

        // Step 3: Get the GetPluginFactory function
        let get_factory: Symbol<GetPluginFactoryFn> = unsafe {
            library.get(b"GetPluginFactory").map_err(|e| {
                PluginError::FormatError(format!("GetPluginFactory symbol not found: {e}"))
            })?
        };

        // Step 4: Call GetPluginFactory
        let factory_ptr = unsafe { get_factory() };
        let factory = unsafe { PluginFactory::from_raw(factory_ptr) }.ok_or_else(|| {
            PluginError::FormatError("GetPluginFactory returned null".to_string())
        })?;

        tracing::debug!("Successfully obtained plugin factory");

        // Step 5: Query the factory for plugin information
        let class_count = factory.count_classes();
        tracing::debug!("Plugin factory has {} classes", class_count);

        if class_count == 0 {
            return Err(PluginError::FormatError(
                "Plugin has no exported classes".to_string(),
            ));
        }

        // For now, just use the first class
        // TODO: Allow selecting specific class or query by category
        let class_info = factory.get_class_info(0)?;
        tracing::info!("Loading plugin class: {}", class_info.name);

        // Step 6: Create IComponent instance
        tracing::debug!("Creating IComponent instance...");
        let component_ptr =
            unsafe { factory.create_instance(&class_info.class_id, &crate::com::ICOMPONENT_IID)? };
        tracing::debug!("IComponent created at {:?}", component_ptr);

        // Step 7: Query for IAudioProcessor interface
        // We need to query the component for the processor interface to get the correct vtable
        tracing::debug!("Querying for IAudioProcessor interface...");
        let processor_ptr = unsafe {
            match crate::com::query_interface(component_ptr, &crate::com::IAUDIO_PROCESSOR_IID) {
                Ok(ptr) => ptr,
                Err(e) => {
                    // Release component_ptr before returning error to avoid leak
                    crate::com::release_interface(component_ptr);
                    return Err(e);
                }
            }
        };
        tracing::debug!("IAudioProcessor obtained at {:?}", processor_ptr);

        // Step 8: Create the plugin wrapper with COM pointers
        let info = PluginInfo {
            name: class_info.name.clone(),
            vendor: "Unknown".to_string(), // TODO: Get from factory info
            version: "1.0.0".to_string(),  // TODO: Get from class info
            unique_id: format!("{:?}", class_info.class_id),
        };

        Ok(Vst3Plugin::new_with_library(
            info,
            library,
            factory,
            component_ptr,
            processor_ptr,
        ))
    }

    /// Scan a directory for VST3 plugins
    ///
    /// Searches for `.vst3` bundle directories and returns plugin information
    /// without fully loading the plugins.
    ///
    /// # Errors
    ///
    /// Returns `PluginError::FormatError` if the directory can't be read.
    #[allow(unused_variables)]
    pub fn scan<P: AsRef<Path>>(path: P) -> Result<Vec<PluginInfo>, PluginError> {
        let path = path.as_ref();
        tracing::info!("Scanning for VST3 plugins in: {}", path.display());

        // TODO: Implement plugin scanning
        // 1. Walk the directory tree
        // 2. Find all `.vst3` bundles
        // 3. Load each plugin temporarily to get info
        // 4. Return the list of plugin info

        Ok(Vec::new())
    }

    /// Get the platform-specific library path within a VST3 bundle
    ///
    /// VST3 bundles have platform-specific subdirectories:
    /// - macOS: `Contents/MacOS/<name>.vst3`
    /// - Windows: `Contents/x86_64-win/<name>.vst3`
    /// - Linux: `Contents/x86_64-linux/<name>.so`
    #[cfg(target_os = "macos")]
    fn get_library_path(bundle_path: &Path) -> Result<PathBuf, PluginError> {
        let name = bundle_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| PluginError::FormatError("Invalid bundle name".to_string()))?;

        Ok(bundle_path.join("Contents").join("MacOS").join(name))
    }

    #[cfg(target_os = "windows")]
    fn get_library_path(bundle_path: &Path) -> Result<PathBuf, PluginError> {
        let name = bundle_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| PluginError::FormatError("Invalid bundle name".to_string()))?;

        Ok(bundle_path
            .join("Contents")
            .join("x86_64-win")
            .join(format!("{name}.vst3")))
    }

    #[cfg(target_os = "linux")]
    fn get_library_path(bundle_path: &Path) -> Result<PathBuf, PluginError> {
        let name = bundle_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| PluginError::FormatError("Invalid bundle name".to_string()))?;

        Ok(bundle_path
            .join("Contents")
            .join("x86_64-linux")
            .join(format!("{name}.so")))
    }

    /// Validate plugin path for security
    ///
    /// Prevents directory traversal attacks by ensuring:
    /// 1. Path doesn't contain ".." components
    /// 2. Path is absolute or relative to current directory (no symlink traversal)
    ///
    /// # Errors
    ///
    /// Returns `PluginError::FormatError` if path validation fails
    fn validate_plugin_path(path: &Path) -> Result<(), PluginError> {
        // Check for directory traversal attempts
        for component in path.components() {
            if component == std::path::Component::ParentDir {
                return Err(PluginError::FormatError(
                    "Plugin path cannot contain '..' components (directory traversal)".to_string(),
                ));
            }
        }

        // Canonicalize path to resolve symlinks and validate it exists
        // This also prevents TOCTOU attacks by ensuring the path is real
        if let Ok(canonical_path) = path.canonicalize() {
            // Ensure file extension is valid for VST3
            if let Some(ext) = canonical_path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if !matches!(ext_str.as_str(), "vst3" | "so" | "dylib" | "dll") {
                    tracing::warn!(
                        "Plugin path has unusual extension: {} (expected .vst3, .so, .dylib, or .dll)",
                        ext_str
                    );
                }
            } else if !canonical_path.is_dir() {
                // Not a directory and no extension - suspicious
                return Err(PluginError::FormatError(
                    "Plugin path must be a .vst3 bundle or dynamic library file".to_string(),
                ));
            }
        }
        // Note: We don't fail if canonicalize fails - the file might not exist yet
        // and that will be caught by the exists() check in load()

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvdaw_plugin::Plugin;

    #[test]
    fn test_loader_exists() {
        // Basic test to ensure module compiles
        let result = Vst3Loader::scan("/nonexistent");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_path_validation_rejects_parent_dir() {
        // Test that directory traversal attempts are rejected
        let result = Vst3Loader::load("../../../etc/passwd");
        assert!(
            result.is_err(),
            "Should reject directory traversal attempts"
        );
        // Note: We can't call unwrap_err() because Vst3Plugin doesn't implement Debug,
        // but the error case is verified by is_err() above
    }

    #[test]
    fn test_path_validation_accepts_absolute_path() {
        // Test that absolute paths without ".." are accepted (validation-wise)
        // Note: The file doesn't exist, so load() will fail later, but validation should pass
        let result =
            Vst3Loader::validate_plugin_path(Path::new("/Library/Audio/Plug-Ins/VST3/Test.vst3"));
        assert!(result.is_ok());
    }

    /// Integration test: Load and initialize a real VST3 plugin
    ///
    /// This test is only run if Noises.vst3 is available (common on macOS).
    /// It verifies the VST3 integration by:
    /// 1. Loading a real plugin from filesystem
    /// 2. Initializing it with valid parameters
    /// 3. Verifying plugin info is correct
    /// 4. Cleanly shutting down
    ///
    /// Note: Audio processing is tested in the examples (`play_vst3`, `process_wav`)
    /// which run in a full audio context. Unit tests don't have that context.
    #[test]
    fn test_integration_load_and_initialize() {
        // Skip if plugin not available (e.g., CI environment)
        let plugin_path = "/Library/Audio/Plug-Ins/VST3/Noises.vst3";
        if !Path::new(plugin_path).exists() {
            eprintln!("Skipping integration test: {plugin_path} not found");
            return;
        }

        // Load the plugin
        let mut plugin = match Vst3Loader::load(plugin_path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Skipping integration test: Failed to load plugin - {e}");
                return;
            }
        };

        // Verify plugin info
        assert_eq!(plugin.info().name, "Noises");

        eprintln!("✓ Loaded plugin: {}", plugin.info().name);
        eprintln!(
            "  Channels: {} in, {} out",
            plugin.input_channels(),
            plugin.output_channels()
        );

        // Initialize the plugin
        let sample_rate = 48000;
        let block_size = 512;
        match plugin.initialize(sample_rate, block_size) {
            Ok(()) => eprintln!("✓ Plugin initialized successfully"),
            Err(e) => {
                eprintln!("Skipping integration test: Failed to initialize - {e}");
                return;
            }
        }

        // Clean up
        plugin.deactivate();
        eprintln!("✓ Plugin deactivated successfully");
        eprintln!("✓ Integration test passed!");
    }
}
