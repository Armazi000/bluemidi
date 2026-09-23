use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

pub type MidiCallback = Box<dyn Fn(&[u8]) + Send + Sync + 'static>;

type LpvmMidiDataCb = Option<unsafe extern "system" fn(
    midi_data_bytes: *const u8,
    length: u32,
    dw_callback_instance: usize,
)>;

type FnCreatePort = unsafe extern "system" fn(
    port_name: *const u16,
    callback: LpvmMidiDataCb,
    dw_callback_instance: usize,
    max_sysex_length: u32,
    flags: u32,
) -> *mut c_void;

type FnClosePort = unsafe extern "system" fn(port: *mut c_void);
type FnSendData = unsafe extern "system" fn(port: *mut c_void, data: *const u8, length: u32) -> i32;

struct VirtualMidiFfi {
    _lib: Library,
    create_port: FnCreatePort,
    close_port: FnClosePort,
    send_data: FnSendData,
}

pub struct VirtualMidiPort {
    ffi: Arc<VirtualMidiFfi>,
    handle: AtomicPtr<c_void>,
    _callback: Option<Box<MidiCallback>>,
}

unsafe extern "system" fn native_midi_callback(
    midi_data_bytes: *const u8,
    length: u32,
    dw_callback_instance: usize,
) {
    if dw_callback_instance == 0 || midi_data_bytes.is_null() || length == 0 {
        return;
    }
    unsafe {
        let cb = dw_callback_instance as *const MidiCallback;
        let data = std::slice::from_raw_parts(midi_data_bytes, length as usize);
        (*cb)(data);
    }
}

impl VirtualMidiPort {
    pub fn new(port_name: &str, on_receive: Option<MidiCallback>) -> Result<Self, String> {
        let lib = unsafe {
            Library::new("teVirtualMIDI64.dll")
                .or_else(|_| Library::new("tevirtualMIDI.dll"))
                .map_err(|e| format!("Failed to load teVirtualMIDI DLL: {}", e))?
        };

        let ffi = unsafe {
            let create_port: Symbol<FnCreatePort> = lib
                .get(b"virtualMIDICreatePortEx2\0")
                .map_err(|e| format!("Missing virtualMIDICreatePortEx2: {}", e))?;
            let close_port: Symbol<FnClosePort> = lib
                .get(b"virtualMIDIClosePort\0")
                .map_err(|e| format!("Missing virtualMIDIClosePort: {}", e))?;
            let send_data: Symbol<FnSendData> = lib
                .get(b"virtualMIDISendData\0")
                .map_err(|e| format!("Missing virtualMIDISendData: {}", e))?;

            Arc::new(VirtualMidiFfi {
                create_port: *create_port,
                close_port: *close_port,
                send_data: *send_data,
                _lib: lib,
            })
        };

        let mut wide_name: Vec<u16> = port_name.encode_utf16().collect();
        wide_name.push(0);

        let (cb_fn, cb_instance, boxed_cb) = if let Some(cb) = on_receive {
            let boxed = Box::new(cb);
            let ptr = boxed.as_ref() as *const MidiCallback as usize;
            (Some(native_midi_callback as _), ptr, Some(boxed))
        } else {
            (None, 0, None)
        };

        let flags = 1;
        let max_sysex = 65535;

        let handle = unsafe {
            (ffi.create_port)(
                wide_name.as_ptr(),
                cb_fn,
                cb_instance,
                max_sysex,
                flags,
            )
        };

        if handle.is_null() {
            return Err(format!(
                "Failed to create virtual MIDI port '{}' (OS error: {})",
                port_name,
                std::io::Error::last_os_error()
            ));
        }

        Ok(Self {
            ffi,
            handle: AtomicPtr::new(handle),
            _callback: boxed_cb,
        })
    }

    #[inline(always)]
    pub fn send(&self, data: &[u8]) -> bool {
        let handle = self.handle.load(Ordering::Relaxed);
        if handle.is_null() || data.is_empty() {
            return false;
        }
        let res = unsafe { (self.ffi.send_data)(handle, data.as_ptr(), data.len() as u32) };
        res != 0
    }
}

impl Drop for VirtualMidiPort {
    fn drop(&mut self) {
        let handle = self.handle.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !handle.is_null() {
            unsafe {
                (self.ffi.close_port)(handle);
            }
        }
    }
}

unsafe impl Send for VirtualMidiPort {}
unsafe impl Sync for VirtualMidiPort {}
