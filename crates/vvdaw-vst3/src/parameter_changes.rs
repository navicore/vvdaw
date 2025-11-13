//! VST3 parameter change queue implementations
//!
//! Implements `IParamValueQueue` and `IParameterChanges` interfaces for transmitting
//! parameter changes from the host to the audio processor.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

const K_RESULT_OK: i32 = 0;
const K_RESULT_FALSE: i32 = 1;

/// Parameter value change point (sample-accurate)
#[derive(Debug, Clone)]
struct ValuePoint {
    sample_offset: i32,
    value: f64,
}

/// Implementation of `IParamValueQueue`
///
/// Stores value changes for a single parameter with sample-accurate timing.
#[repr(C)]
pub struct ParamValueQueue {
    /// COM vtable pointer
    vtable: *const IParamValueQueueVTable,

    /// Reference count for COM lifetime management
    ref_count: AtomicU32,

    /// Parameter ID this queue is for
    param_id: u32,

    /// Value change points (sample offset + value)
    points: Vec<ValuePoint>,
}

/// COM vtable for `IParamValueQueue`
#[repr(C)]
struct IParamValueQueueVTable {
    // FUnknown methods
    query_interface:
        unsafe extern "C" fn(this: *mut c_void, iid: *const [u8; 16], obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(this: *mut c_void) -> u32,
    release: unsafe extern "C" fn(this: *mut c_void) -> u32,

    // IParamValueQueue methods
    get_parameter_id: unsafe extern "C" fn(this: *mut c_void) -> u32,
    get_point_count: unsafe extern "C" fn(this: *mut c_void) -> i32,
    get_point: unsafe extern "C" fn(
        this: *mut c_void,
        index: i32,
        sample_offset: *mut i32,
        value: *mut f64,
    ) -> i32,
    add_point: unsafe extern "C" fn(
        this: *mut c_void,
        sample_offset: i32,
        value: f64,
        index: *mut i32,
    ) -> i32,
}

static PARAM_VALUE_QUEUE_VTABLE: IParamValueQueueVTable = IParamValueQueueVTable {
    query_interface,
    add_ref,
    release,
    get_parameter_id,
    get_point_count,
    get_point,
    add_point,
};

impl ParamValueQueue {
    /// Create a new parameter value queue
    pub fn new(param_id: u32) -> Self {
        Self {
            vtable: &raw const PARAM_VALUE_QUEUE_VTABLE,
            ref_count: AtomicU32::new(1),
            param_id,
            points: Vec::new(),
        }
    }

    /// Add a value change at a specific sample offset
    pub fn add_value(&mut self, sample_offset: i32, value: f64) {
        self.points.push(ValuePoint {
            sample_offset,
            value,
        });
    }
}

// FUnknown implementation

#[allow(unsafe_code)]
unsafe extern "C" fn query_interface(
    _this: *mut c_void,
    _iid: *const [u8; 16],
    _obj: *mut *mut c_void,
) -> i32 {
    // For now, don't support querying other interfaces
    K_RESULT_FALSE
}

#[allow(unsafe_code)]
unsafe extern "C" fn add_ref(this: *mut c_void) -> u32 {
    unsafe {
        let queue = &*(this.cast::<ParamValueQueue>());
        let old_count = queue.ref_count.fetch_add(1, Ordering::Relaxed);
        old_count + 1
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn release(this: *mut c_void) -> u32 {
    unsafe {
        let queue = &*(this.cast::<ParamValueQueue>());
        let old_count = queue.ref_count.fetch_sub(1, Ordering::Release);

        if old_count == 1 {
            // Last reference released - deallocate
            drop(Box::from_raw(this.cast::<ParamValueQueue>()));
            0
        } else {
            old_count - 1
        }
    }
}

// IParamValueQueue implementation

#[allow(unsafe_code)]
unsafe extern "C" fn get_parameter_id(this: *mut c_void) -> u32 {
    unsafe {
        let queue = &*(this.cast::<ParamValueQueue>());
        queue.param_id
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn get_point_count(this: *mut c_void) -> i32 {
    unsafe {
        let queue = &*(this.cast::<ParamValueQueue>());
        queue.points.len() as i32
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn get_point(
    this: *mut c_void,
    index: i32,
    sample_offset: *mut i32,
    value: *mut f64,
) -> i32 {
    if sample_offset.is_null() || value.is_null() {
        return K_RESULT_FALSE;
    }

    unsafe {
        let queue = &*(this.cast::<ParamValueQueue>());

        if index < 0 || index >= queue.points.len() as i32 {
            return K_RESULT_FALSE;
        }

        let point = &queue.points[index as usize];
        *sample_offset = point.sample_offset;
        *value = point.value;

        K_RESULT_OK
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn add_point(
    this: *mut c_void,
    sample_offset: i32,
    value: f64,
    index: *mut i32,
) -> i32 {
    unsafe {
        let queue = &mut *(this.cast::<ParamValueQueue>());

        let new_index = queue.points.len();
        queue.points.push(ValuePoint {
            sample_offset,
            value,
        });

        if !index.is_null() {
            *index = new_index as i32;
        }

        K_RESULT_OK
    }
}

/// Implementation of `IParameterChanges`
///
/// Manages a collection of parameter value queues, one per changed parameter.
#[repr(C)]
pub struct ParameterChanges {
    /// COM vtable pointer
    vtable: *const IParameterChangesVTable,

    /// Reference count for COM lifetime management
    ref_count: AtomicU32,

    /// Collection of parameter queues (owned)
    queues: Vec<*mut ParamValueQueue>,
}

/// COM vtable for `IParameterChanges`
#[repr(C)]
struct IParameterChangesVTable {
    // FUnknown methods
    query_interface_changes:
        unsafe extern "C" fn(this: *mut c_void, iid: *const [u8; 16], obj: *mut *mut c_void) -> i32,
    add_ref_changes: unsafe extern "C" fn(this: *mut c_void) -> u32,
    release_changes: unsafe extern "C" fn(this: *mut c_void) -> u32,

    // IParameterChanges methods
    get_parameter_count: unsafe extern "C" fn(this: *mut c_void) -> i32,
    get_parameter_data: unsafe extern "C" fn(this: *mut c_void, index: i32) -> *mut c_void,
    add_parameter_data:
        unsafe extern "C" fn(this: *mut c_void, id: *const u32, index: *mut i32) -> *mut c_void,
}

static PARAMETER_CHANGES_VTABLE: IParameterChangesVTable = IParameterChangesVTable {
    query_interface_changes,
    add_ref_changes,
    release_changes,
    get_parameter_count: get_parameter_count_changes,
    get_parameter_data: get_parameter_data_changes,
    add_parameter_data: add_parameter_data_changes,
};

impl ParameterChanges {
    /// Create a new empty parameter changes collection
    pub fn new() -> Self {
        Self {
            vtable: &raw const PARAMETER_CHANGES_VTABLE,
            ref_count: AtomicU32::new(1),
            queues: Vec::new(),
        }
    }

    /// Add a parameter change
    ///
    /// If a queue already exists for this parameter, adds to that queue.
    /// Otherwise creates a new queue.
    #[allow(unsafe_code)]
    pub fn add_change(&mut self, param_id: u32, sample_offset: i32, value: f64) {
        // Find existing queue for this parameter
        for &queue_ptr in &self.queues {
            unsafe {
                let queue = &mut *queue_ptr;
                if queue.param_id == param_id {
                    queue.add_value(sample_offset, value);
                    return;
                }
            }
        }

        // No existing queue - create new one
        let mut queue = Box::new(ParamValueQueue::new(param_id));
        queue.add_value(sample_offset, value);
        self.queues.push(Box::into_raw(queue));
    }

    /// Clear all queues (for reuse)
    #[allow(unsafe_code)]
    pub fn clear(&mut self) {
        // Release all queue references
        for &queue_ptr in &self.queues {
            unsafe {
                // Each queue has refcount 1 (we own it), so this will deallocate
                drop(Box::from_raw(queue_ptr));
            }
        }
        self.queues.clear();
    }
}

impl Drop for ParameterChanges {
    fn drop(&mut self) {
        self.clear();
    }
}

// SAFETY: ParameterChanges is only used from the audio thread in Vst3Plugin.
// The raw pointers to ParamValueQueue are owned and managed exclusively by this struct.
// The vtable is a static reference and safe to share.
// The ref_count is AtomicU32 which is already Send + Sync.
#[allow(unsafe_code)]
unsafe impl Send for ParameterChanges {}

// FUnknown implementation for ParameterChanges

#[allow(unsafe_code)]
unsafe extern "C" fn query_interface_changes(
    _this: *mut c_void,
    _iid: *const [u8; 16],
    _obj: *mut *mut c_void,
) -> i32 {
    K_RESULT_FALSE
}

#[allow(unsafe_code)]
unsafe extern "C" fn add_ref_changes(this: *mut c_void) -> u32 {
    unsafe {
        let changes = &*(this.cast::<ParameterChanges>());
        let old_count = changes.ref_count.fetch_add(1, Ordering::Relaxed);
        old_count + 1
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn release_changes(this: *mut c_void) -> u32 {
    unsafe {
        let changes = &*(this.cast::<ParameterChanges>());
        let old_count = changes.ref_count.fetch_sub(1, Ordering::Release);

        if old_count == 1 {
            // Last reference released - deallocate
            drop(Box::from_raw(this.cast::<ParameterChanges>()));
            0
        } else {
            old_count - 1
        }
    }
}

// IParameterChanges implementation

#[allow(unsafe_code)]
unsafe extern "C" fn get_parameter_count_changes(this: *mut c_void) -> i32 {
    unsafe {
        let changes = &*(this.cast::<ParameterChanges>());
        changes.queues.len() as i32
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn get_parameter_data_changes(this: *mut c_void, index: i32) -> *mut c_void {
    unsafe {
        let changes = &*(this.cast::<ParameterChanges>());

        if index < 0 || index >= changes.queues.len() as i32 {
            return std::ptr::null_mut();
        }

        changes.queues[index as usize].cast::<c_void>()
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn add_parameter_data_changes(
    this: *mut c_void,
    id: *const u32,
    index: *mut i32,
) -> *mut c_void {
    if id.is_null() {
        return std::ptr::null_mut();
    }

    unsafe {
        let changes = &mut *(this.cast::<ParameterChanges>());
        let param_id = *id;

        // Check if queue already exists
        for (i, &queue_ptr) in changes.queues.iter().enumerate() {
            let queue = &*queue_ptr;
            if queue.param_id == param_id {
                if !index.is_null() {
                    *index = i as i32;
                }
                return queue_ptr.cast::<c_void>();
            }
        }

        // Create new queue
        let queue = Box::new(ParamValueQueue::new(param_id));
        let queue_ptr = Box::into_raw(queue);
        let new_index = changes.queues.len();
        changes.queues.push(queue_ptr);

        if !index.is_null() {
            *index = new_index as i32;
        }

        queue_ptr.cast::<c_void>()
    }
}
