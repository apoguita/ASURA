//! The control channel: a line on loopback, answered by the supervisor.
//!
//! Commands: `status`, `stop <service>`, `stop` (everything, then exit), `restart <service>`,
//! `quit`. The channel is loopback-only and is how the CLI stops or inspects a supervisor it did not
//! start (a different shell, a script).

use std::io;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, Instant};

use crate::supervise::{self, Shared};

/// Answer commands until `quit` (or until the process ends).
pub async fn serve(shared: Shared, listener: TcpListener) {
    loop {
        if shared.lock().unwrap().quit.load(Ordering::SeqCst) {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(200), listener.accept()).await {
            Ok(Ok((stream, _peer))) => {
                let _ = handle(shared.clone(), stream).await;
            }
            Ok(Err(err)) => {
                eprintln!("control: accept failed: {err}");
                sleep(Duration::from_millis(200)).await;
            }
            Err(_) => {}
        }
    }
}

async fn handle(shared: Shared, mut stream: TcpStream) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, stream.read(&mut byte)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(_)) => {
                if byte[0] == b'\n' {
                    break;
                }
                if byte[0] != b'\r' {
                    line.push(byte[0]);
                }
                if line.len() > 4096 {
                    break;
                }
            }
            _ => break,
        }
    }
    let command = String::from_utf8_lossy(&line).trim().to_string();
    let reply = act(&shared, &command).await;
    stream.write_all(reply.as_bytes()).await?;
    stream.flush().await
}

async fn act(shared: &Shared, line: &str) -> String {
    let mut parts = line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("status"), None) => crate::status::table(shared),
        (Some("stop"), Some(name)) => {
            match supervise::stop_service(shared, name, Duration::from_secs(10)).await {
                Ok(()) => "ok\n".into(),
                Err(err) => format!("err: {err}\n"),
            }
        }
        (Some("stop"), None) => {
            let _ = supervise::stop_all(shared, Duration::from_secs(15)).await;
            shared.lock().unwrap().quit.store(true, Ordering::SeqCst);
            "ok\n".into()
        }
        (Some("restart"), Some(name)) => {
            match supervise::restart_service(shared, name, Duration::from_secs(10)).await {
                Ok(()) => "ok\n".into(),
                Err(err) => format!("err: {err}\n"),
            }
        }
        (Some("quit"), None) => {
            let _ = supervise::stop_all(shared, Duration::from_secs(15)).await;
            shared.lock().unwrap().quit.store(true, Ordering::SeqCst);
            "ok\n".into()
        }
        _ => "err: expected: status | stop [service] | restart service | quit\n".into(),
    }
}

/// Ask a running supervisor one question. Returns its reply, or a message saying why it could not
/// be asked.
pub async fn request(control: &str, line: &str) -> Result<String, String> {
    let address: std::net::SocketAddr = control
        .parse()
        .map_err(|err| format!("control address {control:?}: {err}"))?;
    let mut stream =
        match tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(address)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(err)) => return Err(format!("no supervisor is answering on {address} ({err})")),
            Err(_) => return Err(format!("no supervisor is answering on {address}")),
        };
    stream
        .write_all(format!("{line}\n").as_bytes())
        .await
        .map_err(|err| err.to_string())?;
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(60), stream.read_to_end(&mut reply))
        .await
        .map_err(|_| "the supervisor did not answer in time".to_string())?
        .map_err(|err| err.to_string())?;
    Ok(String::from_utf8_lossy(&reply).trim_end().to_string())
}
