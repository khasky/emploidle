use super::{BrowserContext, BrowserSession, EditorSession, ScrollReceipt, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Serialize)]
struct Request {
    window: u64,
    process_id: u32,
    #[serde(flatten)]
    operation: Operation,
}

#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Operation {
    Inspect,
    Scroll {
        horizontal: bool,
        amount: i32,
    },
    Restore {
        receipt: ScrollReceipt,
    },
    BrowserContext,
    OpenTab,
    CloseTab {
        session: BrowserSession,
    },
    ReviewTree {
        index: u32,
    },
    BeginEditor {
        action: u32,
    },
    RestoreEditor {
        session: EditorSession,
        interrupted: bool,
    },
}

#[derive(Deserialize)]
struct Response<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
    #[serde(default)]
    commit_required: bool,
}

fn drain(
    mut pipe: impl Read + Send + 'static,
    response: Option<mpsc::Sender<Vec<u8>>>,
) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut response = response;
        loop {
            let read = pipe.read(&mut buffer)?;
            if read == 0 {
                return Ok(output);
            }
            // Continue draining after the cap so a noisy child cannot block on a full pipe.
            let retain = read.min(65_536usize.saturating_sub(output.len()));
            output.extend_from_slice(&buffer[..retain]);
            if let Some(end) = output.iter().position(|byte| *byte == b'\n') {
                if let Some(sender) = response.take() {
                    let _ = sender.send(output[..end].to_vec());
                }
            }
        }
    })
}

fn run<T: for<'de> Deserialize<'de>>(request: Request) -> Result<T, String> {
    #[derive(Serialize)]
    struct Envelope {
        #[serde(flatten)]
        request: Request,
        input_tick: u32,
    }
    let mutating = !matches!(
        request.operation,
        Operation::Inspect | Operation::BrowserContext
    );
    let payload = serde_json::to_vec(&Envelope {
        request,
        input_tick: super::windows::last_input_tick()?,
    })
    .map_err(|error| error.to_string())?;
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::InputEncoding = [Text.UTF8Encoding]::new($false); & ([scriptblock]::Create(([Console]::In.ReadLine() | ConvertFrom-Json)))",
        ])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Cannot start Windows accessibility helper: {error}"))?;
    let (sender, receiver) = mpsc::channel();
    let output = drain(
        child.stdout.take().ok_or("Missing accessibility output")?,
        Some(sender),
    );
    let errors = drain(
        child
            .stderr
            .take()
            .ok_or("Missing accessibility error output")?,
        None,
    );
    let result = (|| -> Result<T, String> {
        let input = child
            .stdin
            .as_mut()
            .ok_or("Missing accessibility request input")?;
        let script = format!(
            "{}\n{}",
            include_str!("vscode.ps1"),
            include_str!("accessibility.ps1")
        );
        let script = serde_json::to_vec(&script).map_err(|error| error.to_string())?;
        input
            .write_all(&script)
            .and_then(|()| input.write_all(b"\n"))
            .map_err(|error| format!("Cannot send accessibility helper: {error}"))?;
        input
            .write_all(&payload)
            .and_then(|()| input.write_all(b"\n"))
            .map_err(|error| format!("Cannot send accessibility request: {error}"))?;
        let response = receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|error| format!("Windows accessibility response unavailable: {error}"))?;
        let response: Response<T> = serde_json::from_slice(&response)
            .map_err(|error| format!("Invalid accessibility response: {error}"))?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Accessibility operation failed".into()));
        }
        let data = response
            .data
            .ok_or("Accessibility provider returned no result")?;
        // Acknowledge only after the recovery identity has reached the parent intact.
        if response.commit_required {
            input
                .write_all(b"commit\n")
                .map_err(|error| format!("Cannot acknowledge accessibility response: {error}"))?;
        }
        Ok(data)
    })();
    drop(child.stdin.take());
    if mutating {
        // EOF rolls back unacknowledged actions. An acknowledged identity must survive reaping errors.
        thread::spawn(move || {
            let _ = child.wait();
            let _ = output.join();
            let _ = errors.join();
        });
        return result;
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = output.join();
                let errors = errors
                    .join()
                    .map_err(|_| "Accessibility error reader failed")?
                    .map_err(|error| error.to_string())?;
                if !status.success() {
                    return Err(format!(
                        "Accessibility helper failed: {}",
                        String::from_utf8_lossy(&errors)
                    ));
                }
                return result;
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output.join();
                let _ = errors.join();
                return Err(
                    "Windows accessibility provider did not finish; activity must stop".into(),
                );
            }
        }
    }
}

pub fn inspect(window: u64, process_id: u32) -> Result<Snapshot, String> {
    run(Request {
        window,
        process_id,
        operation: Operation::Inspect,
    })
}

pub fn begin_editor(
    window: u64,
    process_id: u32,
    action: u32,
) -> Result<Option<EditorSession>, String> {
    #[derive(Deserialize)]
    struct Review {
        session: Option<EditorSession>,
    }
    run::<Review>(Request {
        window,
        process_id,
        operation: Operation::BeginEditor { action },
    })
    .map(|review| review.session)
}

pub fn restore_editor(
    window: u64,
    process_id: u32,
    session: EditorSession,
    interrupted: bool,
) -> Result<(), String> {
    run::<bool>(Request {
        window,
        process_id,
        operation: Operation::RestoreEditor {
            session,
            interrupted,
        },
    })
    .map(|_| ())
}

pub fn browser_context(window: u64, process_id: u32) -> Result<BrowserContext, String> {
    run(Request {
        window,
        process_id,
        operation: Operation::BrowserContext,
    })
}

pub fn open_tab(window: u64, process_id: u32) -> Result<BrowserSession, String> {
    run(Request {
        window,
        process_id,
        operation: Operation::OpenTab,
    })
}

pub fn close_tab(window: u64, process_id: u32, session: BrowserSession) -> Result<(), String> {
    run::<bool>(Request {
        window,
        process_id,
        operation: Operation::CloseTab { session },
    })
    .map(|_| ())
}

pub fn review_tree(window: u64, process_id: u32, index: u32) -> Result<bool, String> {
    run(Request {
        window,
        process_id,
        operation: Operation::ReviewTree { index },
    })
}

pub fn scroll(
    window: u64,
    process_id: u32,
    horizontal: bool,
    amount: i32,
) -> Result<ScrollReceipt, String> {
    run(Request {
        window,
        process_id,
        operation: Operation::Scroll { horizontal, amount },
    })
}

pub fn restore(window: u64, process_id: u32, receipt: ScrollReceipt) -> Result<(), String> {
    run::<bool>(Request {
        window,
        process_id,
        operation: Operation::Restore { receipt },
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct AwaitingAcknowledgement {
        response: Cursor<Vec<u8>>,
        release: mpsc::Receiver<()>,
    }

    impl Read for AwaitingAcknowledgement {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = self.response.read(buffer)?;
            if count == 0 {
                self.release.recv().unwrap();
            }
            Ok(count)
        }
    }

    #[test]
    fn recovery_identity_arrives_before_helper_acknowledgement() {
        let (release, waiting) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        let response = b"{\"ok\":true,\"data\":{\"created\":[1,2,3]}}\n".to_vec();
        let output = drain(
            AwaitingAcknowledgement {
                response: Cursor::new(response.clone()),
                release: waiting,
            },
            Some(sender),
        );
        let received = receiver.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        assert_eq!(received.unwrap(), response[..response.len() - 1]);
        assert_eq!(output.join().unwrap().unwrap(), response);
    }

    #[test]
    fn capped_output_is_drained_after_the_recovery_response() {
        let (sender, receiver) = mpsc::channel();
        let mut response = b"{\"ok\":true}\n".to_vec();
        response.resize(80_000, b'x');
        let output = drain(Cursor::new(response), Some(sender));
        assert_eq!(receiver.recv().unwrap(), b"{\"ok\":true}");
        assert_eq!(output.join().unwrap().unwrap().len(), 65_536);
    }
}
