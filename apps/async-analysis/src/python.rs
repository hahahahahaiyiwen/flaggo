use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use flaggo_analysis_agent::{AnalysisRunStatus, YieldSignal};
use flaggo_analysis_domain::AnalysisError;
use serde::Serialize;
use tokio::{
    io::AsyncReadExt,
    process::Command,
    time::{MissedTickBehavior, interval, sleep_until},
};

#[derive(Clone, Debug)]
pub struct PythonConfig {
    pub executable: PathBuf,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for PythonConfig {
    fn default() -> Self {
        let environment = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".venv");
        Self {
            executable: if cfg!(windows) {
                environment.join("Scripts").join("python.exe")
            } else {
                environment.join("bin").join("python")
            },
            timeout: Duration::from_secs(60),
            max_output_bytes: 16_384,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStatus {
    Succeeded,
    Failed,
    LaunchFailed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonExecution {
    pub status: ExecutionStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub output_complete: bool,
    pub diagnostic: Option<String>,
}

const BOOTSTRAP: &str = r#"
import json, platform, runpy, sys
script, runtime_file, source_directory = sys.argv[1:]
with open(runtime_file, "w", encoding="utf-8") as stream:
    json.dump({"executable": sys.executable, "version": sys.version,
               "implementation": platform.python_implementation()}, stream)
sys.path.insert(0, source_directory)
sys.argv = [script]
runpy.run_path(script, run_name="__main__")
"#;

impl PythonConfig {
    /// Runs a saved script locally. This process inherits host access and environment.
    pub async fn run(
        &self,
        directory: &Path,
        script: &Path,
        runtime_file: &Path,
        source_directory: &Path,
        signal: &YieldSignal,
    ) -> Result<PythonExecution, AnalysisError> {
        if self.timeout.is_zero() || self.max_output_bytes == 0 {
            return Err(AnalysisError::Agent(
                "Python timeout and captured-output limit must be positive".to_owned(),
            ));
        }
        if signal.status()? != AnalysisRunStatus::Active {
            return Err(AnalysisError::Agent(
                "Python execution requested after yield".to_owned(),
            ));
        }
        let started = Instant::now();
        let mut child = match Command::new(&self.executable)
            .args(["-u", "-c", BOOTSTRAP])
            .arg(script)
            .arg(runtime_file)
            .arg(source_directory)
            .current_dir(directory)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                return Ok(PythonExecution {
                    status: ExecutionStatus::LaunchFailed,
                    exit_code: None,
                    duration_ms: elapsed_ms(started)?,
                    stdout: String::new(),
                    stderr: String::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    output_complete: true,
                    diagnostic: Some(error.to_string()),
                });
            }
        };
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| runner_error("missing stdout pipe"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| runner_error("missing stderr pipe"))?;
        let mut out = Capture::default();
        let mut err = Capture::default();
        let mut out_buffer = [0u8; 8192];
        let mut err_buffer = [0u8; 8192];
        let mut outcome = None;
        let mut exit_code = None;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut drain_deadline = deadline;
        let mut control = interval(Duration::from_millis(50));
        control.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            if outcome.is_some() && out.complete && err.complete {
                break;
            }
            tokio::select! {
                status = child.wait(), if outcome.is_none() => {
                    let status = status.map_err(runner_error)?;
                    exit_code = status.code();
                    outcome = Some(if status.success() { ExecutionStatus::Succeeded } else { ExecutionStatus::Failed });
                    drain_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                }
                read = stdout.read(&mut out_buffer), if !out.complete => {
                    out.accept(&out_buffer, read.map_err(runner_error)?, self.max_output_bytes);
                }
                read = stderr.read(&mut err_buffer), if !err.complete => {
                    err.accept(&err_buffer, read.map_err(runner_error)?, self.max_output_bytes);
                }
                _ = sleep_until(deadline), if outcome.is_none() => {
                    child.kill().await.map_err(runner_error)?;
                    outcome = Some(ExecutionStatus::TimedOut);
                    drain_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                }
                _ = control.tick(), if outcome.is_none() => {
                    if signal.status()? != AnalysisRunStatus::Active {
                        child.kill().await.map_err(runner_error)?;
                        outcome = Some(ExecutionStatus::Cancelled);
                        drain_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                    }
                }
                _ = sleep_until(drain_deadline), if outcome.is_some() => break,
            }
        }
        Ok(PythonExecution {
            status: outcome.ok_or_else(|| runner_error("Python process has no terminal status"))?,
            exit_code,
            duration_ms: elapsed_ms(started)?,
            stdout: String::from_utf8_lossy(&out.bytes).into_owned(),
            stderr: String::from_utf8_lossy(&err.bytes).into_owned(),
            stdout_truncated: out.truncated || !out.complete,
            stderr_truncated: err.truncated || !err.complete,
            output_complete: out.complete && err.complete,
            diagnostic: None,
        })
    }
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
    complete: bool,
}

impl Capture {
    fn accept(&mut self, buffer: &[u8], count: usize, limit: usize) {
        if count == 0 {
            self.complete = true;
            return;
        }
        let remaining = limit.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&buffer[..count.min(remaining)]);
        self.truncated |= count > remaining;
    }
}

fn elapsed_ms(started: Instant) -> Result<u64, AnalysisError> {
    u64::try_from(started.elapsed().as_millis()).map_err(runner_error)
}

fn runner_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Agent(format!("local Python execution: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn execute(
        code: &str,
        config: PythonConfig,
        signal: Arc<YieldSignal>,
    ) -> PythonExecution {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tokio::fs::write(root.join("script.py"), code)
            .await
            .unwrap();
        config
            .run(
                root,
                &root.join("script.py"),
                &root.join("runtime.json"),
                root,
                &signal,
            )
            .await
            .unwrap()
    }

    use std::sync::Arc;

    #[tokio::test]
    async fn runs_real_python_and_reports_runtime_and_failures() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tokio::fs::write(root.join("helper.py"), "value = 7\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("script.py"), "from helper import value\nfrom pathlib import Path\nPath('output.txt').write_text(str(value))\nprint(value)\n").await.unwrap();
        let result = PythonConfig::default()
            .run(
                root,
                &root.join("script.py"),
                &root.join("runtime.json"),
                root,
                &YieldSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "7");
        assert_eq!(
            tokio::fs::read_to_string(root.join("output.txt"))
                .await
                .unwrap(),
            "7"
        );
        let runtime: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(root.join("runtime.json")).await.unwrap())
                .unwrap();
        assert!(runtime["version"].as_str().unwrap().starts_with('3'));
        assert!(!runtime["executable"].as_str().unwrap().is_empty());
        for code in [
            "raise ValueError('expected error')",
            "this is invalid Python !",
        ] {
            let failed = execute(
                code,
                PythonConfig::default(),
                Arc::new(YieldSignal::default()),
            )
            .await;
            assert_eq!(failed.status, ExecutionStatus::Failed);
            assert_ne!(failed.exit_code, Some(0));
            assert!(!failed.stderr.is_empty());
        }
    }

    #[tokio::test]
    async fn bounds_output_without_blocking_process_pipes() {
        let config = PythonConfig {
            max_output_bytes: 32,
            ..PythonConfig::default()
        };
        let result = execute(
            "import sys\nprint('x' * 200000)\nprint('y' * 200000, file=sys.stderr)",
            config,
            Arc::new(YieldSignal::default()),
        )
        .await;
        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(result.stdout.len(), 32);
        assert_eq!(result.stderr.len(), 32);
        assert!(result.stdout_truncated && result.stderr_truncated);
        assert!(result.output_complete);
    }

    #[tokio::test]
    async fn reports_launch_failure_timeout_and_shutdown_cancellation() {
        let missing = PythonConfig {
            executable: PathBuf::from("flaggo-missing-python-executable"),
            ..PythonConfig::default()
        };
        let result = execute("print(1)", missing, Arc::new(YieldSignal::default())).await;
        assert_eq!(result.status, ExecutionStatus::LaunchFailed);
        assert!(result.diagnostic.is_some());
        let timeout = PythonConfig {
            timeout: Duration::from_millis(150),
            ..PythonConfig::default()
        };
        let result = execute(
            "import time\ntime.sleep(20)",
            timeout,
            Arc::new(YieldSignal::default()),
        )
        .await;
        assert_eq!(result.status, ExecutionStatus::TimedOut);
        assert!(result.duration_ms < 3000);
        let signal = Arc::new(YieldSignal::default());
        let cancel = signal.clone();
        let (result, ()) = tokio::join!(
            execute(
                "import time\ntime.sleep(20)",
                PythonConfig::default(),
                signal
            ),
            async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                cancel.request_shutdown().unwrap();
            }
        );
        assert_eq!(result.status, ExecutionStatus::Cancelled);
        assert!(result.duration_ms < 3000);
    }
}
