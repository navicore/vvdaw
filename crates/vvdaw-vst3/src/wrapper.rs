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
    #[allow(dead_code)] // Will be used for COM calls
    component: *mut std::ffi::c_void,
    #[allow(dead_code)] // Will be used for COM calls
    processor: *mut std::ffi::c_void,
    // TODO: Add edit_controller when needed

    // Audio configuration
    sample_rate: SampleRate,
    block_size: Frames,
    input_channels: ChannelCount,
    output_channels: ChannelCount,
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
        Self {
            info,
            library,
            factory,
            component,
            processor,
            sample_rate: 48000,
            block_size: 512,
            input_channels: 2,
            output_channels: 2,
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

        // TODO: Call VST3 initialization methods:
        // 1. component->initialize(context)
        // 2. processor->setupProcessing(process_setup)
        // 3. component->setActive(true)
        // 4. processor->setProcessing(true)

        Ok(())
    }

    fn process(
        &mut self,
        _audio: &mut AudioBuffer,
        _events: &EventBuffer,
    ) -> Result<(), PluginError> {
        // TODO: Process audio through VST3 plugin
        // 1. Convert AudioBuffer to VST3's ProcessData
        // 2. Convert EventBuffer to VST3's input events
        // 3. Call processor->process(data)
        // 4. Convert output back

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

    fn deactivate(&mut self) {
        tracing::info!("Deactivating VST3 plugin '{}'", self.info.name);

        // TODO: Deactivate VST3 plugin
        // 1. processor->setProcessing(false)
        // 2. component->setActive(false)
        // 3. Release COM interfaces
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
