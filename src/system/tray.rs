use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, LoadIconW, PostQuitMessage, RegisterClassExW,
    SetForegroundWindow, TrackPopupMenu, TranslateMessage, HMENU, IDI_APPLICATION, MF_CHECKED,
    MF_DISABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, TPM_RIGHTBUTTON,
    WINDOW_EX_STYLE, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_USER, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW,
};

use crate::system::autostart::{is_autostart_enabled, set_autostart};

const WM_TRAYICON: u32 = WM_USER + 1;
const ID_STATUS: usize = 1001;
const ID_PORT: usize = 1002;
const ID_RECONNECT: usize = 1003;
const ID_AUTOSTART: usize = 1004;
const ID_EXIT: usize = 1005;

use std::sync::Mutex;

pub struct TrayManager {
    hwnd: HWND,
    _status: Arc<Mutex<String>>,
    _reconnect_requested: Arc<AtomicBool>,
    _exit_requested: Arc<AtomicBool>,
}

static TRAY_RECONNECT: AtomicBool = AtomicBool::new(false);
static TRAY_EXIT: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                unsafe {
                    show_tray_menu(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd = (wparam.0 & 0xFFFF) as usize;
            match cmd {
                ID_RECONNECT => {
                    TRAY_RECONNECT.store(true, Ordering::SeqCst);
                }
                ID_AUTOSTART => {
                    let current = is_autostart_enabled();
                    let _ = set_autostart(!current);
                }
                ID_EXIT => {
                    TRAY_EXIT.store(true, Ordering::SeqCst);
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

static CURRENT_STATUS: Mutex<String> = Mutex::new(String::new());
static PORT_NAME: Mutex<String> = Mutex::new(String::new());

unsafe fn show_tray_menu(hwnd: HWND) {
    let mut cursor = POINT::default();
    unsafe {
        if GetCursorPos(&mut cursor).is_err() {
            return;
        }

        let hmenu: HMENU = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        let status = {
            let lock = CURRENT_STATUS.lock().unwrap();
            if lock.is_empty() {
                "BlueMIDI: Scanning...".to_string()
            } else {
                lock.clone()
            }
        };

        let port = {
            let lock = PORT_NAME.lock().unwrap();
            format!("Port: {}", lock)
        };

        let mut wide_status: Vec<u16> = status.encode_utf16().collect();
        wide_status.push(0);

        let mut wide_port: Vec<u16> = port.encode_utf16().collect();
        wide_port.push(0);

        let _ = AppendMenuW(
            hmenu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            ID_STATUS,
            PCWSTR(wide_status.as_ptr()),
        );
        let _ = AppendMenuW(
            hmenu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            ID_PORT,
            PCWSTR(wide_port.as_ptr()),
        );
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());

        let _ = AppendMenuW(
            hmenu,
            MF_STRING,
            ID_RECONNECT,
            w!("Reconnect Device"),
        );

        let autostart_flag = if is_autostart_enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            hmenu,
            autostart_flag,
            ID_AUTOSTART,
            w!("Start with Windows"),
        );

        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(hmenu, MF_STRING, ID_EXIT, w!("Exit BlueMIDI"));

        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            hmenu,
            TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(hmenu);
    }
}

impl TrayManager {
    pub fn new(initial_status: &str, port_name: &str) -> Result<Self, String> {
        {
            let mut s = CURRENT_STATUS.lock().unwrap();
            *s = initial_status.to_string();
            let mut p = PORT_NAME.lock().unwrap();
            *p = port_name.to_string();
        }

        unsafe {
            let class_name = w!("BlueMidiTrayClass");
            let icon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                lpszClassName: class_name,
                hIcon: icon,
                ..Default::default()
            };

            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("BlueMIDI Background Service"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| format!("Failed to create background window: {}", e))?;

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: icon,
                ..Default::default()
            };

            let tip = "BlueMIDI - Low Latency BLE MIDI";
            for (i, c) in tip.encode_utf16().take(127).enumerate() {
                nid.szTip[i] = c;
            }

            let _ = Shell_NotifyIconW(NIM_ADD, &nid);

            Ok(Self {
                hwnd,
                _status: Arc::new(Mutex::new(initial_status.to_string())),
                _reconnect_requested: Arc::new(AtomicBool::new(false)),
                _exit_requested: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    pub fn update_status(&self, status: &str) {
        if let Ok(mut lock) = CURRENT_STATUS.lock() {
            *lock = status.to_string();
        }

        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                uFlags: NIF_TIP,
                ..Default::default()
            };

            let tip = format!("BlueMIDI: {}", status);
            for (i, c) in tip.encode_utf16().take(127).enumerate() {
                nid.szTip[i] = c;
            }

            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn should_reconnect(&self) -> bool {
        TRAY_RECONNECT.swap(false, Ordering::SeqCst)
    }

    pub fn should_exit(&self) -> bool {
        TRAY_EXIT.load(Ordering::SeqCst)
    }

    pub fn pump_messages(&self) -> bool {
        unsafe {
            let mut msg = MSG::default();
            while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut msg,
                Some(self.hwnd),
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
            .as_bool()
            {
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    return false;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            !self.should_exit()
        }
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        unsafe {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
