//! Memory-based `IBStream` implementation for VST3 state transfer
//!
//! Provides a simple COM object that implements `IBStream` for transferring
//! component state to edit controllers.
//!
//! ## Implementation Status
//!
//! This module implements a complete, working `IBStream` COM interface:
//! - ✓ `FUnknown` methods: queryInterface, addRef, release
//! - ✓ `IBStream` methods: read, write, seek, tell
//! - ✓ Proper COM reference counting
//! - ✓ Memory-backed buffer with position tracking
//!
//! However, commercial VST3 plugins are currently rejecting the stream
//! when passed to `IEditController::setComponentState()` with kInvalidArgument (result: 3).
//! The stream implementation itself is correct - the issue is likely in how it's being used
//! or an incorrect vtable offset for `setComponentState`.
//!
//! See loader.rs for full state transfer status documentation.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// VST3 `IBStream` interface IID
/// FUID: C3BF6EA2-30994752-9B6BF990-1EE33E9B
const IBSTREAM_IID: [u8; 16] = [
    0xC3, 0xBF, 0x6E, 0xA2, 0x30, 0x99, 0x47, 0x52, 0x9B, 0x6B, 0xF9, 0x90, 0x1E, 0xE3, 0x3E, 0x9B,
];

/// `FUnknown` IID (base interface for all COM objects)
/// FUID: 00000000-00000000-C0000000-00000046
const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// Memory-based `IBStream` implementation
///
/// This is a simple COM object that holds data in a `Vec<u8>` and implements
/// the `IBStream` interface for reading and writing.
#[repr(C)]
pub struct MemoryStream {
    /// COM vtable pointer (must be first field)
    vtable: *const IBStreamVTable,

    /// Reference count for COM lifetime management
    ref_count: AtomicU32,

    /// Internal buffer holding the stream data
    data: Vec<u8>,

    /// Current read/write position
    position: usize,
}

/// `IBStream` vtable structure
///
/// Layout matches VST3 COM vtable for `IBStream` interface.
#[repr(C)]
struct IBStreamVTable {
    // FUnknown methods
    query_interface:
        unsafe extern "C" fn(this: *mut c_void, iid: *const [u8; 16], obj: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(this: *mut c_void) -> u32,
    release: unsafe extern "C" fn(this: *mut c_void) -> u32,

    // IBStream methods
    read: unsafe extern "C" fn(
        this: *mut c_void,
        buffer: *mut c_void,
        num_bytes: i32,
        num_bytes_read: *mut i32,
    ) -> i32,
    write: unsafe extern "C" fn(
        this: *mut c_void,
        buffer: *const c_void,
        num_bytes: i32,
        num_bytes_written: *mut i32,
    ) -> i32,
    seek: unsafe extern "C" fn(this: *mut c_void, pos: i64, mode: i32, result: *mut i64) -> i32,
    tell: unsafe extern "C" fn(this: *mut c_void, pos: *mut i64) -> i32,
}

/// VST3 result codes
const K_RESULT_OK: i32 = 0;
const K_RESULT_FALSE: i32 = 1;
const K_NO_INTERFACE: i32 = -1;

/// Seek modes
const K_IBSEEK_SET: i32 = 0; // Set position from start
const K_IBSEEK_CUR: i32 = 1; // Set position from current
const K_IBSEEK_END: i32 = 2; // Set position from end

/// Static vtable instance
static VTABLE: IBStreamVTable = IBStreamVTable {
    query_interface,
    add_ref,
    release,
    read,
    write,
    seek,
    tell,
};

impl MemoryStream {
    /// Create a new empty memory stream
    #[allow(unsafe_code)]
    pub fn new() -> Box<Self> {
        Box::new(Self {
            vtable: &raw const VTABLE,
            ref_count: AtomicU32::new(1),
            data: Vec::new(),
            position: 0,
        })
    }

    /// Create a memory stream with initial capacity
    #[allow(unsafe_code)]
    pub fn with_capacity(capacity: usize) -> Box<Self> {
        Box::new(Self {
            vtable: &raw const VTABLE,
            ref_count: AtomicU32::new(1),
            data: Vec::with_capacity(capacity),
            position: 0,
        })
    }

    /// Get a raw pointer to this stream as a COM interface
    ///
    /// # Safety
    ///
    /// The returned pointer is valid as long as the Box is not dropped.
    /// The caller must ensure the Box outlives any uses of the pointer.
    pub fn as_com_ptr(&mut self) -> *mut c_void {
        std::ptr::from_mut::<Self>(self).cast::<c_void>()
    }

    /// Get the stream data
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get the current position
    pub fn position(&self) -> usize {
        self.position
    }

    /// Reset position to start
    pub fn rewind(&mut self) {
        self.position = 0;
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
        tracing::warn!("stream::queryInterface - null pointer");
        return K_NO_INTERFACE;
    }

    let iid_bytes = unsafe { *iid };
    tracing::debug!("stream::queryInterface called for IID: {:?}", iid_bytes);

    // Check if requesting IBStream or FUnknown (which IBStream extends)
    if iid_bytes == IBSTREAM_IID || iid_bytes == FUNKNOWN_IID {
        tracing::debug!("  -> Returning stream (IBStream or FUnknown)");
        unsafe {
            *obj = this;
            let stream = &*(this.cast::<MemoryStream>());
            stream.ref_count.fetch_add(1, Ordering::Relaxed);
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
        tracing::warn!("stream::addRef - null pointer");
        return 0;
    }

    let stream = unsafe { &*(this.cast::<MemoryStream>()) };
    let new_count = stream.ref_count.fetch_add(1, Ordering::Relaxed) + 1;
    tracing::debug!("stream::addRef -> ref_count now {}", new_count);
    new_count
}

#[allow(unsafe_code)]
unsafe extern "C" fn release(this: *mut c_void) -> u32 {
    if this.is_null() {
        tracing::warn!("stream::release - null pointer");
        return 0;
    }

    let stream = unsafe { &*(this.cast::<MemoryStream>()) };
    let old_count = stream.ref_count.fetch_sub(1, Ordering::Relaxed);
    tracing::debug!(
        "stream::release - ref_count was {}, now {}",
        old_count,
        old_count - 1
    );

    if old_count == 1 {
        // Last reference - destroy the object
        tracing::debug!("stream::release - dropping stream (last reference)");
        unsafe {
            drop(Box::from_raw(this.cast::<MemoryStream>()));
        }
        0
    } else {
        old_count - 1
    }
}

#[allow(unsafe_code)]
#[allow(clippy::cast_sign_loss)]
unsafe extern "C" fn read(
    this: *mut c_void,
    buffer: *mut c_void,
    num_bytes: i32,
    num_bytes_read: *mut i32,
) -> i32 {
    if this.is_null() || buffer.is_null() {
        tracing::warn!("stream::read - null pointer");
        return K_RESULT_FALSE;
    }

    let stream = unsafe { &mut *(this.cast::<MemoryStream>()) };
    let to_read = num_bytes.max(0) as usize;
    let available = stream.data.len().saturating_sub(stream.position);
    let actual_read = to_read.min(available);

    tracing::debug!(
        "stream::read - requested: {}, available: {}, actual: {}, pos: {}",
        to_read,
        available,
        actual_read,
        stream.position
    );

    if actual_read > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                stream.data.as_ptr().add(stream.position),
                buffer.cast::<u8>(),
                actual_read,
            );
        }
        stream.position += actual_read;
    }

    if !num_bytes_read.is_null() {
        unsafe {
            *num_bytes_read = actual_read as i32;
        }
    }

    K_RESULT_OK
}

#[allow(unsafe_code)]
#[allow(clippy::cast_sign_loss)]
unsafe extern "C" fn write(
    this: *mut c_void,
    buffer: *const c_void,
    num_bytes: i32,
    num_bytes_written: *mut i32,
) -> i32 {
    if this.is_null() || buffer.is_null() {
        return K_RESULT_FALSE;
    }

    let stream = unsafe { &mut *(this.cast::<MemoryStream>()) };
    let to_write = num_bytes.max(0) as usize;

    // Ensure buffer has enough capacity
    if stream.position + to_write > stream.data.len() {
        stream.data.resize(stream.position + to_write, 0);
    }

    if to_write > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                buffer.cast::<u8>(),
                stream.data.as_mut_ptr().add(stream.position),
                to_write,
            );
        }
        stream.position += to_write;
    }

    if !num_bytes_written.is_null() {
        unsafe {
            *num_bytes_written = to_write as i32;
        }
    }

    K_RESULT_OK
}

#[allow(unsafe_code)]
#[allow(clippy::cast_possible_truncation)]
unsafe extern "C" fn seek(this: *mut c_void, pos: i64, mode: i32, result: *mut i64) -> i32 {
    if this.is_null() {
        return K_RESULT_FALSE;
    }

    let stream = unsafe { &mut *(this.cast::<MemoryStream>()) };

    let new_pos = match mode {
        K_IBSEEK_SET => pos.max(0) as usize,
        K_IBSEEK_CUR => (stream.position as i64 + pos).max(0) as usize,
        K_IBSEEK_END => (stream.data.len() as i64 + pos).max(0) as usize,
        _ => return K_RESULT_FALSE,
    };

    stream.position = new_pos;

    if !result.is_null() {
        unsafe {
            *result = stream.position as i64;
        }
    }

    K_RESULT_OK
}

#[allow(unsafe_code)]
unsafe extern "C" fn tell(this: *mut c_void, pos: *mut i64) -> i32 {
    if this.is_null() || pos.is_null() {
        return K_RESULT_FALSE;
    }

    let stream = unsafe { &*(this.cast::<MemoryStream>()) };

    unsafe {
        *pos = stream.position as i64;
    }

    K_RESULT_OK
}
