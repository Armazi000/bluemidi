use std::ffi::c_void;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};

#[link(name = "avrt")]
unsafe extern "system" {
    fn AvSetMmThreadCharacteristicsW(task_name: *const u16, task_index: *mut u32) -> *mut c_void;
    fn AvRevertMmThreadCharacteristics(handle: *mut c_void) -> i32;
}

pub struct MmcssGuard {
    handle: *mut c_void,
}

impl MmcssGuard {
    pub fn new(task_name: &str) -> Self {
        let mut wide_task: Vec<u16> = task_name.encode_utf16().collect();
        wide_task.push(0);

        let mut task_index = 0u32;
        let handle = unsafe {
            AvSetMmThreadCharacteristicsW(wide_task.as_ptr(), &mut task_index)
        };

        unsafe {
            let thread: HANDLE = GetCurrentThread();
            let _ = SetThreadPriority(thread, THREAD_PRIORITY_TIME_CRITICAL);
        }

        Self { handle }
    }
}

impl Drop for MmcssGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                AvRevertMmThreadCharacteristics(self.handle);
            }
        }
    }
}
