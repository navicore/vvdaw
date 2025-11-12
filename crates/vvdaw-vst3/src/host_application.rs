//! `IHostApplication` implementation for VST3 host
//!
//! This module provides a COM object that implements the `IHostApplication` interface,
//! which plugins use to identify the host and create host-provided objects.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// `IHostApplication` interface IID
/// FUID: 58E595CC-DB2D-4969-8B6A-AF8C36A664E5
const IHOST_APPLICATION_IID: [u8; 16] = [
    0x58, 0xE5, 0x95, 0xCC, 0xDB, 0x2D, 0x49, 0x69, 0x8B, 0x6A, 0xAF, 0x8C, 0x36, 0xA6, 0x64, 0xE5,
];

/// `FUnknown` IID (base interface for all COM objects)
/// FUID: 00000000-00000000-C0000000-00000046
const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// Basic `IHostApplication` implementation
///
/// This provides minimal host identification for VST3 plugins.
#[repr(C)]
pub struct HostApplication {
    /// COM vtable pointer (must be first field)
    vtable: *const IHostApplicationVTable,

    /// Reference count for COM lifetime management
    ref_count: AtomicU32,
}

/// `IHostApplication` vtable structure
///
/// Layout matches VST3 COM vtable for `IHostApplication` interface.
#[repr(C)]
struct IHostApplicationVTable {
    // FUnknown methods
    query_interface:
        unsafe extern "C" fn(this: *mut c_void, iid: *const [u8; 16], obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(this: *mut c_void) -> u32,
    release: unsafe extern "C" fn(this: *mut c_void) -> u32,

    // IHostApplication methods
    get_name: unsafe extern "C" fn(this: *mut c_void, name: *mut i16) -> i32,
    create_instance: unsafe extern "C" fn(
        this: *mut c_void,
        cid: *const [u8; 16],
        iid: *const [u8; 16],
        obj: *mut *mut c_void,
    ) -> i32,
}

/// VST3 result codes
const K_RESULT_OK: i32 = 0;
const K_NO_INTERFACE: i32 = -1;
const K_NOT_IMPLEMENTED: i32 = -4;

/// Static vtable instance
static VTABLE: IHostApplicationVTable = IHostApplicationVTable {
    query_interface,
    add_ref,
    release,
    get_name,
    create_instance,
};

impl HostApplication {
    /// Create a new host application context
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
        tracing::warn!("host_app::queryInterface - null pointer");
        return K_NO_INTERFACE;
    }

    let iid_bytes = unsafe { *iid };
    tracing::debug!("host_app::queryInterface called for IID: {:?}", iid_bytes);

    // Check if requesting IHostApplication or FUnknown
    if iid_bytes == IHOST_APPLICATION_IID || iid_bytes == FUNKNOWN_IID {
        tracing::debug!("  -> Returning host app (IHostApplication or FUnknown)");
        unsafe {
            *obj = this;
            let host_app = &*(this.cast::<HostApplication>());
            host_app.ref_count.fetch_add(1, Ordering::Relaxed);
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
        tracing::warn!("host_app::addRef - null pointer");
        return 0;
    }

    let host_app = unsafe { &*(this.cast::<HostApplication>()) };
    let old_count = host_app.ref_count.fetch_add(1, Ordering::Relaxed);
    let new_count = old_count + 1;
    tracing::debug!(
        "host_app::addRef at {:?} - ref_count: {} -> {}",
        this,
        old_count,
        new_count
    );
    new_count
}

#[allow(unsafe_code)]
unsafe extern "C" fn release(this: *mut c_void) -> u32 {
    if this.is_null() {
        tracing::warn!("host_app::release - null pointer");
        return 0;
    }

    let host_app = unsafe { &*(this.cast::<HostApplication>()) };
    let old_count = host_app.ref_count.fetch_sub(1, Ordering::Relaxed);
    let new_count = old_count.saturating_sub(1);

    tracing::debug!(
        "host_app::release at {:?} - ref_count: {} -> {}",
        this,
        old_count,
        new_count
    );

    if old_count == 1 {
        // Last reference - destroy the object
        tracing::info!(
            "host_app::release at {:?} - freeing memory (last reference)",
            this
        );
        unsafe {
            drop(Box::from_raw(this.cast::<HostApplication>()));
        }
        0
    } else if old_count == 0 {
        tracing::error!(
            "host_app::release at {:?} - ref_count was already 0! (double-free attempt)",
            this
        );
        0
    } else {
        new_count
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn get_name(this: *mut c_void, name: *mut i16) -> i32 {
    if this.is_null() || name.is_null() {
        tracing::warn!("host_app::getName - null pointer");
        return K_RESULT_OK; // Return OK to not break the plugin
    }

    // Host name: "vvdaw" (UTF-16)
    // String128 is 128 characters (256 bytes)
    let host_name = "vvdaw";
    let mut utf16_name: Vec<u16> = host_name.encode_utf16().collect();
    utf16_name.push(0); // Null terminator

    tracing::debug!("host_app::getName - returning '{}'", host_name);

    // Copy to output buffer
    unsafe {
        std::ptr::copy_nonoverlapping(utf16_name.as_ptr().cast::<i16>(), name, utf16_name.len());
    }

    K_RESULT_OK
}

#[allow(unsafe_code)]
unsafe extern "C" fn create_instance(
    this: *mut c_void,
    cid: *const [u8; 16],
    _iid: *const [u8; 16],
    obj: *mut *mut c_void,
) -> i32 {
    if this.is_null() || cid.is_null() || obj.is_null() {
        tracing::warn!("host_app::createInstance - null pointer");
        return K_NOT_IMPLEMENTED;
    }

    let cid_bytes = unsafe { *cid };
    tracing::debug!(
        "host_app::createInstance - CID: {:?} (not implemented)",
        cid_bytes
    );

    // For now, we don't support creating host objects
    // This would be needed for IMessage, etc.
    unsafe {
        *obj = std::ptr::null_mut();
    }

    K_NOT_IMPLEMENTED
}
