use super::Snapshot;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Serialize)]
struct Request {
    window: u64,
    process_id: u32,
}

#[derive(Deserialize)]
struct Response<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = pipe.read(&mut buffer)?;
            if read == 0 {
                return Ok(output);
            }
            // Continue draining after the cap so a noisy child cannot block on a full pipe.
            let retain = read.min(65_536usize.saturating_sub(output.len()));
            output.extend_from_slice(&buffer[..retain]);
        }
    })
}

fn run<T: for<'de> Deserialize<'de>>(request: Request) -> Result<T, String> {
    let payload = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("accessibility.ps1"),
        ])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Cannot start Windows accessibility helper: {error}"))?;
    let output = drain(child.stdout.take().ok_or("Missing accessibility output")?);
    let errors = drain(
        child
            .stderr
            .take()
            .ok_or("Missing accessibility error output")?,
    );
    let write = child
        .stdin
        .take()
        .ok_or("Missing accessibility request input")?
        .write_all(&payload);
    if let Err(error) = write {
        let _ = child.kill();
        let _ = child.wait();
        let _ = output.join();
        let _ = errors.join();
        return Err(format!("Cannot send accessibility request: {error}"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => break Err("Windows accessibility provider timed out".to_owned()),
            Err(error) => break Err(format!("Cannot wait for accessibility helper: {error}")),
        }
    };
    if result.is_err() {
        // This handle belongs only to the helper created above, never the target application.
        let _ = child.kill();
        let _ = child.wait();
    }
    let output = output
        .join()
        .map_err(|_| "Accessibility output reader failed")?
        .map_err(|error| error.to_string())?;
    let errors = errors
        .join()
        .map_err(|_| "Accessibility error reader failed")?
        .map_err(|error| error.to_string())?;
    let status = result?;
    if !status.success() {
        return Err(format!(
            "Accessibility helper failed: {}",
            String::from_utf8_lossy(&errors)
        ));
    }
    let response: Response<T> = serde_json::from_slice(&output)
        .map_err(|error| format!("Invalid accessibility response: {error}"))?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "Accessibility operation failed".into()));
    }
    response
        .data
        .ok_or_else(|| "Accessibility provider returned no result".into())
}

pub fn inspect(window: u64, process_id: u32) -> Result<Snapshot, String> {
    run(Request { window, process_id })
}
