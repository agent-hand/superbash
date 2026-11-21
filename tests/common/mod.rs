#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::net::SocketAddr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::Value;
use superbash::npm::Npm;

pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/example-tools")
}

/// test ごとの使い捨ての dir（target の下）。home・npm の cache・prefix をここに置く。
pub struct Sandbox {
    pub root: PathBuf,
}

impl Sandbox {
    pub fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("home")).unwrap();
        Sandbox { root }
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("npm-cache")
    }

    pub fn env(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            ("HOME", self.root.join("home")),
            ("npm_config_cache", self.cache()),
            ("npm_config_prefix", self.root.join("prefix")),
            ("npm_config_userconfig", self.root.join("home/.npmrc")),
            ("npm_config_update_notifier", PathBuf::from("false")),
        ]
    }

    /// 環境を引き継がず、使い捨ての cache・prefix に向けた npm。
    pub fn npm(&self) -> Npm {
        Npm {
            clear_env: true,
            envs: self
                .env()
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// `npx --yes --package=<tgz> -- <bin> <args...>` を shell を通さず起動する。
    pub fn npx(&self, tgz: &Path, bin: &str, args: &[&str]) -> std::process::Output {
        let mut c = Command::new("npx");
        c.env_clear().env("PATH", std::env::var_os("PATH").unwrap());
        for (k, v) in self.env() {
            c.env(k, v);
        }
        c.current_dir(&self.root)
            .arg("--yes")
            .arg(format!("--package={}", tgz.display()))
            .arg("--")
            .arg(bin)
            .args(args)
            .output()
            .unwrap()
    }
}

pub fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            fs::copy(e.path(), &to).unwrap(); // 実行の権限も写る
        }
    }
}

pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// fixture から git の bare repo を作り、`git+file://` の指定を返す。
pub fn make_bare_repo(sb: &Sandbox) -> String {
    let work = sb.root.join("repo-work");
    copy_dir(&fixture(), &work);
    git(&work, &["init", "-q"]);
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "fixture"]);
    let bare = sb.root.join("repo.git");
    git(
        &sb.root,
        &[
            "clone",
            "-q",
            "--bare",
            work.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    format!("git+file://{}", bare.display())
}

/// 起動した superbash（HTTP の MCP server）。drop で止める。
pub struct Server {
    child: tokio::process::Child,
    /// 起動の 1 行を読んだ後も stdout を握っておく（閉じると出力が失敗する）
    _stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
    pub addr: SocketAddr,
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    /// SIGTERM で止め、終わるのを待つ（test の中で止める）。
    pub async fn stop(mut self) {
        if let Some(pid) = self.child.id() {
            // SAFETY: 自分が起動した子の pid に SIGTERM を送るだけ。
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
        tokio::time::timeout(Duration::from_secs(30), self.child.wait())
            .await
            .expect("superbash が SIGTERM で 30 秒以内に終わらない")
            .expect("superbash の終了を待てない");
    }
}

/// superbash を MCP の mode（空き port）で起動し、起動の 1 行から address を知る。
pub async fn start_server(sb: &Sandbox, toolset: impl AsRef<OsStr>, allow: Option<&str>) -> Server {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_superbash"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap());
    for (k, v) in sb.env() {
        cmd.env(k, v);
    }
    cmd.current_dir(&sb.root)
        .args(["--host", "127.0.0.1", "--port", "0"]);
    if let Some(a) = allow {
        cmd.arg("--allow-cmd").arg(a);
    }
    cmd.arg(toolset)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().expect("superbash を起動できない");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let n = tokio::time::timeout(Duration::from_secs(120), stdout.read_line(&mut line))
        .await
        .expect("起動の 1 行が 120 秒で出ない")
        .unwrap();
    assert!(n > 0, "起動の 1 行が出ずに終了した");
    let addr = line
        .trim()
        .strip_prefix("superbash: listening on http://")
        .and_then(|r| r.strip_suffix("/mcp"))
        .unwrap_or_else(|| panic!("起動の 1 行が想定と違う: {line:?}"))
        .parse()
        .unwrap();
    Server {
        child,
        _stdout: stdout,
        addr,
    }
}

type Inner = RunningService<RoleClient, ()>;

/// 起動した superbash と、それに HTTP で繋いだ MCP client。
pub struct Client {
    inner: Inner,
    server: Server,
}

impl Deref for Client {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.inner
    }
}

impl Client {
    /// client を閉じ、server を止める。
    pub async fn cancel(self) -> Result<(), String> {
        let r = self
            .inner
            .cancel()
            .await
            .map(|_| ())
            .map_err(|e| e.to_string());
        self.server.stop().await;
        r
    }
}

/// superbash を MCP の mode で起動し、Streamable HTTP の MCP client として繋ぐ（使い捨ての cache・prefix）。
pub async fn start(sb: &Sandbox, toolset: impl AsRef<OsStr>, allow: Option<&str>) -> Client {
    let server = start_server(sb, toolset, allow).await;
    let inner =
        ().serve(StreamableHttpClientTransport::from_uri(server.url()))
            .await
            .expect("initialize");
    Client { inner, server }
}

pub fn text(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn call(c: &Client, args: Value) -> CallToolResult {
    let Value::Object(m) = args else { panic!() };
    c.call_tool(CallToolRequestParams::new("superbash-command").with_arguments(m))
        .await
        .expect("tools/call")
}

pub async fn description(c: &Client) -> String {
    let tools = c.list_tools(None).await.expect("tools/list").tools;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "superbash-command");
    tools[0].description.clone().unwrap().to_string()
}

pub fn is_error(r: &CallToolResult) -> bool {
    r.is_error == Some(true)
}

/// 状態を観測して待つ。`probe` が Some を返したらその値を返す。上限までに返らなければ panic。
pub async fn wait_for<T>(what: &str, limit: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = probe() {
            return v;
        }
        assert!(
            start.elapsed() < limit,
            "{what} を {limit:?} 待ったが成らなかった"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn wait_until(what: &str, limit: Duration, mut cond: impl FnMut() -> bool) {
    wait_for(what, limit, || cond().then_some(())).await;
}

/// process が居るか（zombie は居ない物として扱う）。
pub fn process_alive(pid: i32) -> bool {
    // SAFETY: signal 0 は存在の確認だけで、signal は送らない。
    let exists = unsafe { libc::kill(pid, 0) } == 0;
    if !exists {
        return false;
    }
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        // "pid (comm) S ..." の S が Z なら zombie
        Ok(s) => s.rsplit(')').next().and_then(|r| r.trim().chars().next()) != Some('Z'),
        Err(_) => true, // /proc が無い OS（mac）は kill の結果だけ
    }
}
