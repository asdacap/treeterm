use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tonic::Streaming;
use treeterm_proto::treeterm::*;

const READ_BUF_SIZE: usize = 4096;
/// Timeout for sending output to the gRPC channel. If the client can't keep up
/// for this long, we stop reading from the child's pipe — the child will get
/// SIGPIPE or block on its next write, which is better than freezing indefinitely.
const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn handle_exec_stream(
    mut in_stream: Streaming<ExecInput>,
    tx: mpsc::Sender<Result<ExecOutput, tonic::Status>>,
) {
    // 1. Wait for ExecStart as the first message
    let start = match in_stream.message().await {
        Ok(Some(ExecInput { input: Some(exec_input::Input::Start(s)) })) => s,
        Ok(_) => {
            let _ = send_result(&tx, 1, Some("expected ExecStart as first message".into())).await;
            return;
        }
        Err(e) => {
            let _ = send_result(&tx, 1, Some(format!("stream error: {e}"))).await;
            return;
        }
    };

    // 2. Spawn child process
    let mut cmd = Command::new(&start.command);
    cmd.args(&start.args)
        .current_dir(&start.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Merge env: inherit current + provided overrides
    if !start.env.is_empty() {
        cmd.envs(&start.env);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = send_result(&tx, 1, Some(format!("spawn error: {e}"))).await;
            return;
        }
    };

    let child_pid = child.id();
    let mut child_stdin = child.stdin.take();
    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    // 3. Spawn stdout reader
    let tx_out = tx.clone();
    let stdout_task = tokio::spawn(async move {
        if let Some(mut stdout) = child_stdout {
            let mut buf = vec![0u8; READ_BUF_SIZE];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let msg = ExecOutput {
                            output: Some(exec_output::Output::Stdout(ExecStdout {
                                data: buf[..n].to_vec(),
                            })),
                        };
                        match tokio::time::timeout(SEND_TIMEOUT, tx_out.send(Ok(msg))).await {
                            Ok(Ok(())) => {}
                            _ => break, // timeout elapsed or channel closed
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });

    // 4. Spawn stderr reader
    let tx_err = tx.clone();
    let stderr_task = tokio::spawn(async move {
        if let Some(mut stderr) = child_stderr {
            let mut buf = vec![0u8; READ_BUF_SIZE];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let msg = ExecOutput {
                            output: Some(exec_output::Output::Stderr(ExecStderr {
                                data: buf[..n].to_vec(),
                            })),
                        };
                        match tokio::time::timeout(SEND_TIMEOUT, tx_err.send(Ok(msg))).await {
                            Ok(Ok(())) => {}
                            _ => break, // timeout elapsed or channel closed
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });

    // 5. Spawn stdin writer / signal handler from incoming stream
    let stdin_task = tokio::spawn(async move {
        while let Ok(Some(msg)) = in_stream.message().await {
            match msg.input {
                Some(exec_input::Input::Stdin(data)) => {
                    if let Some(ref mut stdin) = child_stdin {
                        let _ = stdin.write_all(&data).await;
                    }
                }
                Some(exec_input::Input::Signal(sig)) => {
                    if let Some(pid) = child_pid {
                        unsafe {
                            libc::kill(pid as i32, sig.signal);
                        }
                    }
                }
                _ => {}
            }
        }
        // Client closed stream -- close stdin so child sees EOF
        drop(child_stdin);
    });

    // 6. Timeout handling
    let timeout_ms = start.timeout_ms.unwrap_or(0);
    let timeout_task = if timeout_ms > 0 {
        let pid = child_pid;
        Some(tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms as u64)).await;
            if let Some(pid) = pid {
                tracing::warn!(pid, timeout_ms, "exec timeout, sending SIGTERM");
                unsafe { libc::kill(pid as i32, libc::SIGTERM); }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                // SIGKILL as fallback -- process may already be gone, that's fine
                unsafe { libc::kill(pid as i32, libc::SIGKILL); }
            }
        }))
    } else {
        None
    };

    // 7. Wait for child to exit
    let exit_code = match child.wait().await {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            let _ = send_result(&tx, 1, Some(format!("wait error: {e}"))).await;
            return;
        }
    };

    // Cancel timeout if it hasn't fired
    if let Some(t) = timeout_task {
        t.abort();
    }

    // Wait for IO tasks to drain
    let _ = stdout_task.await;
    let _ = stderr_task.await;
    stdin_task.abort();

    let _ = send_result(&tx, exit_code, None).await;
}

async fn send_result(
    tx: &mpsc::Sender<Result<ExecOutput, tonic::Status>>,
    exit_code: i32,
    error: Option<String>,
) -> Result<(), mpsc::error::SendError<Result<ExecOutput, tonic::Status>>> {
    tx.send(Ok(ExecOutput {
        output: Some(exec_output::Output::Result(ExecResult { exit_code, error })),
    }))
    .await
}
