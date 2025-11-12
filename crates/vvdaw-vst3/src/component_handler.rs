//! `IComponentHandler` implementation for VST3 host
//!
//! This module provides a COM object that implements the `IComponentHandler` interface,
//! which is the callback interface used by edit controllers to communicate with the host.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// `IComponentHandler` interface IID
/// FUID: 93A0BEA3-0BD0-45DB-8E89-0B0CC1E46AC6
const ICOMPONENT_HANDLER_IID: [u8; 16] = [
    0x93, 0xA0, 0xBE, 0xA3, 0x0B, 0xD0, 0x45, 0xDB, 0x8E, 0x89, 0x0B, 0x0C, 0xC1, 0xE4, 0x6A, 0xC6,
];

/// `FUnknown` IID (base interface for all COM objects)
/// FUID: 00000000-00000000-C0000000-00000046
const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// Basic `IComponentHandler` implementation
///
/// This is a minimal implementation that allows plugins to function.
/// Currently it just acknowledges callbacks without actually processing them.
#[repr(C)]
pub struct ComponentHandler {
    /// COM vtable pointer (must be first field)
    vtable: *const IComponentHandlerVTable,

    /// Reference count for COM lifetime management
    ref_count: AtomicU32,
}

/// `IComponentHandler` vtable structure
///
/// Layout matches VST3 COM vtable for `IComponentHandler` interface.
#[repr(C)]
struct IComponentHandlerVTable {
    // FUnknown methods
    query_interface:
        unsafe extern "C" fn(this: *mut c_void, iid: *const [u8; 16], obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(this: *mut c_void) -> u32,
    release: unsafe extern "C" fn(this: *mut c_void) -> u32,

    // IComponentHandler methods
    begin_edit: unsafe extern "C" fn(this: *mut c_void, id: u32) -> i32,
    perform_edit: unsafe extern "C" fn(this: *mut c_void, id: u32, value_normalized: f64) -> i32,
    end_edit: unsafe extern "C" fn(this: *mut c_void, id: u32) -> i32,
    restart_component: unsafe extern "C" fn(this: *mut c_void, flags: i32) -> i32,
}

/// VST3 result codes
const K_RESULT_OK: i32 = 0;
const K_NO_INTERFACE: i32 = -1;

/// Static vtable instance
static VTABLE: IComponentHandlerVTable = IComponentHandlerVTable {
    query_interface,
    add_ref,
    release,
    begin_edit,
    perform_edit,
    end_edit,
    restart_component,
};

impl ComponentHandler {
    /// Create a new component handler
    ///
    /// Caller must use `Box::leak()` to transfer ownership to COM reference counting
    #[allow(unsafe_code)]
    pub fn new() -> Self {
        Self {
            vtable: &raw const VTABLE,
            ref_count: AtomicU32::new(1),
        }
    }
}

// COM vtable implementations

#[allow(unsafe_code)]
unsafe extern "C" fn query_interface(
    this: *mut c_void,
    iid: *const [u8; 16],
    obj: *mut *mut c_void,
) -> i32 {
    if this.is_null() || iid.is_null() || obj.is_null() {
        tracing::warn!("handler::queryInterface - null pointer");
        return K_NO_INTERFACE;
    }

    let iid_bytes = unsafe { *iid };
    tracing::debug!("handler::queryInterface called for IID: {:?}", iid_bytes);

    // Check if requesting IComponentHandler or FUnknown
    if iid_bytes == ICOMPONENT_HANDLER_IID || iid_bytes == FUNKNOWN_IID {
        tracing::debug!("  -> Returning handler (IComponentHandler or FUnknown)");
        unsafe {
            *obj = this;
            let handler = &*(this.cast::<ComponentHandler>());
            handler.ref_count.fetch_add(1, Ordering::Relaxed);
        }
        K_RESULT_OK
    } else {
        tracing::debug!("  -> Interface not supported");
        unsafe {
            *obj = std::ptr::null_mut();
        }
        K_NO_INTERFACE
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn add_ref(this: *mut c_void) -> u32 {
    if this.is_null() {
        tracing::warn!("handler::addRef - null pointer");
        return 0;
    }

    let handler = unsafe { &*(this.cast::<ComponentHandler>()) };
    let old_count = handler.ref_count.fetch_add(1, Ordering::Relaxed);
    let new_count = old_count + 1;
    tracing::debug!(
        "handler::addRef at {:?} - ref_count: {} -> {}",
        this,
        old_count,
        new_count
    );
    new_count
}

#[allow(unsafe_code)]
unsafe extern "C" fn release(this: *mut c_void) -> u32 {
    if this.is_null() {
        tracing::warn!("handler::release - null pointer");
        return 0;
    }

    let handler = unsafe { &*(this.cast::<ComponentHandler>()) };
    let old_count = handler.ref_count.fetch_sub(1, Ordering::Relaxed);
    let new_count = old_count.saturating_sub(1);

    tracing::debug!(
        "handler::release at {:?} - ref_count: {} -> {}",
        this,
        old_count,
        new_count
    );

    if old_count == 1 {
        // Last reference - destroy the object
        tracing::info!(
            "handler::release at {:?} - freeing memory (last reference)",
            this
        );
        unsafe {
            drop(Box::from_raw(this.cast::<ComponentHandler>()));
        }
        0
    } else if old_count == 0 {
        tracing::error!(
            "handler::release at {:?} - ref_count was already 0! (double-free attempt)",
            this
        );
        0
    } else {
        new_count
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn begin_edit(this: *mut c_void, id: u32) -> i32 {
    if this.is_null() {
        tracing::warn!("handler::beginEdit - null pointer");
        return K_RESULT_OK; // Return OK to not break the plugin
    }

    tracing::info!("handler::beginEdit - param_id: {}", id);
    K_RESULT_OK
}

#[allow(unsafe_code)]
unsafe extern "C" fn perform_edit(this: *mut c_void, id: u32, value_normalized: f64) -> i32 {
    if this.is_null() {
        tracing::warn!("handler::performEdit - null pointer");
        return K_RESULT_OK;
    }

    tracing::info!(
        "handler::performEdit - param_id: {}, value: {:.4}",
        id,
        value_normalized
    );
    // TODO: Send parameter changes to audio processor
    K_RESULT_OK
}

#[allow(unsafe_code)]
unsafe extern "C" fn end_edit(this: *mut c_void, id: u32) -> i32 {
    if this.is_null() {
        tracing::warn!("handler::endEdit - null pointer");
        return K_RESULT_OK;
    }

    tracing::info!("handler::endEdit - param_id: {}", id);
    K_RESULT_OK
}

#[allow(unsafe_code)]
unsafe extern "C" fn restart_component(this: *mut c_void, flags: i32) -> i32 {
    if this.is_null() {
        tracing::warn!("handler::restartComponent - null pointer");
        return K_RESULT_OK;
    }

    tracing::info!("handler::restartComponent - flags: 0x{:x}", flags);
    // TODO: Handle restart requests (latency changes, IO changes, etc.)
    K_RESULT_OK
}
