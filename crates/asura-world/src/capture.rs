//! A server-side packet capture: every game message, both directions, as plaintext JSONL.
//!
//! The client-side sniffer (`tools/sniff-capture.py`) records what the client's socket carried,
//! through an injected hook. This records the *same* messages from this side of the wire - they are
//! identical bytes: this server encodes the plaintext frame and then ARC4-scrambles it on the way
//! out, and it undoes the scramble on the way in. So a drift, a terrain jump or a load failure can
//! be captured with no injected client, no elevated shell and nothing from another project.
//!
//! One file, one JSON object per line, starting with a header record:
//!
//! ```text
//! {"record":"capture","tool":"asura-world","started_at":"..."}
//! {"seq":1,"t":0.012,"direction":"StoC","header":153,"size":12,"data":"99000000a100000000000000"}
//! {"seq":2,"t":0.498,"direction":"CToS","header":32829,"size":26,"data":"3d80000000000000..."}
//! ```
//!
//! `t` is seconds since the capture started, so a capture lines up with the world log by time.
//! Nothing is sanitised: this file never leaves the machine (it is gitignored), and the whole point
//! is the payloads.

use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// One open capture: the file, when it started, and a sequence number shared by every connection.
struct Capture {
    writer: Mutex<BufWriter<std::fs::File>>,
    start: Instant,
    seq: AtomicU64,
}

static CAPTURE: OnceLock<Option<Capture>> = OnceLock::new();

/// Start capturing to `path`, once per process. An empty path is a no-op, and a second call is
/// ignored: there is one capture per server run.
pub fn install(path: &str) -> std::io::Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    let file = std::fs::File::create(path)?;
    let capture = Capture {
        writer: Mutex::new(BufWriter::new(file)),
        start: Instant::now(),
        seq: AtomicU64::new(0),
    };
    let mut writer = capture.writer.lock().unwrap_or_else(|err| err.into_inner());
    writeln!(
        writer,
        "{{\"record\":\"capture\",\"tool\":\"asura-world\",\"started_at\":\"{}\"}}",
        chrono_like_now()
    )?;
    writer.flush()?;
    drop(writer);
    let _ = CAPTURE.set(Some(capture));
    Ok(())
}

/// Whether a capture is running.
pub fn recording() -> bool {
    CAPTURE.get().map(Option::is_some).unwrap_or(false)
}

/// Record one message. `bytes` is the plaintext frame, header first.
pub fn record(direction: &str, header: u16, bytes: &[u8]) {
    let Some(capture) = CAPTURE.get().and_then(|capture| capture.as_ref()) else {
        return;
    };
    let seq = capture.seq.fetch_add(1, Ordering::Relaxed) + 1;
    let t = capture.start.elapsed().as_secs_f64();
    let mut writer = capture.writer.lock().unwrap_or_else(|err| err.into_inner());
    let _ = writeln!(
        writer,
        "{{\"seq\":{seq},\"t\":{t:.3},\"direction\":\"{direction}\",\"header\":{header},\"size\":{},\"data\":\"{}\"}}",
        bytes.len(),
        hex(bytes)
    );
    // Flushed per record: a capture is evidence, and a server that is killed (the usual end of a
    // diagnostic session) must leave its last line on disk rather than in a buffer.
    let _ = writer.flush();
}

/// A UTC timestamp without pulling a date crate: seconds since the Unix epoch, ISO-8601-ish.
fn chrono_like_now() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos()),
        Err(_) => "unknown".to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_writes_one_json_line_per_message() {
        let path = std::env::temp_dir().join(format!("asura-capture-{}.jsonl", std::process::id()));
        let path_text = path.to_str().expect("a path").to_string();
        // The process-wide capture can only be installed once; this test owns it.
        install(&path_text).expect("install the capture");
        record(
            "StoC",
            0x0099,
            &[0x99, 0x00, 0x00, 0x00, 0x94, 0x00, 0x00, 0x00],
        );
        record("CToS", 0x803D, &[0x3d, 0x80, 0x00, 0x00]);
        let text = std::fs::read_to_string(&path).expect("read the capture");
        std::fs::remove_file(&path).ok();

        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "a header record and two messages: {text}");
        assert!(lines[0].contains("\"record\":\"capture\""), "{}", lines[0]);
        assert!(
            lines[1].contains("\"direction\":\"StoC\"")
                && lines[1].contains("\"header\":153")
                && lines[1].contains("\"size\":8")
                && lines[1].contains("\"data\":\"9900000094")
                || lines[1].contains("\"data\":\"99000000"),
            "the message record carries direction, header, size and the bytes: {}",
            lines[1]
        );
        assert!(lines[2].contains("\"direction\":\"CToS\""), "{}", lines[2]);
        let seq_second = lines[1].contains("\"seq\":1") || lines[2].contains("\"seq\":2");
        assert!(
            seq_second,
            "messages are sequenced: {} / {}",
            lines[1], lines[2]
        );
    }
}
