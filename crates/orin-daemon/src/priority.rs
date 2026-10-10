//! Thread priority for background indexing work.

/// `THREAD_PRIORITY_BELOW_NORMAL`: one step below the normal priority.
#[cfg(windows)]
const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThread() -> *mut core::ffi::c_void;
    fn SetThreadPriority(thread: *mut core::ffi::c_void, priority: i32) -> i32;
}

/// Drop the calling thread to below-normal priority on Windows.
///
/// Interactive query latency must never compete with background indexing, so
/// the startup scan/revalidation worker and every on-demand revalidation
/// thread call this first. The workspace declares no windows crate and `std`
/// has no portable priority API, so kernel32 is declared directly instead of
/// adding a dependency.
#[cfg(windows)]
pub fn set_below_normal() {
    // SAFETY: the pseudo-handle from `GetCurrentThread` is valid for the
    // calling thread and `SetThreadPriority` does not retain it.
    unsafe {
        let thread = GetCurrentThread();
        let _ = SetThreadPriority(thread, THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// No-op off Windows: `std` has no portable thread-priority API.
#[cfg(not(windows))]
pub fn set_below_normal() {}
