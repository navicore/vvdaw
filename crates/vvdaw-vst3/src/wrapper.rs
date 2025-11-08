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
    // TODO: Add edit_controller when needed

    // Audio configuration
    sample_rate: SampleRate,
    block_size: Frames,
    input_channels: ChannelCount,
    output_channels: ChannelCount,

    // Pre-allocated buffers for VST3 process calls
    // These avoid allocations in the audio hot path
    input_channel_ptrs: Vec<*mut f32>,
    output_channel_ptrs: Vec<*mut f32>,
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
            sample_rate: 48000,
            block_size: 512,
            input_channels,
            output_channels,
            input_channel_ptrs: Vec::with_capacity(input_channels),
            output_channel_ptrs: Vec::with_capacity(output_channels),
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
            // Step 1: Initialize the component
            // Pass null context for now (TODO: implement proper host context)
            crate::com::component_initialize(self.component, std::ptr::null_mut())?;
            tracing::debug!("IComponent::initialize succeeded");

            // Step 2: Set up audio processing parameters
            let process_setup = crate::com::ProcessSetup {
                process_mode: 0,         // 0 = realtime
                symbolic_sample_size: 0, // 0 = 32-bit float
                max_samples_per_block: max_block_size as i32,
                sample_rate: f64::from(sample_rate),
            };
            crate::com::processor_setup_processing(self.processor, &process_setup)?;
            tracing::debug!("IAudioProcessor::setupProcessing succeeded");

            // Step 3: Activate the component
            tracing::debug!("Calling IComponent::setActive(true)...");
            crate::com::component_set_active(self.component, true)?;
            tracing::debug!("IComponent::setActive(true) succeeded");

            // Step 4: Start audio processing
            tracing::debug!("Calling IAudioProcessor::setProcessing(true)...");
            crate::com::processor_set_processing(self.processor, true)?;
            tracing::debug!("IAudioProcessor::setProcessing(true) succeeded");

            tracing::info!("VST3 plugin '{}' initialized successfully", self.info.name);
        }

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
        if FIRST_CALL.swap(false, std::sync::atomic::Ordering::Relaxed) {
            tracing::debug!("VST3 process() called for first time");
        }

        unsafe {
            // Step 1: Update input channel pointers
            // VST3 expects an array of pointers to channel buffers
            for (i, input_slice) in audio.inputs.iter().enumerate() {
                if i < self.input_channel_ptrs.len() {
                    // Cast away const - VST3 may write to input buffers for in-place processing
                    self.input_channel_ptrs[i] = input_slice.as_ptr().cast_mut();
                }
            }

            // Step 2: Update output channel pointers
            for (i, output_slice) in audio.outputs.iter_mut().enumerate() {
                if i < self.output_channel_ptrs.len() {
                    self.output_channel_ptrs[i] = output_slice.as_mut_ptr();
                }
            }

            // Step 3: Create AudioBusBuffers for input and output
            let mut input_bus = crate::com::AudioBusBuffers {
                num_channels: self.input_channels as i32,
                silence_flags: 0,
                channel_buffers_32: self.input_channel_ptrs.as_mut_ptr(),
                channel_buffers_64: std::ptr::null_mut(),
            };

            let mut output_bus = crate::com::AudioBusBuffers {
                num_channels: self.output_channels as i32,
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

            // Step 5: Call VST3 processor->process()
            crate::com::processor_process(self.processor, &raw mut process_data)?;
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

    fn parameters(&self) -> Vec<ParameterInfo> {
        // TODO: Query VST3 parameters
        // 1. Get parameter count from edit_controller
        // 2. For each parameter, get ParameterInfo
        // 3. Convert to our ParameterInfo format

        Vec::new()
    }

    fn input_channels(&self) -> ChannelCount {
        self.input_channels
    }

    fn output_channels(&self) -> ChannelCount {
        self.output_channels
    }

    #[allow(unsafe_code)] // Required for FFI calls
    fn deactivate(&mut self) {
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

            // Note: COM interfaces (component, processor) are released when
            // the plugin is dropped. We don't manually call release() here
            // because Rust's ownership system handles cleanup via Drop.
        }

        tracing::debug!("VST3 plugin '{}' deactivated", self.info.name);
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        self.deactivate();

        // TODO: Release COM interfaces and unload library
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
