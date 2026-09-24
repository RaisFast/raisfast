//! Out-of-process execution for self-contained, CPU/memory-heavy pure
//! functions.
//!
//! The worker runner can only cancel a job cooperatively (drop its future at an
//! await point); synchronous CPU work inside a `spawn_blocking` cannot be
//! stopped. Running such work in a child process makes it hard-killable (admin
//! cancel, hard timeout via `kill_on_drop`) and isolates its memory.
//!
//! The child is the same binary invoked as the hidden `compute` subcommand,
//! which dispatches `kind` → a pure function (see [`execute`]). The parent
//! writes raw input bytes + JSON params to temp files and reads a JSON
//! envelope back.
//!
//! Tests and unusually-named binaries fall back to in-process execution via
//! [`subprocess_supported`].

use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;

use serde::de::DeserializeOwned;
use tokio::process::Command;

/// Failure of an out-of-process compute call.
#[derive(Debug)]
pub enum ComputeError {
    /// The job was cancelled while the child was running (child killed).
    Cancelled,
    /// The function ran but rejected the input (a real/domain error — the
    /// caller must not retry in-process).
    Rejected(String),
    /// The subprocess could not be used (spawn/protocol/unknown-kind failure) —
    /// safe for the caller to retry in-process.
    Unavailable(String),
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rejected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fatal: Option<String>,
}

/// Dispatches a `kind` to its pure function. Runs in the CHILD process (called
/// by the `compute` CLI subcommand); `input` is the raw payload and `params` is
/// kind-specific JSON.
pub fn execute(
    kind: &str,
    input: &[u8],
    params: &serde_json::Value,
) -> Result<serde_json::Value, ComputeError> {
    match kind {
        "parse_builtin" => {
            #[derive(serde::Deserialize)]
            struct Params {
                mime: String,
                filename: String,
                #[serde(default)]
                extract_images: bool,
            }
            let p: Params = serde_json::from_value(params.clone())
                .map_err(|e| ComputeError::Unavailable(format!("parse_builtin params: {e}")))?;
            let outcome = crate::docparse::engines::builtin::parse_builtin(
                input,
                &p.mime,
                &p.filename,
                &crate::docparse::ParseOpts {
                    extract_images: p.extract_images,
                },
            )
            .map_err(|e| ComputeError::Rejected(e.to_string()))?;
            serde_json::to_value(outcome)
                .map_err(|e| ComputeError::Unavailable(format!("encode outcome: {e}")))
        }
        "chunk_markdown" => {
            let cfg: crate::kb::chunker::ChunkerConfig = serde_json::from_value(params.clone())
                .map_err(|e| ComputeError::Unavailable(format!("chunk_markdown params: {e}")))?;
            let md = std::str::from_utf8(input)
                .map_err(|e| ComputeError::Rejected(format!("markdown not utf-8: {e}")))?;
            let chunks = crate::kb::chunker::chunk_markdown(md, &cfg);
            serde_json::to_value(chunks)
                .map_err(|e| ComputeError::Unavailable(format!("encode chunks: {e}")))
        }
        other => Err(ComputeError::Unavailable(format!(
            "unknown compute kind '{other}'"
        ))),
    }
}

/// Whether the running executable is the real `raisfast` binary (which
/// implements the `compute` subcommand). Test harnesses are named
/// `raisfast-<hash>`, so they take the in-process path.
#[must_use]
pub fn subprocess_supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .is_some_and(|stem| stem == "raisfast")
    })
}

/// Runs `kind` in a child process and decodes its typed result. `kill_on_drop`
/// ensures the child dies if this future is dropped (hard timeout); the job's
/// cancel signal (task-local [`crate::cancellation::JOB_CANCEL`]) kills it on
/// cancel.
pub async fn run<T: DeserializeOwned>(
    kind: &str,
    input: &[u8],
    params: &serde_json::Value,
) -> Result<T, ComputeError> {
    let exe = std::env::current_exe()
        .map_err(|e| ComputeError::Unavailable(format!("current_exe: {e}")))?;
    let dir =
        TempWorkspace::new().map_err(|e| ComputeError::Unavailable(format!("tempdir: {e}")))?;
    let input_path = dir.path().join("in.bin");
    let params_path = dir.path().join("params.json");
    let output_path = dir.path().join("out.json");

    tokio::fs::write(&input_path, input)
        .await
        .map_err(|e| ComputeError::Unavailable(format!("write input: {e}")))?;
    let params_bytes = serde_json::to_vec(params)
        .map_err(|e| ComputeError::Unavailable(format!("encode params: {e}")))?;
    tokio::fs::write(&params_path, params_bytes)
        .await
        .map_err(|e| ComputeError::Unavailable(format!("write params: {e}")))?;

    let mut cmd = Command::new(&exe);
    cmd.arg("compute")
        .arg("--kind")
        .arg(kind)
        .arg("--input")
        .arg(&input_path)
        .arg("--params")
        .arg(&params_path)
        .arg("--output")
        .arg(&output_path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| ComputeError::Unavailable(format!("spawn: {e}")))?;

    match crate::cancellation::JOB_CANCEL.try_with(|rx| rx.clone()) {
        Ok(mut rx) => {
            tokio::select! {
                status = child.wait() => {
                    status.map_err(|e| ComputeError::Unavailable(format!("wait: {e}")))?;
                }
                _ = rx.changed() => {
                    let _ = child.kill().await;
                    return Err(ComputeError::Cancelled);
                }
            }
        }
        Err(_) => {
            child
                .wait()
                .await
                .map_err(|e| ComputeError::Unavailable(format!("wait: {e}")))?;
        }
    }

    let data = tokio::fs::read(&output_path).await.map_err(|e| {
        ComputeError::Unavailable(format!("read output (child produced none): {e}"))
    })?;
    let env: Envelope = serde_json::from_slice(&data)
        .map_err(|e| ComputeError::Unavailable(format!("decode envelope: {e}")))?;
    if let Some(msg) = env.fatal {
        return Err(ComputeError::Unavailable(msg));
    }
    if let Some(msg) = env.rejected {
        return Err(ComputeError::Rejected(msg));
    }
    let value = env
        .result
        .ok_or_else(|| ComputeError::Unavailable("empty envelope".into()))?;
    serde_json::from_value(value)
        .map_err(|e| ComputeError::Unavailable(format!("decode result: {e}")))
}

/// Temp directory for subprocess I/O, removed on drop (also when the future is
/// dropped by a timeout/cancel).
struct TempWorkspace {
    dir: std::path::PathBuf,
}

impl TempWorkspace {
    fn new() -> std::io::Result<Self> {
        let dir =
            std::env::temp_dir().join(format!("raisfast-compute-{}", crate::utils::id::new_id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Child-side entry point used by the `compute` CLI subcommand: reads input +
/// params, runs [`execute`], writes the JSON envelope, always exits 0.
pub fn child_main(kind: &str, input: &str, params: &str, output: &str) -> anyhow::Result<()> {
    let env = match std::fs::read(input) {
        Err(e) => Envelope {
            ok: false,
            result: None,
            rejected: None,
            fatal: Some(format!("read input: {e}")),
        },
        Ok(bytes) => match std::fs::read_to_string(params)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            None => Envelope {
                ok: false,
                result: None,
                rejected: None,
                fatal: Some("read/parse params".into()),
            },
            Some(params) => match execute(kind, &bytes, &params) {
                Ok(v) => Envelope {
                    ok: true,
                    result: Some(v),
                    rejected: None,
                    fatal: None,
                },
                Err(ComputeError::Rejected(m)) => Envelope {
                    ok: false,
                    result: None,
                    rejected: Some(m),
                    fatal: None,
                },
                Err(ComputeError::Unavailable(m)) => Envelope {
                    ok: false,
                    result: None,
                    rejected: None,
                    fatal: Some(m),
                },
                Err(ComputeError::Cancelled) => Envelope {
                    ok: false,
                    result: None,
                    rejected: None,
                    fatal: Some("cancelled".into()),
                },
            },
        },
    };
    match serde_json::to_vec(&env) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(output, bytes) {
                eprintln!("compute: write output: {e}");
            }
        }
        Err(e) => eprintln!("compute: encode envelope: {e}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_kind_is_unavailable() {
        let r = execute("nope", b"", &serde_json::json!({}));
        assert!(matches!(r, Err(ComputeError::Unavailable(_))));
    }

    #[test]
    fn chunk_markdown_roundtrips() {
        let cfg = crate::kb::chunker::ChunkerConfig::default();
        let params = serde_json::to_value(cfg).unwrap();
        let md = "# Title\n\nHello world. This is a paragraph of text.";
        let value = execute("chunk_markdown", md.as_bytes(), &params).unwrap();
        let chunks: Vec<crate::kb::chunker::Chunk> = serde_json::from_value(value).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn parse_builtin_markdown_roundtrips() {
        let params = serde_json::json!({
            "mime": "text/markdown",
            "filename": "a.md",
            "extract_images": false,
        });
        let value = execute("parse_builtin", b"# Hi\n\nhello", &params).unwrap();
        let out: crate::docparse::ParseOutcome = serde_json::from_value(value).unwrap();
        assert!(out.markdown.contains("hello"));
    }
}
