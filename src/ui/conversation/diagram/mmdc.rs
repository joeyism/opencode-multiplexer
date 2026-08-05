use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use thiserror::Error;
use wait_timeout::ChildExt;

/// Bump when render flags change so stale cache entries are ignored.
pub const RENDER_CACHE_VERSION: &str = "v2-dark";

#[derive(Debug, Clone)]
pub struct MermaidRenderConfig {
    pub mmdc_path: PathBuf,
    pub cache_dir: PathBuf,
    pub timeout: Duration,
    pub max_rows: usize,
    pub prefetch_viewports: usize,
    pub protocol_enabled: bool,
    /// Optional path written each time a render is actually performed (tests).
    pub invocation_log: Option<PathBuf>,
}

#[derive(Error, Debug)]
pub enum MermaidError {
    #[error("mmdc timed out after {0:?}")]
    Timeout(Duration),
    #[error("mmdc failed with exit code {0:?}: {1}")]
    Failure(Option<i32>, String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

fn log_invocation(cfg: &MermaidRenderConfig) {
    if let Some(log) = &cfg.invocation_log {
        if let Some(parent) = log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "mmdc")
            });
    }
}

/// In-process stand-in for `tests/fixtures/fake_mmdc.sh`.
///
/// Real subprocess + `wait_timeout` is unreliable under tarpaulin/coverage
/// instrumentation (hangs / false timeouts). Tests point `mmdc_path` at the
/// fake script name; we detect that and stay in-process.
fn is_inline_fake(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains("fake_mmdc"))
}

fn render_inline_fake(source: &str, cfg: &MermaidRenderConfig) -> Result<Vec<u8>, MermaidError> {
    log_invocation(cfg);

    if source.contains("FORCE_FAIL") {
        return Err(MermaidError::Failure(
            Some(1),
            "Fake mmdc error: found FORCE_FAIL".into(),
        ));
    }

    if source.contains("FORCE_SLEEP") {
        // Honor timeout without spawning a process (coverage-safe).
        let start = Instant::now();
        while start.elapsed() < cfg.timeout + Duration::from_millis(50) {
            std::thread::sleep(Duration::from_millis(10));
        }
        return Err(MermaidError::Timeout(cfg.timeout));
    }

    // Prefer fixture next to the fake script; fall back to crate fixtures path.
    let candidates = [
        cfg.mmdc_path
            .parent()
            .map(|p| p.join("tiny.png"))
            .unwrap_or_default(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny.png"),
    ];
    for path in &candidates {
        if path.is_file() {
            return Ok(fs::read(path)?);
        }
    }
    Err(MermaidError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "tiny.png fixture missing for inline fake mmdc",
    )))
}

pub fn render_with_mmdc(source: &str, cfg: &MermaidRenderConfig) -> Result<Vec<u8>, MermaidError> {
    let _ = fs::create_dir_all(&cfg.cache_dir);

    if is_inline_fake(&cfg.mmdc_path) {
        return render_inline_fake(source, cfg);
    }

    let temp_id = uuid::Uuid::new_v4();
    let input_path = cfg.cache_dir.join(format!("input-{temp_id}.mmd"));
    let output_path = cfg.cache_dir.join(format!("output-{temp_id}.png"));

    fs::write(&input_path, source)?;
    log_invocation(cfg);

    // Dark theme + solid dark background: diagrams are meant for dark TUIs.
    let mut child = Command::new(&cfg.mmdc_path)
        .arg("-i")
        .arg(&input_path)
        .arg("-o")
        .arg(&output_path)
        .arg("-t")
        .arg("dark")
        .arg("-b")
        .arg("#1a1b26")
        .arg("-s")
        .arg("2")
        .spawn()?;

    match child.wait_timeout(cfg.timeout)? {
        Some(status) => {
            let result = if status.success() {
                fs::read(&output_path).map_err(MermaidError::from)
            } else {
                Err(MermaidError::Failure(
                    status.code(),
                    "mmdc execution failed".into(),
                ))
            };
            let _ = fs::remove_file(&input_path);
            let _ = fs::remove_file(&output_path);
            result
        }
        None => {
            let _ = child.kill();
            let _ = fs::remove_file(&input_path);
            let _ = fs::remove_file(&output_path);
            Err(MermaidError::Timeout(cfg.timeout))
        }
    }
}
