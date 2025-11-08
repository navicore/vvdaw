//! Shared memory implementation for IPC audio buffers.
//!
//! This module provides platform-specific shared memory allocation
//! for zero-copy audio buffer transfer between processes.

use std::io;
use std::ptr::NonNull;
use vvdaw_plugin::PluginError;

/// Platform-specific shared memory handle
#[cfg(unix)]
use std::os::unix::io::RawFd;

/// Shared memory region for audio buffers
pub struct SharedMemory {
    /// Platform-specific file descriptor
    #[cfg(unix)]
    fd: RawFd,

    /// Pointer to mapped memory
    ptr: NonNull<u8>,

    /// Size of the memory region
    size: usize,

    /// Name of the shared memory region (for cleanup)
    name: String,
}

impl SharedMemory {
    /// Create a new shared memory region
    ///
    /// # Safety
    ///
    /// This function uses unsafe operations to create and map shared memory.
    /// The caller must ensure:
    /// - The name is unique and doesn't conflict with existing regions
    /// - The size is appropriate for the data structure
    /// - The memory is properly synchronized between processes
    #[allow(unsafe_code)]
    pub fn create(name: &str, size: usize) -> Result<Self, PluginError> {
        #[cfg(target_os = "macos")]
        {
            Self::create_posix(name, size)
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(PluginError::FormatError(
                "Shared memory not implemented for this platform".to_string(),
            ))
        }
    }

    /// Create shared memory using POSIX `shm_open` (macOS, Linux, BSD)
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn create_posix(name: &str, size: usize) -> Result<Self, PluginError> {
        use std::ffi::CString;

        // Create shared memory object
        let c_name = CString::new(name)
            .map_err(|e| PluginError::FormatError(format!("Invalid shared memory name: {e}")))?;

        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_RDWR,
                0o600, // Owner read/write only
            )
        };

        if fd < 0 {
            return Err(PluginError::FormatError(format!(
                "Failed to create shared memory: {}",
                io::Error::last_os_error()
            )));
        }

        // Set size of shared memory
        if unsafe { libc::ftruncate(fd, size as i64) } != 0 {
            unsafe { libc::close(fd) };
            return Err(PluginError::FormatError(format!(
                "Failed to set shared memory size: {}",
                io::Error::last_os_error()
            )));
        }

        // Map the shared memory
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(PluginError::FormatError(format!(
                "Failed to map shared memory: {}",
                io::Error::last_os_error()
            )));
        }

        Ok(Self {
            fd,
            ptr: NonNull::new(ptr.cast::<u8>()).unwrap(),
            size,
            name: name.to_string(),
        })
    }

    /// Open an existing shared memory region
    #[allow(unsafe_code)]
    pub fn open(name: &str, size: usize) -> Result<Self, PluginError> {
        #[cfg(target_os = "macos")]
        {
            Self::open_posix(name, size)
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(PluginError::FormatError(
                "Shared memory not implemented for this platform".to_string(),
            ))
        }
    }

    /// Open shared memory using POSIX `shm_open`
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn open_posix(name: &str, size: usize) -> Result<Self, PluginError> {
        use std::ffi::CString;

        let c_name = CString::new(name)
            .map_err(|e| PluginError::FormatError(format!("Invalid shared memory name: {e}")))?;

        let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0o600) };

        if fd < 0 {
            return Err(PluginError::FormatError(format!(
                "Failed to open shared memory: {}",
                io::Error::last_os_error()
            )));
        }

        // Map the shared memory
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(PluginError::FormatError(format!(
                "Failed to map shared memory: {}",
                io::Error::last_os_error()
            )));
        }

        Ok(Self {
            fd,
            ptr: NonNull::new(ptr.cast::<u8>()).unwrap(),
            size,
            name: name.to_string(),
        })
    }

    /// Get a typed reference to the shared memory
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `T` is the correct type for the memory region
    /// - The memory region is at least `size_of::<T>()` bytes
    /// - Proper synchronization is used when accessing from multiple processes
    #[allow(unsafe_code)]
    pub unsafe fn as_ref<T>(&self) -> &T {
        unsafe { &*self.ptr.as_ptr().cast::<T>() }
    }

    /// Get a typed mutable reference to the shared memory
    ///
    /// # Safety
    ///
    /// Same requirements as [`as_ref`](Self::as_ref), plus:
    /// - No other references to the same memory exist
    /// - Or proper synchronization is used (atomics, mutexes, etc.)
    #[allow(unsafe_code)]
    pub unsafe fn as_mut<T>(&mut self) -> &mut T {
        unsafe { &mut *self.ptr.as_ptr().cast::<T>() }
    }

    /// Get the size of the shared memory region
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the name of the shared memory region
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for SharedMemory {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // Unmap memory
        #[cfg(unix)]
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast(), self.size);
            libc::close(self.fd);

            // Unlink the shared memory object (only creator should do this)
            // In a production system, we'd track who created it
            let c_name = std::ffi::CString::new(self.name.as_str()).unwrap();
            libc::shm_unlink(c_name.as_ptr());
        }
    }
}

// Shared memory is explicitly designed to be shared between processes
#[allow(unsafe_code)]
unsafe impl Send for SharedMemory {}
#[allow(unsafe_code)]
unsafe impl Sync for SharedMemory {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn test_shared_memory_create_and_open() {
        let name = "/test_vvdaw_shm";
        let size = 4096;

        // Create shared memory
        let mut shm_creator = SharedMemory::create(name, size).expect("Failed to create shm");
        assert_eq!(shm_creator.size(), size);

        // Write some data
        unsafe {
            let data = shm_creator.as_mut::<[u8; 4096]>();
            data[0] = 42;
            data[100] = 123;
        }

        // Open from another "process" (same process for testing)
        let shm_reader = SharedMemory::open(name, size).expect("Failed to open shm");

        // Read the data
        unsafe {
            let data = shm_reader.as_ref::<[u8; 4096]>();
            assert_eq!(data[0], 42);
            assert_eq!(data[100], 123);
        }

        // Cleanup happens in drop()
    }
}
