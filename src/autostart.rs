use std::path::Path;

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    type Key = *mut c_void;
    const ROOT: Key = -2147483647isize as Key;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            root: Key,
            path: *const u16,
            options: u32,
            access: u32,
            key: *mut Key,
        ) -> i32;
        fn RegCreateKeyExW(
            root: Key,
            path: *const u16,
            reserved: u32,
            class: *mut u16,
            options: u32,
            access: u32,
            security: *const c_void,
            key: *mut Key,
            disposition: *mut u32,
        ) -> i32;
        fn RegQueryValueExW(
            key: Key,
            name: *const u16,
            reserved: *mut u32,
            kind: *mut u32,
            data: *mut u8,
            size: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            key: Key,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            size: u32,
        ) -> i32;
        fn RegDeleteValueW(key: Key, name: *const u16) -> i32;
        fn RegCloseKey(key: Key) -> i32;
    }
    struct Owned(Key);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn location() -> Vec<u16> {
        wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    }
    fn failure(code: i32) -> String {
        format!(
            "Cannot update Emploidle startup registration: {}",
            std::io::Error::from_raw_os_error(code)
        )
    }
    pub(super) fn command(executable: &Path) -> Vec<u16> {
        std::iter::once(b'"' as u16)
            .chain(executable.as_os_str().encode_wide())
            .chain("\" --background".encode_utf16())
            .chain(Some(0))
            .collect()
    }
    pub fn enabled() -> Result<bool, String> {
        let mut key = ptr::null_mut();
        let status = unsafe { RegOpenKeyExW(ROOT, location().as_ptr(), 0, 1, &mut key) };
        if status == 2 {
            return Ok(false);
        }
        if status != 0 {
            return Err(failure(status));
        }
        let key = Owned(key);
        let (mut kind, mut size) = (0, 0);
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                wide("Emploidle").as_ptr(),
                ptr::null_mut(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        match status {
            0 => Ok(kind == 1 && size > 2),
            2 => Ok(false),
            _ => Err(failure(status)),
        }
    }
    pub fn set_enabled(enable: bool, executable: &Path) -> Result<(), String> {
        let mut key = ptr::null_mut();
        let status = unsafe {
            if enable {
                RegCreateKeyExW(
                    ROOT,
                    location().as_ptr(),
                    0,
                    ptr::null_mut(),
                    0,
                    2,
                    ptr::null(),
                    &mut key,
                    ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(ROOT, location().as_ptr(), 0, 2, &mut key)
            }
        };
        if !enable && status == 2 {
            return Ok(());
        }
        if status != 0 {
            return Err(failure(status));
        }
        let key = Owned(key);
        let name = wide("Emploidle");
        let status = unsafe {
            if enable {
                let value = command(executable);
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    1,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            } else {
                RegDeleteValueW(key.0, name.as_ptr())
            }
        };
        if status == 0 || (!enable && status == 2) {
            Ok(())
        } else {
            Err(failure(status))
        }
    }
}

#[cfg(unix)]
mod native {
    use super::*;
    use std::{fs, io::ErrorKind, path::PathBuf};
    const MARKER: &str = "Emploidle autostart";
    fn path() -> Result<PathBuf, String> {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or("Cannot locate the user home directory")?;
        #[cfg(target_os = "macos")]
        {
            Ok(PathBuf::from(home).join("Library/LaunchAgents/com.khasky.emploidle.plist"))
        }
        #[cfg(target_os = "linux")]
        {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| PathBuf::from(home).join(".config"));
            Ok(config.join("autostart/emploidle.desktop"))
        }
    }
    fn owned(path: &Path) -> Result<bool, String> {
        match fs::read_to_string(path) {
            Ok(text) if text.contains(MARKER) => Ok(true),
            Ok(_) => Err("The startup entry exists but is not owned by Emploidle".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn enabled() -> Result<bool, String> {
        owned(&path()?)
    }
    pub fn set_enabled(enable: bool, executable: &Path) -> Result<(), String> {
        let path = path()?;
        let exists = owned(&path)?;
        if enable {
            let text = executable
                .to_str()
                .ok_or("Startup executable path is not valid Unicode")?;
            let contents = entry(text)?;
            fs::create_dir_all(path.parent().ok_or("Invalid startup entry path")?)
                .map_err(|error| error.to_string())?;
            fs::write(path, contents).map_err(|error| error.to_string())
        } else if exists {
            fs::remove_file(path).map_err(|error| error.to_string())
        } else {
            Ok(())
        }
    }
    #[cfg(target_os = "macos")]
    fn entry(executable: &str) -> Result<String, String> {
        Ok(plist(executable))
    }
    #[cfg(target_os = "linux")]
    fn entry(executable: &str) -> Result<String, String> {
        desktop_entry(executable)
    }
}

pub fn enabled() -> Result<bool, String> {
    native::enabled()
}
pub fn set_enabled(enable: bool, executable: &Path) -> Result<(), String> {
    if enable && (!executable.is_absolute() || !executable.is_file()) {
        return Err("Startup requires the original installed executable path".into());
    }
    native::set_enabled(enable, executable)
}

#[cfg(any(target_os = "macos", test))]
fn plist(executable: &str) -> String {
    let escaped = executable
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- Emploidle autostart -->\n<plist version=\"1.0\"><dict><key>Label</key><string>com.khasky.emploidle</string><key>ProgramArguments</key><array><string>{escaped}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>\n")
}
#[cfg(any(target_os = "linux", test))]
fn desktop_entry(executable: &str) -> Result<String, String> {
    if executable
        .chars()
        .any(|character| character.is_control() || character == '=')
    {
        return Err("Unsupported character in startup executable path".into());
    }
    // Desktop Entry decoding precedes Exec quoting, so literal backslashes need four.
    let mut quoted = String::new();
    for character in executable.chars() {
        match character {
            '\\' => quoted.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    Ok(format!("# Emploidle autostart\n[Desktop Entry]\nType=Application\nName=Emploidle\nExec=\"{quoted}\" --background\nTerminal=false\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plist_escapes_xml_and_keeps_arguments_separate() {
        let entry = plist("/Applications/A & B/<app>\"'.app/bin");
        assert!(entry
            .contains("A &amp; B/&lt;app&gt;&quot;&apos;.app/bin</string><string>--background"));
        assert!(!entry.contains("KeepAlive"));
    }
    #[test]
    fn desktop_escapes_both_string_and_exec_layers() {
        let entry = desktop_entry("/home/a b/100%\\\"$`app").unwrap();
        assert!(entry.contains(r#"Exec="/home/a b/100%%\\\\\\"\\$\\`app" --background"#));
        assert!(desktop_entry("/tmp/bad\nExec=other").is_err());
        assert!(desktop_entry("/tmp/a=b").is_err());
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_registration_quotes_executable_and_passes_background() {
        let value = native::command(Path::new("C:\\Program Files\\Emploidle\\emploidle.exe"));
        assert_eq!(
            String::from_utf16(&value[..value.len() - 1]).unwrap(),
            "\"C:\\Program Files\\Emploidle\\emploidle.exe\" --background"
        );
    }
}
