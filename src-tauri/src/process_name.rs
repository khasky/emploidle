use std::io;

fn random_name() -> String {
    format!("p{:014x}", rand::random::<u64>() & 0x00ff_ffff_ffff_ffff)
}

#[cfg(target_os = "linux")]
pub fn prepare() -> io::Result<Option<String>> {
    let name = random_name();
    std::fs::write("/proc/self/comm", &name)?;
    Ok(Some(name))
}

#[cfg(target_os = "macos")]
pub fn prepare() -> io::Result<Option<String>> {
    use std::ffi::{c_char, c_int, c_void, CString};

    extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            old: *mut c_void,
            old_len: *mut usize,
            new: *mut c_void,
            new_len: usize,
        ) -> c_int;
    }

    let process_name = random_name();
    let name = CString::new(process_name.as_str()).unwrap();
    // kern.procname changes the calling process without modifying its signed bundle.
    let result = unsafe {
        sysctlbyname(
            b"kern.procname\0".as_ptr().cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_ptr().cast_mut().cast(),
            name.as_bytes().len(),
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Some(process_name))
}

#[cfg(target_os = "windows")]
const LAUNCH_PATH: &str = "EMPLOIDLE_LAUNCH_PATH";

#[cfg(target_os = "windows")]
pub fn prepare() -> io::Result<Option<String>> {
    use std::{env, fs, path::PathBuf, process::Command};

    let executable = env::current_exe()?.canonicalize()?;
    let launched_path = env::var_os(LAUNCH_PATH);
    env::remove_var(LAUNCH_PATH);
    if let Some(path) = launched_path {
        if PathBuf::from(path).canonicalize()? == executable {
            return Ok(Some(
                executable
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ));
        }
    }

    let directory = env::temp_dir().join("emploidle-processes");
    fs::create_dir_all(&directory)?;
    for entry in fs::read_dir(&directory)?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() == 19
            && name.starts_with('p')
            && name.ends_with(".exe")
            && name.as_bytes()[1..15]
                .iter()
                .all(|byte| byte.is_ascii_hexdigit())
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            // Windows keeps running images locked; remove copies from earlier runs.
            let _ = fs::remove_file(entry.path());
        }
    }

    let path = directory.join(format!("{}.exe", random_name()));
    let mut original = fs::File::open(executable)?;
    let mut copy = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let result = io::copy(&mut original, &mut copy);
    drop(copy);
    if let Err(error) = result {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    if let Err(error) = clear_display_metadata(&path) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    if let Err(error) = Command::new(&path)
        .args(env::args_os().skip(1))
        .env(LAUNCH_PATH, &path)
        .spawn()
    {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(None)
}

#[cfg(target_os = "windows")]
fn clear_display_metadata(path: &std::path::Path) -> io::Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn EnumResourceLanguagesW(
            module: *mut c_void,
            kind: *const u16,
            name: *const u16,
            callback: unsafe extern "system" fn(
                *mut c_void,
                *const u16,
                *const u16,
                u16,
                isize,
            ) -> i32,
            parameter: isize,
        ) -> i32;
        fn BeginUpdateResourceW(path: *const u16, delete_existing: i32) -> *mut c_void;
        fn UpdateResourceW(
            handle: *mut c_void,
            kind: *const u16,
            name: *const u16,
            language: u16,
            data: *mut c_void,
            length: u32,
        ) -> i32;
        fn EndUpdateResourceW(handle: *mut c_void, discard: i32) -> i32;
    }

    unsafe extern "system" fn collect_language(
        _: *mut c_void,
        _: *const u16,
        _: *const u16,
        language: u16,
        parameter: isize,
    ) -> i32 {
        let languages = &mut *(parameter as *mut Vec<u16>);
        languages.push(language);
        1
    }

    let mut languages = Vec::<u16>::new();
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Tauri emits VERSIONINFO as resource 1. Task Manager prefers its FileDescription.
    unsafe {
        let module = GetModuleHandleW(ptr::null());
        if module.is_null() {
            return Err(io::Error::last_os_error());
        }
        if EnumResourceLanguagesW(
            module,
            ptr::without_provenance(16),
            ptr::without_provenance(1),
            collect_language,
            &mut languages as *mut Vec<u16> as isize,
        ) == 0
        {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(1812..=1814)) {
                return Ok(());
            }
            return Err(error);
        }
        let handle = BeginUpdateResourceW(wide_path.as_ptr(), 0);
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        for language in languages {
            if UpdateResourceW(
                handle,
                ptr::without_provenance(16),
                ptr::without_provenance(1),
                language,
                ptr::null_mut(),
                0,
            ) == 0
            {
                let error = io::Error::last_os_error();
                EndUpdateResourceW(handle, 1);
                return Err(error);
            }
        }
        if EndUpdateResourceW(handle, 0) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fit_native_process_limits_and_change() {
        let names: std::collections::HashSet<_> = (0..128).map(|_| random_name()).collect();
        assert_eq!(names.len(), 128);
        assert!(names.iter().all(|name| name.len() == 15
            && name.starts_with('p')
            && name[1..].bytes().all(|byte| byte.is_ascii_hexdigit())));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_relaunch_uses_new_image_and_preserves_arguments() {
        use std::{env, process::Command};

        const PROBE: &str = "EMPLOIDLE_PROCESS_PROBE";
        if env::var_os(PROBE).is_some() {
            if let Some(name) = prepare().unwrap() {
                assert!(env::var_os(LAUNCH_PATH).is_none());
                let executable = env::current_exe().unwrap();
                let stem = executable.file_stem().unwrap().to_str().unwrap();
                assert_eq!(stem.len(), 15);
                assert_eq!(name, stem);
                assert!(env::args().any(|arg| arg == "--nocapture"));
                println!("RANDOM_IMAGE={stem}");
            }
            return;
        }

        let mut names = std::collections::HashSet::new();
        for _ in 0..2 {
            let output = Command::new(env::current_exe().unwrap())
                .args([
                    "--exact",
                    "process_name::tests::windows_relaunch_uses_new_image_and_preserves_arguments",
                    "--nocapture",
                ])
                .env(PROBE, "1")
                .env_remove(LAUNCH_PATH)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let stdout = String::from_utf8(output.stdout).unwrap();
            let name = stdout
                .lines()
                .find_map(|line| line.strip_prefix("RANDOM_IMAGE="))
                .unwrap_or_else(|| panic!("randomized child did not finish: {stdout}"));
            assert!(names.insert(name.to_owned()), "reused image name: {name}");
        }
    }
}
