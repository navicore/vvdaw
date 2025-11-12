//! VST3 plugin wrapper that implements the Plugin trait.
//!
//! This module wraps VST3 COM interfaces (`IComponent`, `IAudioProcessor`)
//! and implements our format-agnostic Plugin trait.

use crate::com::PluginFactory;
use libloading::Library;
use vvdaw_core::{ChannelCount, Frames, SampleRate};
use vvdaw_plugin::{AudioBuffer, EventBuffer, ParameterInfo, Plugin, PluginError, PluginInfo};

/// VST3 plugin wrapper
///
/// Wraps a VST3 plugin's `IComponent` and `IAudioProcessor` interfaces
/// and implements our common Plugin trait.
pub struct Vst3Plugin {
    info: PluginInfo,

    // VST3-specific fields
    #[allow(dead_code)] // Will be used for COM calls
    library: Library,
    #[allow(dead_code)] // Will be used for COM calls
    factory: PluginFactory,

    // COM interface pointers
    component: *mut std::ffi::c_void,
    processor: *mut std::ffi::c_void,
    edit_controller: Option<*mut std::ffi::c_void>,

    // Audio configuration
    sample_rate: SampleRate,
    block_size: Frames,
    input_channels: ChannelCount,
    output_channels: ChannelCount,

    // Pre-allocated buffers for VST3 process calls
    // These avoid allocations in the audio hot path
    input_channel_ptrs: Vec<*mut f32>,
    output_channel_ptrs: Vec<*mut f32>,

    // Track activation state to avoid double-deactivation
    is_active: bool,
}

impl Vst3Plugin {
    /// Create a new VST3 plugin wrapper
    ///
    /// This is called by the loader after successfully loading the library
    /// and querying the VST3 factory.
    #[allow(dead_code)]
    pub(crate) fn new(_info: PluginInfo) -> Self {
        // This method is kept for backwards compatibility with tests
        // Real loading should use new_with_library
        panic!("Use new_with_library for actual VST3 plugins");
    }

    /// Create a new VST3 plugin wrapper with library, factory, and COM interfaces
    ///
    /// This is the proper constructor called by the loader.
    pub(crate) fn new_with_library(
        info: PluginInfo,
        library: Library,
        factory: PluginFactory,
        component: *mut std::ffi::c_void,
        processor: *mut std::ffi::c_void,
        edit_controller: Option<*mut std::ffi::c_void>,
    ) -> Self {
        // Pre-allocate channel pointer vectors (will be resized during initialization)
        let input_channels = 2;
        let output_channels = 2;

        Self {
            info,
            library,
            factory,
            component,
            processor,
            edit_controller,
            sample_rate: 48000,
            block_size: 512,
            input_channels,
            output_channels,
            input_channel_ptrs: Vec::with_capacity(input_channels),
            output_channel_ptrs: Vec::with_capacity(output_channels),
            is_active: false,
        }
    }
}

// SAFETY: VST3 plugins are designed to be used from the audio thread.
// The COM interface pointers are thread-safe as long as all calls are made from
// the same thread (which our architecture ensures - plugins live on audio thread).
// The Library and PluginFactory are already Send.
#[allow(unsafe_code)]
unsafe impl Send for Vst3Plugin {}

impl Plugin for Vst3Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    #[allow(unsafe_code)] // Required for FFI calls
    fn initialize(
        &mut self,
        sample_rate: SampleRate,
        max_block_size: Frames,
    ) -> Result<(), PluginError> {
        tracing::info!(
            "Initializing VST3 plugin '{}' at {} Hz, block size {}",
            self.info.name,
            sample_rate,
            max_block_size
        );

        self.sample_rate = sample_rate;
        self.block_size = max_block_size;

        // Resize channel pointer vectors to match configuration
        // This happens once during initialization, not in the audio hot path
        self.input_channel_ptrs.clear();
        self.input_channel_ptrs
            .resize(self.input_channels, std::ptr::null_mut());
        self.output_channel_ptrs.clear();
        self.output_channel_ptrs
            .resize(self.output_channels, std::ptr::null_mut());

        unsafe {
            // Step 1: Create host application context and initialize the component
            tracing::debug!("Creating host application context...");
            let host_app = crate::host_application::HostApplication::new();
            let host_app_ptr =
                std::ptr::from_mut(Box::leak(Box::new(host_app))).cast::<std::ffi::c_void>();
            tracing::debug!(
                "Created IHostApplication at {:?} for component",
                host_app_ptr
            );

            // Initialize component with host context
            crate::com::component_initialize(self.component, host_app_ptr)?;
            tracing::debug!("IComponent::initialize succeeded");

            // Step 1a: Connect component and controller via IConnectionPoint (if available)
            // This MUST happen before state transfer
            if let Some(edit_controller) = self.edit_controller {
                tracing::debug!("Establishing IConnectionPoint connections...");

                // Query component for IConnectionPoint
                let component_cp =
                    crate::com::query_interface(self.component, &crate::com::ICONNECTION_POINT_IID)
                        .map_or_else(
                            |_| {
                                tracing::debug!("Component does not implement IConnectionPoint");
                                None
                            },
                            |ptr| {
                                tracing::debug!(
                                    "Component implements IConnectionPoint at {:?}",
                                    ptr
                                );
                                Some(ptr)
                            },
                        );

                // Query controller for IConnectionPoint
                let controller_cp = crate::com::query_interface(
                    edit_controller,
                    &crate::com::ICONNECTION_POINT_IID,
                )
                .map_or_else(
                    |_| {
                        tracing::debug!("Controller does not implement IConnectionPoint");
                        None
                    },
                    |ptr| {
                        tracing::debug!("Controller implements IConnectionPoint at {:?}", ptr);
                        Some(ptr)
                    },
                );

                // Connect if both support IConnectionPoint
                if let (Some(comp_cp), Some(ctrl_cp)) = (component_cp, controller_cp) {
                    tracing::debug!("Connecting component and controller...");

                    // Connect component to controller
                    if let Err(e) = crate::com::connection_point_connect(comp_cp, ctrl_cp) {
                        tracing::warn!("Failed to connect component to controller: {}", e);
                    } else {
                        tracing::debug!("✓ Component -> Controller connection established");
                    }

                    // Connect controller to component (bidirectional)
                    if let Err(e) = crate::com::connection_point_connect(ctrl_cp, comp_cp) {
                        tracing::warn!("Failed to connect controller to component: {}", e);
                    } else {
                        tracing::debug!("✓ Controller -> Component connection established");
                    }

                    // Release our references (the connections keep them alive)
                    crate::com::release_interface(comp_cp);
                    crate::com::release_interface(ctrl_cp);
                } else {
                    tracing::debug!("IConnectionPoint not supported by both - skipping connection");
                }

                // Note: setComponentState is only for loading presets/saved sessions,
                // not for initialization. During initialization, the controller
                // starts with default parameter values and we can control them
                // directly via setParamNormalized().
                tracing::debug!("Edit controller ready with default parameter values");
            }

            // Step 2: Set up audio processing parameters
            let process_setup = crate::com::ProcessSetup {
                process_mode: 0,         // 0 = realtime
                symbolic_sample_size: 0, // 0 = 32-bit float
                max_samples_per_block: max_block_size as i32,
                sample_rate: f64::from(sample_rate),
            };
            crate::com::processor_setup_processing(self.processor, &process_setup)?;
            tracing::debug!("IAudioProcessor::setupProcessing succeeded");

            // Step 3: Check and activate audio buses
            // Media type: 0=audio, 1=event
            // Bus direction: 0=input, 1=output
            let input_bus_count = crate::com::component_get_bus_count(self.component, 0, 0);
            let output_bus_count = crate::com::component_get_bus_count(self.component, 0, 1);

            tracing::debug!(
                "Bus counts: {} inputs, {} outputs",
                input_bus_count,
                output_bus_count
            );

            // Activate audio buses if they exist
            if input_bus_count > 0 {
                tracing::debug!("Activating input bus 0...");
                crate::com::component_activate_bus(self.component, 0, 0, 0, true)?;
                tracing::debug!("Input bus 0 activated");
            }

            if output_bus_count > 0 {
                tracing::debug!("Activating output bus 0...");
                crate::com::component_activate_bus(self.component, 0, 1, 0, true)?;
                tracing::debug!("Output bus 0 activated");
            }

            // Step 4: Activate the component
            tracing::debug!("Calling IComponent::setActive(true)...");
            crate::com::component_set_active(self.component, true)?;
            tracing::debug!("IComponent::setActive(true) succeeded");

            // Step 4: Start audio processing
            tracing::debug!("Calling IAudioProcessor::setProcessing(true)...");
            crate::com::processor_set_processing(self.processor, true)?;
            tracing::debug!("IAudioProcessor::setProcessing(true) succeeded");

            tracing::info!("VST3 plugin '{}' initialized successfully", self.info.name);
        }

        self.is_active = true;
        Ok(())
    }

    #[allow(unsafe_code)] // Required for FFI calls
    #[allow(clippy::cast_possible_wrap)] // num_samples is always positive
    fn process(
        &mut self,
        audio: &mut AudioBuffer,
        _events: &EventBuffer,
    ) -> Result<(), PluginError> {
        // REAL-TIME SAFE: Only log first call to avoid flooding
        static FIRST_CALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        // Debug counter for logging first few process calls
        static DEBUG_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

        if FIRST_CALL.swap(false, std::sync::atomic::Ordering::Relaxed) {
            tracing::debug!("VST3 process() called for first time");
        }

        unsafe {
            // Step 1: Validate buffer counts match expected channels
            // This prevents accessing invalid pointers in the channel arrays
            let actual_input_channels = audio.inputs.len().min(self.input_channels);
            let actual_output_channels = audio.outputs.len().min(self.output_channels);

            if audio.inputs.len() < self.input_channels {
                tracing::warn!(
                    "Audio buffer has {} input channels but plugin expects {}",
                    audio.inputs.len(),
                    self.input_channels
                );
            }

            if audio.outputs.len() < self.output_channels {
                tracing::warn!(
                    "Audio buffer has {} output channels but plugin expects {}",
                    audio.outputs.len(),
                    self.output_channels
                );
            }

            // Step 2: Update input channel pointers (only for actual available channels)
            // VST3 expects an array of pointers to channel buffers
            for (i, input_slice) in audio.inputs.iter().enumerate().take(actual_input_channels) {
                if i < self.input_channel_ptrs.len() {
                    // Cast away const - VST3 may write to input buffers for in-place processing
                    self.input_channel_ptrs[i] = input_slice.as_ptr().cast_mut();
                }
            }

            // Step 3: Update output channel pointers (only for actual available channels)
            for (i, output_slice) in audio
                .outputs
                .iter_mut()
                .enumerate()
                .take(actual_output_channels)
            {
                if i < self.output_channel_ptrs.len() {
                    self.output_channel_ptrs[i] = output_slice.as_mut_ptr();
                }
            }

            // Step 4: Create AudioBusBuffers for input and output
            // IMPORTANT: Use actual channel counts to prevent VST3 from accessing invalid pointers
            let mut input_bus = crate::com::AudioBusBuffers {
                num_channels: actual_input_channels as i32,
                silence_flags: 0,
                channel_buffers_32: self.input_channel_ptrs.as_mut_ptr(),
                channel_buffers_64: std::ptr::null_mut(),
            };

            let mut output_bus = crate::com::AudioBusBuffers {
                num_channels: actual_output_channels as i32,
                silence_flags: 0,
                channel_buffers_32: self.output_channel_ptrs.as_mut_ptr(),
                channel_buffers_64: std::ptr::null_mut(),
            };

            // Step 4: Create ProcessData structure
            let mut process_data = crate::com::ProcessData {
                process_mode: 0,         // 0 = realtime
                symbolic_sample_size: 0, // 0 = 32-bit float
                num_samples: audio.frames as i32,
                num_inputs: 1,  // Single stereo bus for now
                num_outputs: 1, // Single stereo bus for now
                inputs: &raw mut input_bus,
                outputs: &raw mut output_bus,
                input_param_changes: std::ptr::null_mut(),
                output_param_changes: std::ptr::null_mut(),
                input_events: std::ptr::null_mut(),
                output_events: std::ptr::null_mut(),
                process_context: std::ptr::null_mut(),
            };

            // DEBUG: Check buffers before processing
            let count = DEBUG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if count < 3 {
                tracing::debug!(
                    "Before VST3 process: frames={}, inputs={}, outputs={}",
                    audio.frames,
                    audio.inputs.len(),
                    audio.outputs.len()
                );
                if let Some(out_ch0) = audio.outputs.first() {
                    tracing::debug!(
                        "  Output ch0 before: first 4 samples = [{:.4}, {:.4}, {:.4}, {:.4}]",
                        out_ch0.first().copied().unwrap_or(0.0),
                        out_ch0.get(1).copied().unwrap_or(0.0),
                        out_ch0.get(2).copied().unwrap_or(0.0),
                        out_ch0.get(3).copied().unwrap_or(0.0)
                    );
                }
            }

            // Step 5: Call VST3 processor->process()
            crate::com::processor_process(self.processor, &raw mut process_data)?;

            // DEBUG: Check buffers after processing
            if count < 3
                && let Some(out_ch0) = audio.outputs.first()
            {
                tracing::debug!(
                    "  Output ch0 after:  first 4 samples = [{:.4}, {:.4}, {:.4}, {:.4}]",
                    out_ch0.first().copied().unwrap_or(0.0),
                    out_ch0.get(1).copied().unwrap_or(0.0),
                    out_ch0.get(2).copied().unwrap_or(0.0),
                    out_ch0.get(3).copied().unwrap_or(0.0)
                );
            }
        }

        Ok(())
    }

    fn set_parameter(&mut self, id: u32, value: f32) -> Result<(), PluginError> {
        tracing::trace!("Setting parameter {} to {}", id, value);

        // TODO: Set VST3 parameter
        // edit_controller->setParamNormalized(id, value)

        Ok(())
    }

    fn get_parameter(&self, _id: u32) -> Result<f32, PluginError> {
        // TODO: Get VST3 parameter
        // edit_controller->getParamNormalized(id)

        Ok(0.0)
    }

    #[allow(unsafe_code)] // Required for FFI calls
    fn parameters(&self) -> Vec<ParameterInfo> {
        // If no edit controller, return empty list
        let Some(edit_controller) = self.edit_controller else {
            tracing::debug!("No IEditController interface - plugin has no parameters");
            return Vec::new();
        };

        unsafe {
            // Get parameter count
            let param_count = crate::com::edit_controller_get_parameter_count(edit_controller);
            tracing::debug!("Edit controller has {} parameters", param_count);

            if param_count <= 0 {
                return Vec::new();
            }

            let mut parameters = Vec::with_capacity(param_count as usize);

            for i in 0..param_count {
                match crate::com::edit_controller_get_parameter_info(edit_controller, i) {
                    Ok(vst3_param_info) => {
                        // Convert UTF-16 title to Rust string
                        let name = String::from_utf16_lossy(&vst3_param_info.title)
                            .trim_end_matches('\0')
                            .to_string();

                        // VST3 parameters are normalized to [0.0, 1.0]
                        // For now, we'll use that range directly
                        // TODO: Handle step_count for discrete parameters
                        let param_info = ParameterInfo {
                            id: vst3_param_info.id,
                            name,
                            min_value: 0.0,
                            max_value: 1.0,
                            default_value: vst3_param_info.default_normalized_value as f32,
                        };

                        parameters.push(param_info);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get parameter info for index {}: {}", i, e);
                    }
                }
            }

            parameters
        }
    }

    fn input_channels(&self) -> ChannelCount {
        self.input_channels
    }

    fn output_channels(&self) -> ChannelCount {
        self.output_channels
    }

    #[allow(unsafe_code)] // Required for FFI calls
    fn deactivate(&mut self) {
        // Only deactivate if currently active (avoid double-deactivation)
        if !self.is_active {
            return;
        }

        tracing::info!("Deactivating VST3 plugin '{}'", self.info.name);

        unsafe {
            // Step 1: Stop audio processing
            if let Err(e) = crate::com::processor_set_processing(self.processor, false) {
                tracing::error!("Failed to stop processing: {}", e);
            }

            // Step 2: Deactivate the component
            if let Err(e) = crate::com::component_set_active(self.component, false) {
                tracing::error!("Failed to deactivate component: {}", e);
            }

            // Step 3: Deactivate buses (only if they exist)
            let input_bus_count = crate::com::component_get_bus_count(self.component, 0, 0);
            let output_bus_count = crate::com::component_get_bus_count(self.component, 0, 1);

            if input_bus_count > 0
                && let Err(e) = crate::com::component_activate_bus(self.component, 0, 0, 0, false)
            {
                tracing::error!("Failed to deactivate input bus: {}", e);
            }

            if output_bus_count > 0
                && let Err(e) = crate::com::component_activate_bus(self.component, 0, 1, 0, false)
            {
                tracing::error!("Failed to deactivate output bus: {}", e);
            }

            // Note: COM interfaces (component, processor) are released when
            // the plugin is dropped. We don't manually call release() here
            // because Rust's ownership system handles cleanup via Drop.
        }

        self.is_active = false;
        tracing::debug!("VST3 plugin '{}' deactivated", self.info.name);
    }
}

impl Drop for Vst3Plugin {
    #[allow(unsafe_code)] // Required for COM cleanup
    fn drop(&mut self) {
        // Deactivate if still active
        self.deactivate();

        // Release COM interfaces using helper function
        unsafe {
            if !self.processor.is_null() {
                tracing::debug!("Releasing IAudioProcessor interface");
                crate::com::release_interface(self.processor);
            }

            if !self.component.is_null() {
                tracing::debug!("Releasing IComponent interface");
                crate::com::release_interface(self.component);
            }
        }

        tracing::debug!("VST3 plugin '{}' dropped", self.info.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vst3_plugin_basic() {
        // Test that PluginInfo works correctly
        let info = PluginInfo {
            name: "Test Plugin".to_string(),
            vendor: "Test Vendor".to_string(),
            version: "1.0.0".to_string(),
            unique_id: "test123".to_string(),
        };

        assert_eq!(info.name, "Test Plugin");
        assert_eq!(info.vendor, "Test Vendor");
    }
}
