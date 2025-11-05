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
        let class_count = factory.count_classes()?;
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

        // Step 6: Create the plugin wrapper
        // TODO: Actually create IComponent and IAudioProcessor instances
        let info = PluginInfo {
            name: class_info.name.clone(),
            vendor: "Unknown".to_string(), // TODO: Get from factory info
            version: "1.0.0".to_string(),  // TODO: Get from class info
            unique_id: format!("{:?}", class_info.class_id),
        };

        Ok(Vst3Plugin::new_with_library(info, library, factory))
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_exists() {
        // Basic test to ensure module compiles
        let result = Vst3Loader::scan("/nonexistent");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
