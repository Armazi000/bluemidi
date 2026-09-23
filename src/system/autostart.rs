use windows::core::w;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY_PATH: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APP_REG_NAME: windows::core::PCWSTR = w!("BlueMIDI");

pub fn is_autostart_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH, Some(0), KEY_READ, &mut hkey);
        if open_res != WIN32_ERROR(0) {
            return false;
        }

        let mut data_len = 0u32;
        let res = RegQueryValueExW(
            hkey,
            APP_REG_NAME,
            None,
            None,
            None,
            Some(&mut data_len),
        );

        let _ = RegCloseKey(hkey);
        res == WIN32_ERROR(0) && data_len > 0
    }
}

pub fn set_autostart(enable: bool) -> Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        let open_res = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            Some(0),
            KEY_WRITE | KEY_READ,
            &mut hkey,
        );
        if open_res != WIN32_ERROR(0) {
            return Err(format!("Failed to open registry run key: {:?}", open_res));
        }

        let res = if enable {
            let current_exe = std::env::current_exe()
                .map_err(|e| format!("Failed to get current executable path: {}", e))?;
            let path_str = current_exe.to_string_lossy().to_string();
            let mut wide_path: Vec<u16> = format!("\"{}\"", path_str).encode_utf16().collect();
            wide_path.push(0);

            let bytes = std::slice::from_raw_parts(
                wide_path.as_ptr() as *const u8,
                wide_path.len() * std::mem::size_of::<u16>(),
            );

            RegSetValueExW(
                hkey,
                APP_REG_NAME,
                Some(0),
                REG_SZ,
                Some(bytes),
            )
        } else {
            let del_res = RegDeleteValueW(hkey, APP_REG_NAME);
            if del_res == WIN32_ERROR(2) {
                WIN32_ERROR(0)
            } else {
                del_res
            }
        };

        let _ = RegCloseKey(hkey);

        if res == WIN32_ERROR(0) {
            Ok(())
        } else {
            Err(format!("Registry operation failed with code: {:?}", res))
        }
    }
}
