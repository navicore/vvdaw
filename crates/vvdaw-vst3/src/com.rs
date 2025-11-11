//! VST3 COM interface helpers.
//!
//! This module provides safe wrappers around VST3's COM-style interfaces.
//! VST3 uses manual reference counting and vtable-based polymorphism,
//! similar to Microsoft COM.
//!
//! ## Vtable Layout
//!
//! VST3 interfaces inherit from `FUnknown` which provides:
//! - vtable[0] = queryInterface
//! - vtable[1] = addRef
//! - vtable[2] = release
//!
//! `IPluginFactory` adds:
//! - vtable[3] = getFactoryInfo
//! - vtable[4] = countClasses
//! - vtable[5] = getClassInfo
//! - vtable[6] = createInstance

use crate::ffi;
use std::ffi::c_void;
use vvdaw_plugin::PluginError;

/// COM result type (tresult in VST3)
pub type TResult = i32;

/// COM success result
pub const K_RESULT_OK: TResult = 0;

/// `IComponent` interface ID (IID)
/// FUID: E831FF31-F2D5-4301-928E-BBEE25697802
/// From VST3 SDK: `DECLARE_CLASS_IID` (`IComponent`, 0xE831FF31, 0xF2D54301, 0x928EBBEE, 0x25697802)
pub const ICOMPONENT_IID: [u8; 16] = [
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
];

/// `IAudioProcessor` interface ID (IID)
/// FUID: 42043F99-B7DA-453C-A569-E79D9AAEC33D
/// From VST3 SDK: `DECLARE_CLASS_IID` (`IAudioProcessor`, 0x42043F99, 0xB7DA453C, 0xA569E79D, 0x9AAEC33D)
pub const IAUDIO_PROCESSOR_IID: [u8; 16] = [
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3D,
];

/// `IEditController` interface ID (IID)
/// FUID: DCD7BBE3-7742-448D-A874-AACC979C759E
/// From VST3 SDK: `DECLARE_CLASS_IID` (`IEditController`, 0xDCD7BBE3, 0x7742448D, 0xA874AACC, 0x979C759E)
pub const IEDIT_CONTROLLER_IID: [u8; 16] = [
    0xDC, 0xD7, 0xBB, 0xE3, 0x77, 0x42, 0x44, 0x8D, 0xA8, 0x74, 0xAA, 0xCC, 0x97, 0x9C, 0x75, 0x9E,
];

/// Function pointer type for `IPluginFactory::countClasses`
///
/// Returns the number of classes exported by this factory.
type CountClassesFn = unsafe extern "C" fn(this: *mut c_void) -> i32;

/// Function pointer type for `IPluginFactory::getClassInfo`
///
/// Fills a `PClassInfo` structure with information about the class at the specified index.
type GetClassInfoFn = unsafe extern "C" fn(
    this: *mut c_void,
    index: i32,
    info: *mut ffi::Steinberg_PClassInfo,
) -> TResult;

/// Function pointer type for `IPluginFactory::createInstance`
///
/// Creates an instance of a plugin class.
type CreateInstanceFn = unsafe extern "C" fn(
    this: *mut c_void,
    cid: *const [i8; 16],  // TUID class ID
    _iid: *const [i8; 16], // TUID interface ID
    obj: *mut *mut c_void, // Output parameter
) -> TResult;

/// Function pointer type for `FUnknown::release`
///
/// Decrements the reference count and destroys the object if it reaches zero.
type ReleaseFn = unsafe extern "C" fn(this: *mut c_void) -> u32;

/// Function pointer type for `FUnknown::queryInterface`
///
/// Queries for a different interface on the same object.
#[allow(dead_code)] // Will be used for vtable calls
type QueryInterfaceFn =
    unsafe extern "C" fn(this: *mut c_void, iid: *const [i8; 16], obj: *mut *mut c_void) -> TResult;

/// Function pointer type for `IComponent::initialize`
///
/// Initializes the component with a host context.
type ComponentInitializeFn =
    unsafe extern "C" fn(this: *mut c_void, context: *mut c_void) -> TResult;

/// Function pointer type for `IComponent::getBusCount`
///
/// Gets the number of buses for a given media type and direction.
type ComponentGetBusCountFn = unsafe extern "C" fn(
    this: *mut c_void,
    media_type: i32,    // 0=audio, 1=event
    bus_direction: i32, // 0=input, 1=output
) -> i32;

/// Function pointer type for `IComponent::activateBus`
///
/// Activates or deactivates a specific bus.
type ComponentActivateBusFn = unsafe extern "C" fn(
    this: *mut c_void,
    media_type: i32,    // 0=audio, 1=event
    bus_direction: i32, // 0=input, 1=output
    bus_index: i32,
    state: u8, // 0=false, 1=true
) -> TResult;

/// Function pointer type for `IComponent::setActive`
///
/// Activates or deactivates the component.
type ComponentSetActiveFn = unsafe extern "C" fn(this: *mut c_void, state: u8) -> TResult;

/// Function pointer type for `IAudioProcessor::setupProcessing`
///
/// Sets up the audio processing parameters.
type ProcessorSetupProcessingFn =
    unsafe extern "C" fn(this: *mut c_void, setup: *const ProcessSetup) -> TResult;

/// Function pointer type for `IAudioProcessor::setProcessing`
///
/// Activates or deactivates audio processing.
type ProcessorSetProcessingFn = unsafe extern "C" fn(this: *mut c_void, state: u8) -> TResult;

/// VST3 `ProcessSetup` structure
///
/// Describes the audio processing configuration.
#[repr(C)]
pub struct ProcessSetup {
    pub process_mode: i32,         // 0=realtime, 1=prefetch, 2=offline
    pub symbolic_sample_size: i32, // 0=32bit, 1=64bit
    pub max_samples_per_block: i32,
    pub sample_rate: f64,
}

/// VST3 `AudioBusBuffers` structure
///
/// Contains pointers to audio channel buffers for a single bus.
#[repr(C)]
pub struct AudioBusBuffers {
    pub num_channels: i32,                 // Number of channels in this bus
    pub silence_flags: u64,                // Bitfield indicating silent channels
    pub channel_buffers_32: *mut *mut f32, // Array of pointers to 32-bit channel buffers
    pub channel_buffers_64: *mut *mut f64, // Array of pointers to 64-bit channel buffers
}

/// VST3 `ProcessData` structure
///
/// Contains all data for a single process call.
#[repr(C)]
pub struct ProcessData {
    pub process_mode: i32,                 // 0=realtime, 1=prefetch, 2=offline
    pub symbolic_sample_size: i32,         // 0=32bit, 1=64bit
    pub num_samples: i32,                  // Number of samples in this block
    pub num_inputs: i32,                   // Number of input buses
    pub num_outputs: i32,                  // Number of output buses
    pub inputs: *mut AudioBusBuffers,      // Array of input bus buffers
    pub outputs: *mut AudioBusBuffers,     // Array of output bus buffers
    pub input_param_changes: *mut c_void,  // IParameterChanges (null for now)
    pub output_param_changes: *mut c_void, // IParameterChanges (null for now)
    pub input_events: *mut c_void,         // IEventList (null for now)
    pub output_events: *mut c_void,        // IEventList (null for now)
    pub process_context: *mut c_void,      // ProcessContext (null for now)
}

/// Function pointer type for `IAudioProcessor::process`
///
/// Processes audio data.
type ProcessorProcessFn =
    unsafe extern "C" fn(this: *mut c_void, data: *mut ProcessData) -> TResult;

/// Type alias for the `GetPluginFactory` function signature
///
/// The VST3 entry point that returns the plugin factory.
/// ```c
/// extern "C" IPluginFactory* PLUGIN_API GetPluginFactory();
/// ```
pub type GetPluginFactoryFn = unsafe extern "C" fn() -> *mut ffi::Steinberg_IPluginFactory;

/// Wrapper around a VST3 `IPluginFactory` pointer
///
/// Provides safe access to the plugin factory interface with automatic
/// reference counting.
pub struct PluginFactory {
    #[allow(dead_code)] // Will be used for vtable calls
    ptr: *mut ffi::Steinberg_IPluginFactory,
}

// SAFETY: VST3 interfaces are designed to be thread-safe and can be called
// from multiple threads. The `IPluginFactory` interface specifically is documented
// as thread-safe in the VST3 SDK. The pointer itself is opaque and managed by
// the plugin, which handles thread safety internally.
#[allow(unsafe_code)]
unsafe impl Send for PluginFactory {}

impl PluginFactory {
    /// Create a new `PluginFactory` from a raw pointer
    ///
    /// # Safety
    ///
    /// The pointer must be valid and properly reference counted.
    /// This function does NOT increment the reference count - the caller
    /// is responsible for ensuring proper ownership.
    #[allow(unsafe_code)]
    pub unsafe fn from_raw(ptr: *mut ffi::Steinberg_IPluginFactory) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr })
        }
    }

    /// Get the number of classes exported by this factory
    #[allow(unsafe_code)]
    pub fn count_classes(&self) -> i32 {
        unsafe {
            // Get the vtable pointer from the interface
            let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

            // countClasses is at vtable[4] (after queryInterface, addRef, release, getFactoryInfo)
            let count_classes_ptr = *vtable_ptr.add(4);
            let count_classes_fn: CountClassesFn = std::mem::transmute(count_classes_ptr);

            // Call the function
            count_classes_fn(self.ptr.cast::<c_void>())
        }
    }

    /// Get information about a class by index
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds or the call fails.
    #[allow(unsafe_code)]
    pub fn get_class_info(&self, index: i32) -> Result<ClassInfo, PluginError> {
        unsafe {
            // Allocate space for the result
            let mut class_info: ffi::Steinberg_PClassInfo = std::mem::zeroed();

            // Get the vtable pointer
            let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

            // getClassInfo is at vtable[5]
            let get_class_info_ptr = *vtable_ptr.add(5);
            let get_class_info_fn: GetClassInfoFn = std::mem::transmute(get_class_info_ptr);

            // Call the function
            let result = get_class_info_fn(self.ptr.cast::<c_void>(), index, &raw mut class_info);

            if result != K_RESULT_OK {
                return Err(PluginError::FormatError(format!(
                    "getClassInfo failed with result: {result}"
                )));
            }

            // Convert the C struct to our Rust struct
            Ok(Self::convert_class_info(&class_info))
        }
    }

    /// Create an instance of a plugin class
    ///
    /// # Safety
    ///
    /// The `class_id` must be a valid TUID obtained from `get_class_info`.
    /// The iid must be a valid interface ID (e.g., `IComponent::iid`).
    ///
    /// # Errors
    ///
    /// Returns an error if the instance creation fails.
    #[allow(unsafe_code)]
    pub unsafe fn create_instance(
        &self,
        class_id: &[u8; 16],
        interface_id: &[u8; 16],
    ) -> Result<*mut c_void, PluginError> {
        unsafe {
            // Get the vtable pointer
            let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

            // createInstance is at vtable[6]
            let create_instance_ptr = *vtable_ptr.add(6);
            let create_instance_fn: CreateInstanceFn = std::mem::transmute(create_instance_ptr);

            // Convert u8 arrays to i8 arrays (VST3 uses i8 for TUIDs)
            let cid: [i8; 16] = std::array::from_fn(|i| class_id[i] as i8);
            let iid: [i8; 16] = std::array::from_fn(|i| interface_id[i] as i8);

            // Output parameter for the created instance
            let mut obj: *mut c_void = std::ptr::null_mut();

            // Call createInstance
            let result = create_instance_fn(
                self.ptr.cast::<c_void>(),
                &raw const cid,
                &raw const iid,
                &raw mut obj,
            );

            if result != K_RESULT_OK {
                return Err(PluginError::FormatError(format!(
                    "createInstance failed with result: {result}"
                )));
            }

            if obj.is_null() {
                return Err(PluginError::FormatError(
                    "createInstance returned null".to_string(),
                ));
            }

            Ok(obj)
        }
    }

    /// Convert a VST3 `PClassInfo` to our `ClassInfo` struct
    ///
    /// # Safety
    ///
    /// The `PClassInfo` must be a valid, initialized struct from VST3.
    #[allow(unsafe_code)]
    fn convert_class_info(info: &ffi::Steinberg_PClassInfo) -> ClassInfo {
        unsafe {
            // Extract the class ID (TUID is a 16-byte array of i8, convert to u8)
            let mut class_id = [0u8; 16];
            for (i, &byte) in info.cid.iter().enumerate() {
                class_id[i] = byte as u8;
            }

            // Convert category and name from C strings
            let category = std::ffi::CStr::from_ptr(info.category.as_ptr())
                .to_string_lossy()
                .into_owned();

            let name = std::ffi::CStr::from_ptr(info.name.as_ptr())
                .to_string_lossy()
                .into_owned();

            ClassInfo {
                class_id,
                cardinality: info.cardinality,
                category,
                name,
            }
        }
    }
}

impl Drop for PluginFactory {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                // Get the vtable pointer
                let vtable_ptr = *(self.ptr.cast::<*const *const c_void>());

                // release() is at vtable[2] (after queryInterface, addRef)
                let release_ptr = *vtable_ptr.add(2);
                let release_fn: ReleaseFn = std::mem::transmute(release_ptr);

                // Call release - this decrements ref count and may destroy the object
                let ref_count = release_fn(self.ptr.cast::<c_void>());
                tracing::debug!("Released PluginFactory, ref count now: {}", ref_count);
            }
        }
    }
}

/// Information about a plugin class
#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub class_id: [u8; 16], // FUID (128-bit UUID)
    #[allow(dead_code)] // Will be used for plugin validation
    pub cardinality: i32,
    #[allow(dead_code)] // Will be used for plugin filtering
    pub category: String,
    pub name: String,
}

/// Helper functions for calling VST3 COM methods
///
/// These functions provide safe(r) wrappers around raw vtable calls.
/// Call `FUnknown::queryInterface(iid, obj)`
///
/// Queries a COM object for a different interface.
///
/// # Safety
///
/// The `this` pointer must be valid and point to a valid COM object.
#[allow(unsafe_code)]
#[allow(dead_code)] // Will be used for vtable calls
pub unsafe fn query_interface(
    this: *mut c_void,
    interface_id: &[u8; 16],
) -> Result<*mut c_void, PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(this.cast::<*const *const c_void>());

        // queryInterface is at vtable[0]
        let query_interface_ptr = *vtable_ptr;
        let query_interface_fn: QueryInterfaceFn = std::mem::transmute(query_interface_ptr);

        // Convert u8 array to i8 array (VST3 uses i8 for TUIDs)
        let iid: [i8; 16] = std::array::from_fn(|i| interface_id[i] as i8);

        // Output parameter for the queried interface
        let mut obj: *mut c_void = std::ptr::null_mut();

        // Call queryInterface
        let result = query_interface_fn(this, &raw const iid, &raw mut obj);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "queryInterface failed with result: {result}"
            )));
        }

        if obj.is_null() {
            return Err(PluginError::FormatError(
                "queryInterface returned null".to_string(),
            ));
        }

        Ok(obj)
    }
}

/// Call `FUnknown::release()`
///
/// Decrements the reference count on a COM interface and potentially destroys the object.
///
/// # Safety
///
/// The `this` pointer must be valid and point to a valid COM object.
#[allow(unsafe_code)]
pub unsafe fn release_interface(this: *mut c_void) {
    unsafe {
        if !this.is_null() {
            // Get the vtable pointer
            let vtable_ptr = *(this.cast::<*const *const c_void>());

            // release() is at vtable[2] (after queryInterface, addRef)
            let release_ptr = *vtable_ptr.add(2);
            let release_fn: ReleaseFn = std::mem::transmute(release_ptr);

            // Call release - this decrements ref count and may destroy the object
            release_fn(this);
        }
    }
}

/// Function pointer type for `IComponent::getControllerClassId`
///
/// Gets the class ID of the associated edit controller.
type ComponentGetControllerClassIdFn =
    unsafe extern "C" fn(this: *mut c_void, class_id: *mut [u8; 16]) -> TResult;

/// Call `IComponent::getControllerClassId(classId)`
///
/// Returns the class ID (TUID) of the edit controller associated with this component.
/// Returns an empty/zero TUID if the component doesn't have a separate edit controller.
///
/// # Safety
///
/// The `component` pointer must be valid and point to a valid `IComponent` interface.
#[allow(unsafe_code)]
pub unsafe fn component_get_controller_class_id(
    component: *mut c_void,
) -> Result<[u8; 16], PluginError> {
    unsafe {
        let mut class_id = [0u8; 16];

        // Get the vtable pointer
        let vtable_ptr = *(component.cast::<*const *const c_void>());

        // IComponent vtable layout:
        // [0-2] FUnknown: queryInterface, addRef, release
        // [3] IPluginBase: initialize
        // [4] IPluginBase: terminate
        // [5] IComponent: getControllerClassId
        let get_controller_class_id_ptr = *vtable_ptr.add(5);
        let get_controller_class_id_fn: ComponentGetControllerClassIdFn =
            std::mem::transmute(get_controller_class_id_ptr);

        // Call getControllerClassId
        let result = get_controller_class_id_fn(component, &raw mut class_id);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IComponent::getControllerClassId failed with result: {result}"
            )));
        }

        Ok(class_id)
    }
}

/// Function pointer type for `IComponent::getState`
///
/// Gets the current component state.
type ComponentGetStateFn = unsafe extern "C" fn(this: *mut c_void, state: *mut c_void) -> TResult;

/// Call `IComponent::getState(state)`
///
/// Stores the complete component state into the provided stream.
///
/// # Safety
///
/// The `component` pointer must be valid and point to a valid `IComponent` interface.
/// The `state_stream` pointer must be a valid IBStream interface.
#[allow(unsafe_code)]
pub unsafe fn component_get_state(
    component: *mut c_void,
    state_stream: *mut c_void,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(component.cast::<*const *const c_void>());

        // IComponent vtable layout:
        // [0-2] FUnknown: queryInterface, addRef, release
        // [3-4] IPluginBase: initialize, terminate
        // [5-11] IComponent: getControllerClassId, setIoMode, getBusCount, getBusInfo,
        //                    getRoutingInfo, activateBus, setActive
        // [12] IComponent: setState
        // [13] IComponent: getState
        let get_state_ptr = *vtable_ptr.add(13);
        let get_state_fn: ComponentGetStateFn = std::mem::transmute(get_state_ptr);

        // Call getState
        let result = get_state_fn(component, state_stream);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IComponent::getState failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IComponent::initialize(context)`
///
/// # Safety
///
/// The component pointer must be valid and point to a valid `IComponent` interface.
#[allow(unsafe_code)]
pub unsafe fn component_initialize(
    component: *mut c_void,
    context: *mut c_void,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(component.cast::<*const *const c_void>());

        // initialize is at vtable[3] (after queryInterface, addRef, release)
        let initialize_ptr = *vtable_ptr.add(3);
        let initialize_fn: ComponentInitializeFn = std::mem::transmute(initialize_ptr);

        // Call initialize
        let result = initialize_fn(component, context);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IComponent::initialize failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IComponent::getBusCount(type, dir)`
///
/// # Safety
///
/// The component pointer must be valid and point to a valid `IComponent` interface.
#[allow(unsafe_code)]
pub unsafe fn component_get_bus_count(
    component: *mut c_void,
    media_type: i32,
    bus_direction: i32,
) -> i32 {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(component.cast::<*const *const c_void>());

        // getBusCount is at vtable[7]
        let get_bus_count_ptr = *vtable_ptr.add(7);
        let get_bus_count_fn: ComponentGetBusCountFn = std::mem::transmute(get_bus_count_ptr);

        // Call getBusCount
        get_bus_count_fn(component, media_type, bus_direction)
    }
}

/// Call `IComponent::activateBus(type, dir, index, state)`
///
/// # Safety
///
/// The component pointer must be valid and point to a valid `IComponent` interface.
#[allow(unsafe_code)]
pub unsafe fn component_activate_bus(
    component: *mut c_void,
    media_type: i32,
    bus_direction: i32,
    bus_index: i32,
    state: bool,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(component.cast::<*const *const c_void>());

        // IComponent vtable layout:
        // [0-2] FUnknown: queryInterface, addRef, release
        // [3-4] IPluginBase: initialize, terminate
        // [5-11] IComponent: getControllerClassId, setIoMode, getBusCount, getBusInfo,
        //                    getRoutingInfo, activateBus, setActive
        // So activateBus is at vtable[10]
        let activate_bus_ptr = *vtable_ptr.add(10);
        let activate_bus_fn: ComponentActivateBusFn = std::mem::transmute(activate_bus_ptr);

        // Call activateBus
        let result = activate_bus_fn(
            component,
            media_type,
            bus_direction,
            bus_index,
            u8::from(state),
        );

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IComponent::activateBus failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IComponent::setActive(state)`
///
/// # Safety
///
/// The component pointer must be valid and point to a valid `IComponent` interface.
#[allow(unsafe_code)]
pub unsafe fn component_set_active(component: *mut c_void, state: bool) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(component.cast::<*const *const c_void>());

        // IComponent vtable layout:
        // [0-2] FUnknown: queryInterface, addRef, release
        // [3-4] IPluginBase: initialize, terminate
        // [5-11] IComponent: getControllerClassId, setIoMode, getBusCount, getBusInfo,
        //                    getRoutingInfo, activateBus, setActive
        // So setActive is at vtable[11]
        let set_active_ptr = *vtable_ptr.add(11);
        let set_active_fn: ComponentSetActiveFn = std::mem::transmute(set_active_ptr);

        // Call setActive (TBool is u8 in VST3: 0=false, 1=true)
        let result = set_active_fn(component, u8::from(state));

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IComponent::setActive failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IAudioProcessor::setupProcessing(setup)`
///
/// # Safety
///
/// The processor pointer must be valid and point to a valid `IAudioProcessor` interface.
#[allow(unsafe_code)]
pub unsafe fn processor_setup_processing(
    processor: *mut c_void,
    setup: &ProcessSetup,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(processor.cast::<*const *const c_void>());

        // setupProcessing is at vtable[7]
        // (after queryInterface, addRef, release, setBusArrangements, getBusArrangement,
        //  canProcessSampleSize, getLatencySamples)
        let setup_processing_ptr = *vtable_ptr.add(7);
        let setup_processing_fn: ProcessorSetupProcessingFn =
            std::mem::transmute(setup_processing_ptr);

        // Call setupProcessing
        let result = setup_processing_fn(processor, std::ptr::from_ref::<ProcessSetup>(setup));

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IAudioProcessor::setupProcessing failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IAudioProcessor::setProcessing(state)`
///
/// # Safety
///
/// The processor pointer must be valid and point to a valid `IAudioProcessor` interface.
#[allow(unsafe_code)]
pub unsafe fn processor_set_processing(
    processor: *mut c_void,
    state: bool,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(processor.cast::<*const *const c_void>());

        // setProcessing is at vtable[8]
        let set_processing_ptr = *vtable_ptr.add(8);
        let set_processing_fn: ProcessorSetProcessingFn = std::mem::transmute(set_processing_ptr);

        // Call setProcessing (TBool is u8 in VST3: 0=false, 1=true)
        let result = set_processing_fn(processor, u8::from(state));

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IAudioProcessor::setProcessing failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IAudioProcessor::process(data)`
///
/// # Safety
///
/// The processor pointer must be valid and point to a valid `IAudioProcessor` interface.
/// The `ProcessData` structure must be properly initialized with valid buffer pointers.
#[allow(unsafe_code)]
pub unsafe fn processor_process(
    processor: *mut c_void,
    data: *mut ProcessData,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(processor.cast::<*const *const c_void>());

        // process is at vtable[9]
        let process_ptr = *vtable_ptr.add(9);
        let process_fn: ProcessorProcessFn = std::mem::transmute(process_ptr);

        // Call process
        let result = process_fn(processor, data);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IAudioProcessor::process failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Function pointer type for `IEditController::getParameterCount`
///
/// Returns the number of parameters exposed by the controller.
type EditControllerGetParameterCountFn = unsafe extern "C" fn(this: *mut c_void) -> i32;

/// VST3 `ParameterInfo` structure
///
/// Contains information about a single parameter.
#[repr(C)]
pub struct ParameterInfo {
    pub id: u32,                       // Unique parameter ID
    pub title: [u16; 128],             // Parameter title (UTF-16)
    pub short_title: [u16; 128],       // Short title (UTF-16)
    pub units: [u16; 128],             // Units string (UTF-16)
    pub step_count: i32,               // Number of discrete steps (0 = continuous)
    pub default_normalized_value: f64, // Default value [0.0, 1.0]
    pub unit_id: i32,                  // Associated unit ID
    pub flags: i32,                    // ParameterFlags
}

/// Function pointer type for `IEditController::getParameterInfo`
///
/// Fills a `ParameterInfo` structure with information about the parameter at the specified index.
type EditControllerGetParameterInfoFn =
    unsafe extern "C" fn(this: *mut c_void, param_index: i32, info: *mut ParameterInfo) -> TResult;

/// Function pointer type for `IEditController::getParamStringByValue`
///
/// Converts a normalized value to a string representation.
#[allow(dead_code)] // Will be used for parameter display
type EditControllerGetParamStringByValueFn = unsafe extern "C" fn(
    this: *mut c_void,
    id: u32,
    value_normalized: f64,
    string: *mut i16, // TChar* (UTF-16 buffer)
) -> TResult;

/// Function pointer type for `IEditController::getParamValueByString`
///
/// Converts a string to a normalized value.
#[allow(dead_code)] // Will be used for parameter parsing
type EditControllerGetParamValueByStringFn = unsafe extern "C" fn(
    this: *mut c_void,
    id: u32,
    string: *const i16,         // TChar* (UTF-16)
    value_normalized: *mut f64, // Output parameter
) -> TResult;

/// Function pointer type for `IEditController::setComponentState`
///
/// Sets the component state for the edit controller.
type EditControllerSetComponentStateFn =
    unsafe extern "C" fn(this: *mut c_void, state: *mut c_void) -> TResult;

/// Call `IEditController::setComponentState(state)`
///
/// Receives the current component state (called after component initialization).
/// This synchronizes the edit controller with the component's state.
///
/// # Safety
///
/// The `edit_controller` pointer must be valid and point to a valid `IEditController` interface.
/// The `state_stream` pointer must be valid IBStream interface or null.
#[allow(unsafe_code)]
pub unsafe fn edit_controller_set_component_state(
    edit_controller: *mut c_void,
    state_stream: *mut c_void,
) -> Result<(), PluginError> {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(edit_controller.cast::<*const *const c_void>());

        // IEditController vtable layout:
        // [0-2] FUnknown: queryInterface, addRef, release
        // [3-4] IPluginBase: initialize, terminate
        // [5] IEditController: setComponentState
        let set_component_state_ptr = *vtable_ptr.add(5);
        let set_component_state_fn: EditControllerSetComponentStateFn =
            std::mem::transmute(set_component_state_ptr);

        // Call setComponentState
        let result = set_component_state_fn(edit_controller, state_stream);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IEditController::setComponentState failed with result: {result}"
            )));
        }

        Ok(())
    }
}

/// Call `IEditController::getParameterCount()`
///
/// # Safety
///
/// The `edit_controller` pointer must be valid and point to a valid `IEditController` interface.
#[allow(unsafe_code)]
pub unsafe fn edit_controller_get_parameter_count(edit_controller: *mut c_void) -> i32 {
    unsafe {
        // Get the vtable pointer
        let vtable_ptr = *(edit_controller.cast::<*const *const c_void>());

        // IEditController vtable layout:
        // [0-2] FUnknown: queryInterface, addRef, release
        // [3-4] IPluginBase: initialize, terminate
        // [5] IEditController: setComponentState
        // [6] IEditController: setState
        // [7] IEditController: getState
        // [8] IEditController: getParameterCount
        let get_parameter_count_ptr = *vtable_ptr.add(8);
        let get_parameter_count_fn: EditControllerGetParameterCountFn =
            std::mem::transmute(get_parameter_count_ptr);

        // Call getParameterCount
        get_parameter_count_fn(edit_controller)
    }
}

/// Call `IEditController::getParameterInfo(paramIndex, info)`
///
/// # Safety
///
/// The `edit_controller` pointer must be valid and point to a valid `IEditController` interface.
/// The `info` pointer must point to valid memory for a `ParameterInfo` struct.
#[allow(unsafe_code)]
pub unsafe fn edit_controller_get_parameter_info(
    edit_controller: *mut c_void,
    param_index: i32,
) -> Result<ParameterInfo, PluginError> {
    unsafe {
        // Allocate space for the result
        let mut param_info: ParameterInfo = std::mem::zeroed();

        // Get the vtable pointer
        let vtable_ptr = *(edit_controller.cast::<*const *const c_void>());

        // getParameterInfo is at vtable[9]
        let get_parameter_info_ptr = *vtable_ptr.add(9);
        let get_parameter_info_fn: EditControllerGetParameterInfoFn =
            std::mem::transmute(get_parameter_info_ptr);

        // Call getParameterInfo
        let result = get_parameter_info_fn(edit_controller, param_index, &raw mut param_info);

        if result != K_RESULT_OK {
            return Err(PluginError::FormatError(format!(
                "IEditController::getParameterInfo failed with result: {result}"
            )));
        }

        Ok(param_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(unsafe_code)] // Tests need unsafe for FFI testing
    fn test_null_factory() {
        let factory = unsafe { PluginFactory::from_raw(std::ptr::null_mut()) };
        assert!(factory.is_none());
    }
}
