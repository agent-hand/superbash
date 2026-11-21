//! command の実行（設計 4 章）。
//!
//! `npx --yes --package=<tarball> -- <wrapper の bin> <name> <args の JSON>` を shell を通さず起動し、
//! foreground は時間の上限で止める。background は出力を file に書き、実行の id と path を返す（上限は掛けない）。

use std::fs::{self, File};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::generate::WRAPPER_BIN;
use crate::npm::Npm;

/// 時間の上限の既定（全 command 共通）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// 呼ぶ時の timeout（ミリ秒。省略時は既定）。
pub fn resolve_timeout(timeout_ms: Option<u64>) -> Duration {
    timeout_ms.map_or(DEFAULT_TIMEOUT, Duration::from_millis)
}

/// 止めた後に、読み取りの task が残りの出力を渡すのを待つ上限。
const DRAIN_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    /// shell と同じく、signal で終わった時は 128 + signal
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct Background {
    pub id: String,
    /// stdout と stderr を合わせた出力
    pub output: PathBuf,
    /// 状態（running / exited / failed）と終了 code の JSON
    pub status: PathBuf,
}

pub struct Runner {
    npm: Npm,
    tarball: PathBuf,
    runs_dir: PathBuf,
    seq: AtomicU64,
}

/// drop した時に、process group ごと止める（要求の取り消しでも残さない）。
struct GroupKill(Option<u32>);

impl GroupKill {
    fn new(child: &Child) -> Self {
        GroupKill(child.id())
    }

    fn kill(&self) {
        if let Some(pid) = self.0 {
            // SAFETY: killpg は引数の整数だけを使う。対象は自分が作った process group。
            unsafe {
                libc::killpg(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }

    fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        self.kill();
    }
}

fn exit_code(status: ExitStatus) -> Option<i32> {
    status.code().or_else(|| status.signal().map(|s| 128 + s))
}

impl Runner {
    pub fn new(npm: Npm, tarball: PathBuf, runs_dir: PathBuf) -> Self {
        Runner {
            npm,
            tarball,
            runs_dir,
            seq: AtomicU64::new(0),
        }
    }

    fn command(&self, name: &str, args_json: &str) -> Command {
        let mut c = self.npm.npx();
        c.arg("--yes")
            .arg(format!("--package={}", self.tarball.display()))
            .arg("--")
            .arg(WRAPPER_BIN)
            .arg(name)
            .arg(args_json);
        let mut c = Command::from(c);
        c.stdin(Stdio::null()).process_group(0);
        c
    }

    /// 完了するか時間切れまで待ち、その時までの出力を返す。
    pub async fn run(&self, name: &str, args_json: &str, timeout: Duration) -> Result<Outcome> {
        let mut cmd = self.command(name, args_json);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().context("npx を起動できない")?;
        let guard = GroupKill::new(&child);

        let out = Arc::new(Mutex::new(Vec::new()));
        let err = Arc::new(Mutex::new(Vec::new()));
        let t_out = tokio::spawn(pump(child.stdout.take().unwrap(), out.clone()));
        let t_err = tokio::spawn(pump(child.stderr.take().unwrap(), err.clone()));

        let waited = tokio::time::timeout(timeout, child.wait()).await;
        let (exit, timed_out) = match waited {
            Ok(status) => (exit_code(status?), false),
            Err(_) => {
                guard.kill();
                let _ = child.wait().await;
                (None, true)
            }
        };
        guard.disarm();
        // 孫が pipe を握ったままでも、待ちは上限で切る。
        let _ = tokio::time::timeout(DRAIN_WAIT, async {
            let _ = t_out.await;
            let _ = t_err.await;
        })
        .await;
        let text =
            |b: &Arc<Mutex<Vec<u8>>>| String::from_utf8_lossy(&b.lock().unwrap()).into_owned();
        Ok(Outcome {
            stdout: text(&out),
            stderr: text(&err),
            exit_code: exit,
            timed_out,
        })
    }

    /// background で起動する。出力は file に書く。
    pub async fn start_background(&self, name: &str, args_json: &str) -> Result<Background> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let id = format!(
            "bg-{millis}-{}-{}",
            std::process::id(),
            self.seq.fetch_add(1, Ordering::Relaxed)
        );
        let dir = self.runs_dir.join(&id);
        fs::create_dir_all(&dir).with_context(|| format!("{} を作れない", dir.display()))?;
        let bg = Background {
            id: id.clone(),
            output: dir.join("output"),
            status: dir.join("status.json"),
        };
        let file = File::create(&bg.output)?;

        let mut cmd = self.command(name, args_json);
        cmd.stdout(Stdio::from(file.try_clone()?))
            .stderr(Stdio::from(file));
        let mut child = cmd.spawn().context("npx を起動できない")?;
        let guard = GroupKill::new(&child);
        write_status(&bg.status, &id, "running", None)?;

        let status_path = bg.status.clone();
        tokio::spawn(async move {
            let (state, code) = match child.wait().await {
                Ok(s) => ("exited", exit_code(s)),
                Err(_) => ("failed", None),
            };
            guard.disarm();
            let _ = write_status(&status_path, &id, state, code);
        });
        Ok(bg)
    }
}

async fn pump<R: tokio::io::AsyncRead + Unpin>(mut r: R, into: Arc<Mutex<Vec<u8>>>) {
    let mut buf = [0u8; 8192];
    while let Ok(n) = r.read(&mut buf).await {
        if n == 0 {
            break;
        }
        into.lock().unwrap().extend_from_slice(&buf[..n]);
    }
}

/// 途中の状態を読まれても壊れないよう、一時 file に書いてから置き換える。
fn write_status(path: &Path, id: &str, state: &str, code: Option<i32>) -> Result<()> {
    let body = json!({
        "id": id,
        "state": state,
        "exit_code": code,
    });
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&body)? + "\n")?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_ten_minutes() {
        assert_eq!(resolve_timeout(None), Duration::from_secs(600));
        assert_eq!(resolve_timeout(Some(1500)), Duration::from_millis(1500));
    }
}
