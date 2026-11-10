use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;

const ENDPOINT: &str = "https://github.com/khasky/emploidle/releases/latest/download/latest.json";
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEYwOUM5NDM3QTdCMTczRTEKUldUaGM3R25ONVNjOE9MNmZVbHA2SGlnYnBYaWVCRktLM2MyVkNYZXNqcno4YkRxdmozZ0lzbU0K";

#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: HashMap<String, Platform>,
}

#[derive(Clone, Deserialize)]
struct Platform {
    url: String,
    signature: String,
}

#[derive(Clone)]
pub struct Update {
    pub version: String,
    platform: Platform,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(concat!("Emploidle/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn target() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        other => return Err(format!("Updates are not available for {other}")),
    };
    Ok(format!("{os}-{}", std::env::consts::ARCH))
}

fn newer(manifest: Manifest, current: &str, target: &str) -> Result<Option<Update>, String> {
    let current = semver::Version::parse(current).map_err(|error| error.to_string())?;
    let version = semver::Version::parse(manifest.version.trim_start_matches('v'))
        .map_err(|error| format!("Invalid update version: {error}"))?;
    if version <= current {
        return Ok(None);
    }
    let platform = manifest
        .platforms
        .get(target)
        .cloned()
        .ok_or_else(|| format!("The update does not include {target}"))?;
    Ok(Some(Update {
        version: version.to_string(),
        platform,
    }))
}

pub fn check() -> Result<Option<Update>, String> {
    let manifest: Manifest = agent()
        .get(ENDPOINT)
        .call()
        .map_err(|error| error.to_string())?
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    newer(manifest, env!("CARGO_PKG_VERSION"), &target()?)
}

fn verify(public_key: &str, bytes: &[u8], signature: &str) -> Result<(), String> {
    let decode = |value: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let key = decode(public_key)
        .and_then(|key| minisign_verify::PublicKey::decode(&key).ok())
        .ok_or("Invalid update public key")?;
    let signature = decode(signature)
        .and_then(|signature| minisign_verify::Signature::decode(&signature).ok())
        .ok_or("Invalid update signature")?;
    key.verify(bytes, &signature, true)
        .map_err(|error| format!("Update signature verification failed: {error}"))
}

/// Downloads the update and verifies its signature before returning it.
pub fn download(
    update: &Update,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let mut response = agent()
        .get(&update.platform.url)
        .call()
        .map_err(|error| error.to_string())?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        progress(bytes.len() as u64, total);
    }
    verify(PUBLIC_KEY, &bytes, &update.platform.signature)?;
    Ok(bytes)
}

#[cfg_attr(target_os = "linux", allow(dead_code))]
fn temporary(name: &str) -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join(format!("emploidle-update-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join(name))
}

/// Installs verified update bytes and starts the updated application.
/// The caller exits after a successful return.
#[cfg(target_os = "windows")]
pub fn install(bytes: &[u8]) -> Result<(), String> {
    let installer = temporary("Emploidle-setup.exe")?;
    std::fs::write(&installer, bytes).map_err(|error| error.to_string())?;
    // The NSIS installer closes the old version and relaunches the new one.
    std::process::Command::new(&installer)
        .args(["/P", "/R"])
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Cannot start the installer: {error}"))
}

#[cfg(target_os = "macos")]
pub fn install(bytes: &[u8]) -> Result<(), String> {
    let executable = crate::process_name::executable_path().map_err(|error| error.to_string())?;
    let bundle = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or("Updates require Emploidle.app")?
        .to_path_buf();
    let extracted = temporary("extracted")?;
    tar::Archive::new(flate2::read::GzDecoder::new(bytes))
        .unpack(&extracted)
        .map_err(|error| format!("Cannot extract the update: {error}"))?;
    let new_bundle = std::fs::read_dir(&extracted)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .ok_or("The update does not contain an application")?;
    let backup = bundle.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(&bundle, &backup).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::rename(&new_bundle, &bundle) {
        let _ = std::fs::rename(&backup, &bundle);
        return Err(format!("Cannot replace the application: {error}"));
    }
    let _ = std::fs::remove_dir_all(&backup);
    std::process::Command::new("open")
        .arg("-n")
        .arg(&bundle)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
pub fn install(bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let image = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or("Updates are only available for the AppImage")?;
    let staged = image.with_extension("update");
    std::fs::write(&staged, bytes).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .and_then(|()| std::fs::rename(&staged, &image))
        .map_err(|error| {
            let _ = std::fs::remove_file(&staged);
            format!("Cannot replace the AppImage: {error}")
        })?;
    std::process::Command::new(&image)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str) -> Manifest {
        serde_json::from_str(&format!(
            r#"{{"version":"{version}","notes":"","pub_date":"2026-01-01T00:00:00Z","platforms":{{"windows-x86_64":{{"signature":"c2ln","url":"https://example.com/setup.exe"}}}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn only_newer_versions_for_this_platform_are_offered() {
        let update = newer(manifest("v1.2.0"), "1.1.9", "windows-x86_64")
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "1.2.0");
        assert_eq!(update.platform.url, "https://example.com/setup.exe");
        assert!(newer(manifest("1.1.9"), "1.1.9", "windows-x86_64")
            .unwrap()
            .is_none());
        assert!(newer(manifest("1.0.0"), "1.1.9", "windows-x86_64")
            .unwrap()
            .is_none());
        assert!(newer(manifest("1.2.0"), "1.1.9", "linux-x86_64").is_err());
    }

    #[test]
    fn unsigned_or_tampered_payloads_are_rejected() {
        assert!(verify(PUBLIC_KEY, b"payload", "c2ln").is_err());
        assert!(verify(PUBLIC_KEY, b"payload", "").is_err());
    }

    #[test]
    fn cargo_packager_signatures_verify() {
        // Produced by `cargo packager signer` with a throwaway key.
        let key = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDUxMzFBODU0Q0U4RUE3RTYKUldUbXA0N09WS2d4VWZPcC9URjE1NUZuQTJIVHVXVnloL0dhd1QreU5JeXBYcXVESWZRSjJockQK";
        let signature = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIGNhcmdvLXBhY2thZ2VyIHNlY3JldCBrZXkKUlVUbXA0N09WS2d4VVdYVUorazJLR3h2M0JzU0l1L2R2N2JUSFduMmdYeVNGa3hWdTZIcHQ0Y2l4SkRHZUN2V1hmNkhFb1Myd3VtdUFGdGhVQkJzRjIzWjJucmRJL3hYRmc4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0Mjc3MjEzCWZpbGU6cGF5bG9hZC5iaW4Kb2JMNTZRRlU1M1F0OVUwRVhBREpzcmhhYklocVFxVTBvcVljaGs2Y0p0LzdCZ0VNK1hnTXlRc0hENG9jTFltZi9yMDZ6N3orV1loaytmS2pNMUdGQ1E9PQo=";
        assert!(verify(key, b"emploidle update payload", signature).is_ok());
        assert!(verify(key, b"emploidle update payload!", signature).is_err());
        assert!(verify(PUBLIC_KEY, b"emploidle update payload", signature).is_err());
    }
}
