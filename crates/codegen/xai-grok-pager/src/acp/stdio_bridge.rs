//! Spawn an external ACP agent over stdio and expose an `AcpClientChannel`.
//!
//! Same JSON-RPC bridge as [`super::leader_bridge`], without reconnect.

use std::path::Path;
use std::process::Stdio;
use std::thread;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, simplex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;

use agent_client_protocol as acp;
use xai_acp_lib::{
    AcpClientChannel, AcpGatewayReceiver, AcpGatewaySender, LineBufferedRead, acp_channels,
};

use super::backend::AgentBackend;

const MAX_BUF: usize = 8 * 1024 * 1024;

pub struct StdioAcpBridge {
    pub channel: AcpClientChannel,
    pub cancel: CancellationToken,
    pub thread_handle: thread::JoinHandle<Result<()>>,
}

pub struct StdioAcpSpawn {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<std::path::PathBuf>,
}

impl StdioAcpSpawn {
    pub fn cursor_agent(bin: &Path, yolo: bool) -> Self {
        let mut args = vec!["--trust".to_string()];
        if yolo {
            args.push("--force".to_string());
        }
        if let Ok(cwd) = std::env::current_dir() {
            args.push("--workspace".to_string());
            args.push(cwd.display().to_string());
        }
        args.push("acp".to_string());
        Self {
            program: bin.to_path_buf(),
            args,
            cwd: std::env::current_dir().ok(),
        }
    }
}

/// Spawn `program args` as a newline-delimited JSON-RPC ACP agent.
pub fn spawn_stdio_acp(
    spawn: StdioAcpSpawn,
    cancel: CancellationToken,
    backend: AgentBackend,
) -> Result<StdioAcpBridge> {
    let (client_channel, agent_channel) = acp_channels();
    let (incoming_read, incoming_write) = simplex(MAX_BUF);
    let (outgoing_read, outgoing_write) = simplex(MAX_BUF);

    let bridge_cancel = cancel.clone();
    let thread_name = format!("pager-{}-acp", backend.as_str());
    let thread_handle = thread::Builder::new().name(thread_name).spawn(move || -> Result<()> {
        let mut builder = tokio::runtime::Builder::new_current_thread();
        let rt = xai_tty_utils::runtime::apply_blocking_pool(builder.enable_all()).build()?;
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let mut cmd = tokio::process::Command::new(&spawn.program);
            cmd.args(&spawn.args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            if let Some(cwd) = spawn.cwd.as_ref() {
                cmd.current_dir(cwd);
            }
            xai_tty_utils::detach_command(&mut cmd);

            let mut child = cmd.spawn().map_err(|e| {
                anyhow::anyhow!(
                    "failed to spawn {} ({}): {e}",
                    backend.display_name(),
                    spawn.program.display()
                )
            })?;
            let mut child_stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow::anyhow!("child stdin missing"))?;
            let child_stdout = child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("child stdout missing"))?;
            if let Some(stderr) = child.stderr.take() {
                let label = backend.as_str();
                tokio::task::spawn_local(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if !line.is_empty() {
                            tracing::debug!(backend = label, "{line}");
                        }
                    }
                });
            }

            let cancel_r = bridge_cancel.clone();
            let reader_task = tokio::task::spawn_local(async move {
                let mut incoming_write = incoming_write;
                let mut lines = BufReader::new(child_stdout).lines();
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel_r.cancelled() => break,
                        line = lines.next_line() => {
                            match line {
                                Ok(Some(json_line)) => {
                                    if incoming_write.write_all(json_line.as_bytes()).await.is_err()
                                        || incoming_write.write_all(b"\n").await.is_err()
                                    {
                                        break;
                                    }
                                }
                                Ok(None) | Err(_) => {
                                    tracing::warn!(
                                        backend = backend.as_str(),
                                        "ACP stdio agent closed stdout"
                                    );
                                    cancel_r.cancel();
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            let cancel_w = bridge_cancel.clone();
            let writer_task = tokio::task::spawn_local(async move {
                let mut reader = BufReader::new(outgoing_read);
                let mut line = String::new();
                loop {
                    line.clear();
                    tokio::select! {
                        biased;
                        _ = cancel_w.cancelled() => break,
                        result = reader.read_line(&mut line) => {
                            match result {
                                Ok(0) => break,
                                Ok(_) => {
                                    let pending = line.trim_end();
                                    if pending.is_empty() {
                                        continue;
                                    }
                                    if child_stdin.write_all(pending.as_bytes()).await.is_err()
                                        || child_stdin.write_all(b"\n").await.is_err()
                                        || child_stdin.flush().await.is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
            });

            let gw_tx = AcpGatewaySender::new(agent_channel.tx).with_tracing(true);
            let incoming = LineBufferedRead::spawn_local(incoming_read.compat());
            let (conn, handle_io) = acp::ClientSideConnection::new(
                gw_tx,
                outgoing_write.compat_write(),
                incoming,
                |fut| {
                    tokio::task::spawn_local(fut);
                },
            );
            let gw_rx = AcpGatewayReceiver::new(agent_channel.rx, conn).with_tracing(true);
            tokio::task::spawn_local(handle_io);
            tokio::task::spawn_local(gw_rx.run());
            tokio::task::yield_now().await;

            tokio::select! {
                _ = bridge_cancel.cancelled() => {}
                status = child.wait() => {
                    tracing::warn!(
                        backend = backend.as_str(),
                        ?status,
                        "ACP stdio agent exited"
                    );
                    bridge_cancel.cancel();
                }
            }
            let _ = child.kill().await;
            reader_task.abort();
            writer_task.abort();
            Ok(())
        })
    })?;

    Ok(StdioAcpBridge {
        channel: client_channel,
        cancel,
        thread_handle,
    })
}
