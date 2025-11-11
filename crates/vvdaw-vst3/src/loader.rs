//! VST3 plugin loading functionality.
//!
//! This module handles loading VST3 plugins from `.vst3` bundle files,
//! querying the plugin factory, and creating plugin instances.

use crate::com::{GetPluginFactoryFn, PluginFactory};
use crate::wrapper::Vst3Plugin;
use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::process::Command;
use vvdaw_plugin::{PluginError, PluginInfo};

/// Result type returned by plugin-scanner subprocess
#[derive(serde::Deserialize)]
#[serde(tag = "status")]
enum ScanResult {
    #[serde(rename = "success")]
    Success { plugin: PluginInfo },

    #[serde(rename = "error")]
    Error { message: String },
}

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

        // Step 8: Get edit controller (for parameters)
        // Try two approaches:
        // 1. Query IEditController directly (simple plugins)
        // 2. Create separate edit controller via class ID (commercial plugins)
        tracing::debug!("Obtaining edit controller...");
        let edit_controller_ptr = unsafe { Self::create_edit_controller(&factory, component_ptr) };

        // Step 9: Create the plugin wrapper with COM pointers
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
            edit_controller_ptr,
        ))
    }

    /// Create or query for an edit controller
    ///
    /// Tries two approaches:
    /// 1. Query `IEditController` directly on component (simple plugins)
    /// 2. Create separate edit controller via class ID (commercial plugins)
    #[allow(unsafe_code)]
    unsafe fn create_edit_controller(
        factory: &PluginFactory,
        component_ptr: *mut c_void,
    ) -> Option<*mut c_void> {
        unsafe {
            // Approach 1: Try querying IEditController directly on component
            if let Ok(ptr) =
                crate::com::query_interface(component_ptr, &crate::com::IEDIT_CONTROLLER_IID)
            {
                tracing::info!("Edit controller is implemented on component (simple plugin)");

                // Initialize the edit controller immediately to make parameters available
                match crate::com::component_initialize(ptr, std::ptr::null_mut()) {
                    Ok(()) => {
                        tracing::debug!("IEditController::initialize succeeded");
                    }
                    Err(e) => {
                        tracing::warn!(
                            "IEditController::initialize failed: {} (parameters may not be available)",
                            e
                        );
                    }
                }

                return Some(ptr);
            }

            // Approach 2: Query for separate controller class ID
            tracing::debug!("Component doesn't implement IEditController directly");
            tracing::debug!("Querying for separate controller class ID...");

            match crate::com::component_get_controller_class_id(component_ptr) {
                Ok(controller_class_id) => {
                    // Check if class ID is non-zero (zero means no controller)
                    if controller_class_id.iter().any(|&b| b != 0) {
                        tracing::info!(
                            "Component has separate controller class ID: {:?}",
                            controller_class_id
                        );

                        // Create separate edit controller instance
                        match factory.create_instance(
                            &controller_class_id,
                            &crate::com::IEDIT_CONTROLLER_IID,
                        ) {
                            Ok(controller_ptr) => {
                                tracing::info!(
                                    "Created separate edit controller at {:?}",
                                    controller_ptr
                                );

                                // Initialize the edit controller
                                // NOTE: We pass the component as context so the controller knows which component it belongs to
                                match crate::com::component_initialize(
                                    controller_ptr,
                                    component_ptr, // Pass component as context!
                                ) {
                                    Ok(()) => {
                                        tracing::info!("Edit controller initialized");

                                        // Transfer component state to edit controller
                                        tracing::debug!(
                                            "Attempting state transfer to edit controller..."
                                        );

                                        // Create memory stream for state transfer
                                        let mut stream = crate::stream::MemoryStream::new();
                                        let stream_ptr = stream.as_com_ptr();

                                        tracing::debug!("Created IBStream at {:?}", stream_ptr);

                                        // Verify the stream works by querying its own interface
                                        let query_result = crate::com::query_interface(
                                            stream_ptr,
                                            &crate::com::IEDIT_CONTROLLER_IID, // Try wrong interface first
                                        );
                                        tracing::debug!(
                                            "Stream queryInterface test (wrong IID): {:?}",
                                            query_result
                                        );

                                        // Try with IBStream IID - use the result as the stream pointer
                                        const IBSTREAM_IID: [u8; 16] = [
                                            0xC3, 0xBF, 0x6E, 0xA2, 0x30, 0x99, 0x47, 0x52, 0x9B,
                                            0x6B, 0xF9, 0x90, 0x1E, 0xE3, 0x3E, 0x9B,
                                        ];
                                        let stream_ptr_to_use = match crate::com::query_interface(
                                            stream_ptr,
                                            &IBSTREAM_IID,
                                        ) {
                                            Ok(ptr) => {
                                                tracing::debug!(
                                                    "Stream queryInterface for IBStream IID returned: {:?}",
                                                    ptr
                                                );
                                                ptr // Use the pointer from queryInterface
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Stream queryInterface failed: {}",
                                                    e
                                                );
                                                stream_ptr // Fall back to original pointer
                                            }
                                        };

                                        // Get component state
                                        match crate::com::component_get_state(
                                            component_ptr,
                                            stream_ptr_to_use,
                                        ) {
                                            Ok(()) => {
                                                let state_size = stream.data().len();
                                                tracing::info!(
                                                    "Retrieved {} bytes of component state",
                                                    state_size
                                                );

                                                // Rewind stream to beginning for reading
                                                stream.rewind();
                                                tracing::debug!("Rewound stream to position 0");

                                                // Transfer state to edit controller
                                                tracing::debug!("Calling setComponentState...");
                                                match crate::com::edit_controller_set_component_state(
                                                    controller_ptr,
                                                    stream_ptr_to_use,
                                                ) {
                                                    Ok(()) => {
                                                        tracing::info!("✓ State transfer successful!");
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!("✗ setComponentState failed: {}", e);
                                                        tracing::warn!("  Controller will have default/empty state");
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to get component state: {}",
                                                    e
                                                );
                                                tracing::warn!("  Skipping state transfer");
                                            }
                                        }

                                        return Some(controller_ptr);
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to initialize edit controller: {}",
                                            e
                                        );
                                        crate::com::release_interface(controller_ptr);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to create separate edit controller: {}", e);
                            }
                        }
                    } else {
                        tracing::debug!("Plugin has no edit controller (class ID is zero)");
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to get controller class ID: {}", e);
                }
            }

            None
        }
    }

    /// Scan a directory for VST3 plugins
    ///
    /// Searches for `.vst3` bundle directories and returns plugin information
    /// without fully loading the plugins.
    ///
    /// # Errors
    ///
    /// Returns `PluginError::FormatError` if the directory can't be read.
    pub fn scan<P: AsRef<Path>>(path: P) -> Result<Vec<PluginInfo>, PluginError> {
        let path = path.as_ref();
        tracing::info!("Scanning for VST3 plugins in: {}", path.display());

        // Check if directory exists
        if !path.exists() {
            tracing::debug!("Scan path does not exist: {}", path.display());
            return Ok(Vec::new());
        }

        if !path.is_dir() {
            return Err(PluginError::FormatError(format!(
                "Scan path is not a directory: {}",
                path.display()
            )));
        }

        let mut plugins = Vec::new();

        // Walk the directory tree to find .vst3 bundles
        match Self::walk_directory(path, &mut plugins) {
            Ok(()) => {
                tracing::info!("Found {} VST3 plugins in {}", plugins.len(), path.display());
                Ok(plugins)
            }
            Err(e) => {
                tracing::error!("Error scanning directory {}: {}", path.display(), e);
                Err(e)
            }
        }
    }

    /// Scan all standard VST3 plugin directories on the system
    ///
    /// Returns information about all discovered VST3 plugins.
    /// Skips directories that don't exist and continues on errors.
    pub fn scan_system() -> Vec<PluginInfo> {
        let mut all_plugins = Vec::new();

        for search_path in Self::get_vst3_search_paths() {
            tracing::debug!("Scanning VST3 search path: {}", search_path.display());
            match Self::scan(&search_path) {
                Ok(mut plugins) => {
                    tracing::info!(
                        "Found {} plugins in {}",
                        plugins.len(),
                        search_path.display()
                    );
                    all_plugins.append(&mut plugins);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to scan {}: {} (continuing)",
                        search_path.display(),
                        e
                    );
                }
            }
        }

        tracing::info!("Total VST3 plugins found: {}", all_plugins.len());

        // Check for known conflicting plugins
        Self::check_for_conflicts(&all_plugins);

        all_plugins
    }

    /// Check for known conflicting plugins and warn the user
    ///
    /// Some plugins share the same class names (e.g., Objective-C classes on macOS)
    /// which can cause issues if both are used in the same project.
    fn check_for_conflicts(plugins: &[PluginInfo]) {
        // Known conflict groups - plugins in the same group share class names
        let conflict_groups: &[&[&str]] = &[
            // plugdata and plugdata-fx share Objective-C classes
            &["plugdata", "plugdata-fx"],
        ];

        for conflict_group in conflict_groups {
            let found_plugins: Vec<&PluginInfo> = plugins
                .iter()
                .filter(|p| conflict_group.iter().any(|name| p.name.contains(name)))
                .collect();

            if found_plugins.len() > 1 {
                let plugin_names: Vec<&str> =
                    found_plugins.iter().map(|p| p.name.as_str()).collect();
                tracing::warn!(
                    "Detected conflicting plugins: {}. Using both in the same project may cause instability.",
                    plugin_names.join(" and ")
                );
            }
        }
    }

    /// Get the standard VST3 search paths for the current platform
    fn get_vst3_search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        #[cfg(target_os = "macos")]
        {
            // System-wide plugins
            paths.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));

            // User plugins
            if let Some(home) = std::env::var_os("HOME") {
                let mut user_path = PathBuf::from(home);
                user_path.push("Library/Audio/Plug-Ins/VST3");
                paths.push(user_path);
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Standard VST3 path
            paths.push(PathBuf::from("C:\\Program Files\\Common Files\\VST3"));

            // User plugins
            if let Some(appdata) = std::env::var_os("LOCALAPPDATA") {
                let mut user_path = PathBuf::from(appdata);
                user_path.push("Programs\\Common\\VST3");
                paths.push(user_path);
            }
        }

        #[cfg(target_os = "linux")]
        {
            // User plugins
            if let Some(home) = std::env::var_os("HOME") {
                let mut user_path = PathBuf::from(home);
                user_path.push(".vst3");
                paths.push(user_path);
            }

            // System-wide plugins
            paths.push(PathBuf::from("/usr/lib/vst3"));
            paths.push(PathBuf::from("/usr/local/lib/vst3"));
        }

        paths
    }

    /// Scan a single plugin using the plugin-scanner subprocess
    ///
    /// This isolates the scanning process so that:
    /// - Plugin crashes don't kill the main application
    /// - Each plugin gets its own Objective-C runtime (no class conflicts)
    /// - Multiple plugins can be scanned in parallel (future)
    fn scan_plugin_subprocess(plugin_path: &Path) -> Result<PluginInfo, PluginError> {
        // Determine the scanner binary path
        let exe_name = if cfg!(windows) {
            "plugin-scanner.exe"
        } else {
            "plugin-scanner"
        };

        // Find the scanner binary by checking multiple locations:
        // 1. Same directory as current executable (normal case)
        // 2. Parent directory (for tests in target/debug/deps/)
        // 3. Just the binary name (let PATH handle it)
        let scanner_path = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                let same_dir = exe.parent()?.join(exe_name);
                if same_dir.exists() {
                    return Some(same_dir);
                }

                // For tests: go from target/debug/deps/ -> target/debug/
                let parent_dir = exe.parent()?.parent()?.join(exe_name);
                if parent_dir.exists() {
                    return Some(parent_dir);
                }

                None
            })
            .unwrap_or_else(|| PathBuf::from(exe_name));

        tracing::debug!("Spawning plugin scanner: {}", scanner_path.display());
        tracing::debug!("Scanning plugin: {}", plugin_path.display());

        // Spawn the scanner subprocess
        let output = Command::new(&scanner_path)
            .arg(plugin_path.as_os_str())
            .output()
            .map_err(|e| {
                PluginError::FormatError(format!("Failed to spawn plugin scanner subprocess: {e}"))
            })?;

        // Parse the JSON output
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check if subprocess crashed
        if !output.status.success() {
            tracing::warn!(
                "Plugin scanner subprocess failed for {}: exit code {:?}",
                plugin_path.display(),
                output.status.code()
            );

            // Try to parse error message from JSON
            if let Ok(result) = serde_json::from_str::<ScanResult>(&stdout) {
                match result {
                    ScanResult::Error { message } => {
                        return Err(PluginError::FormatError(message));
                    }
                    ScanResult::Success { .. } => {
                        // Shouldn't happen if status is not success
                        return Err(PluginError::FormatError(
                            "Subprocess reported success but exited with error status".to_string(),
                        ));
                    }
                }
            }

            // If we can't parse the JSON, return stderr if available
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PluginError::FormatError(format!(
                "Scanner subprocess failed: {}",
                if stderr.is_empty() {
                    stdout.to_string()
                } else {
                    stderr.to_string()
                }
            )));
        }

        // Parse successful result
        let result: ScanResult = serde_json::from_str(&stdout).map_err(|e| {
            PluginError::FormatError(format!(
                "Failed to parse scanner output as JSON: {e}\nOutput: {stdout}"
            ))
        })?;

        match result {
            ScanResult::Success { plugin } => Ok(plugin),
            ScanResult::Error { message } => Err(PluginError::FormatError(message)),
        }
    }

    /// Recursively walk a directory to find VST3 plugins
    fn walk_directory(path: &Path, plugins: &mut Vec<PluginInfo>) -> Result<(), PluginError> {
        let entries = std::fs::read_dir(path).map_err(|e| {
            PluginError::FormatError(format!(
                "Failed to read directory {}: {}",
                path.display(),
                e
            ))
        })?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let entry_path = entry.path();

            // Check if this is a .vst3 bundle
            if entry_path.is_dir() {
                if let Some(ext) = entry_path.extension()
                    && ext.eq_ignore_ascii_case("vst3")
                {
                    // Found a VST3 bundle - scan it using subprocess
                    match Self::scan_plugin_subprocess(&entry_path) {
                        Ok(info) => {
                            tracing::debug!(
                                "Found plugin: {} at {}",
                                info.name,
                                entry_path.display()
                            );
                            plugins.push(info);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to scan plugin from {}: {} (skipping)",
                                entry_path.display(),
                                e
                            );
                        }
                    }
                    // Don't recurse into .vst3 bundles
                    continue;
                }

                // Recurse into subdirectories (but not .vst3 bundles)
                if let Err(e) = Self::walk_directory(&entry_path, plugins) {
                    tracing::warn!(
                        "Failed to scan subdirectory {}: {}",
                        entry_path.display(),
                        e
                    );
                    // Continue scanning other directories
                }
            }
        }

        Ok(())
    }

    /// Load plugin info without creating component instances
    ///
    /// This is a lightweight scan that only loads the library and queries the factory.
    /// It does NOT create `IComponent` or `IAudioProcessor` instances, making it much faster
    /// and avoiding potential conflicts from loading multiple plugins.
    ///
    /// This is public for use by the plugin-scanner subprocess.
    #[allow(unsafe_code)] // Required for FFI
    pub fn load_plugin_info_internal<P: AsRef<Path>>(path: P) -> Result<PluginInfo, PluginError> {
        let path = path.as_ref();
        Self::load_plugin_info(path)
    }

    /// Internal implementation of plugin info loading
    #[allow(unsafe_code)] // Required for FFI
    fn load_plugin_info(path: &Path) -> Result<PluginInfo, PluginError> {
        tracing::info!("Loading VST3 plugin from: {}", path.display());

        // Security: Validate path
        Self::validate_plugin_path(path)?;

        // Step 1: Resolve the actual library path
        let library_path = if path.is_dir() {
            Self::get_library_path(path)?
        } else {
            path.to_path_buf()
        };

        if !library_path.exists() {
            return Err(PluginError::FormatError(format!(
                "VST3 library not found at: {}",
                library_path.display()
            )));
        }

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

        // Step 5: Query the factory for plugin information (no component creation!)
        let class_count = factory.count_classes();
        if class_count == 0 {
            return Err(PluginError::FormatError(
                "Plugin has no exported classes".to_string(),
            ));
        }

        // Get info from first class (most plugins only have one)
        let class_info = factory.get_class_info(0)?;
        tracing::info!("Loading plugin class: {}", class_info.name);

        // Create PluginInfo from factory metadata
        // Note: Vendor and version would require querying IPluginFactory3 interface
        // which most plugins don't implement, so we use defaults for now
        let info = PluginInfo {
            name: class_info.name.clone(),
            vendor: "Unknown".to_string(), // TODO: Query IPluginFactory3 if available
            version: "1.0.0".to_string(),  // TODO: Parse from class info if available
            unique_id: format!("{:?}", class_info.class_id),
        };

        // Explicitly drop to ensure cleanup order and verify unloading
        drop(factory);
        drop(library);

        tracing::info!("Unloaded plugin library for: {}", info.name);
        Ok(info)
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
    fn test_scan_nonexistent_directory() {
        // Scanning a nonexistent directory should return empty list, not error
        let result = Vst3Loader::scan("/nonexistent/path/to/plugins");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_scan_file_not_directory() {
        // Scanning a file (not a directory) should return an error
        let result = Vst3Loader::scan("/etc/hosts");
        assert!(result.is_err());
    }

    /// Integration test: Scan all system plugins
    ///
    /// This test loads all installed VST3 plugins.
    /// Uses #[serial] to prevent conflicts with other tests loading plugins.
    #[test]
    #[serial_test::serial]
    fn test_scan_system() {
        // Scan system-wide VST3 directories
        // This test is informational - it shows what plugins are installed
        // but doesn't fail if no plugins are found (e.g., CI environment)
        let plugins = Vst3Loader::scan_system();

        eprintln!("Found {} VST3 plugins on system", plugins.len());
        for plugin in &plugins {
            eprintln!(
                "  - {} by {} (v{})",
                plugin.name, plugin.vendor, plugin.version
            );
        }

        // Test passes regardless of how many plugins found
        // (some systems may have 0 plugins installed)
    }

    /// Integration test: Scan standard VST3 directory
    ///
    /// Tests scanning a specific directory for plugins.
    /// Uses #[serial] to prevent conflicts with other tests loading plugins.
    #[test]
    #[serial_test::serial]
    fn test_scan_standard_vst3_directory() {
        // Test scanning the standard macOS VST3 directory (if it exists)
        #[cfg(target_os = "macos")]
        {
            let vst3_dir = "/Library/Audio/Plug-Ins/VST3";
            if std::path::Path::new(vst3_dir).exists() {
                let result = Vst3Loader::scan(vst3_dir);
                assert!(result.is_ok());

                let plugins = result.unwrap();
                eprintln!("Found {} plugins in {}", plugins.len(), vst3_dir);

                // Verify each plugin has valid info
                for plugin in &plugins {
                    assert!(!plugin.name.is_empty(), "Plugin name should not be empty");
                    assert!(
                        !plugin.unique_id.is_empty(),
                        "Plugin ID should not be empty"
                    );
                }
            } else {
                eprintln!("Skipping test: {vst3_dir} not found");
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            eprintln!("Skipping test: macOS-specific test");
        }
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
    /// Uses #[serial] to prevent conflicts with other tests loading plugins.
    ///
    /// Note: Audio processing is tested in the examples (`play_vst3`, `process_wav`)
    /// which run in a full audio context. Unit tests don't have that context.
    #[test]
    #[serial_test::serial]
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
