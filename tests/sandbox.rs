//! `--sandbox`（設計 2 章「sandbox の中での実行」）。
//! 組み立ての test は OS に依らない。実際に止まる事の test は、仕組み（Linux は bwrap、macOS は sandbox-exec）が
//! 使える時だけ走る（無ければ skip）。

mod common;

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use serde_json::json;
use superbash::cli::AllowCmd;
use superbash::config::ToolConfig;
use superbash::fetch::fetch_toolset;
use superbash::generate::{GenerateInput, WRAPPER_BIN, generate};
use superbash::sandbox::Mechanism;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sandbox-tools")
}

/// この host で sandbox が使えるか（使えなければ skip の理由つき）。
fn mechanism() -> Option<Mechanism> {
    match Mechanism::detect().and_then(|m| m.probe().map(|()| m)) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!("SKIP: sandbox が使えない: {e:#}");
            None
        }
    }
}

fn exit_code(r: &rmcp::model::CallToolResult) -> Option<i64> {
    r.structured_content.as_ref().unwrap()["exit_code"].as_i64()
}

fn home(sb: &Sandbox) -> PathBuf {
    sb.root.join("home")
}

#[tokio::test]
async fn without_the_option_nothing_is_restricted() {
    let sb = Sandbox::new("sbx-off");
    std::fs::create_dir_all(home(&sb).join("denied")).unwrap();
    let c = start(&sb, fixture_dir(), None).await;
    let target = home(&sb).join("denied/f");
    let r = call(
        &c,
        json!({"name":"net-off-write","args":{"path": target.to_str().unwrap()}}),
    )
    .await;
    assert_eq!(exit_code(&r), Some(0), "{}", text(&r));
    assert!(target.is_file());
    // network も絞らない
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let r = call(&c, json!({"name":"default-connect","args":{"port": port}})).await;
    assert_eq!(exit_code(&r), Some(0), "{}", text(&r));
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn sandbox_restricts_write_and_network_per_tool() {
    let Some(_) = mechanism() else { return };
    let sb = Sandbox::new("sbx-on");
    let allowed = home(&sb).join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(home(&sb).join("denied")).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let c = start_with(&sb, fixture_dir(), None, &["--sandbox"]).await;

    let write = |tool: &str, p: &Path| json!({"name": format!("{tool}-write"), "args":{"path": p.to_str().unwrap()}});
    // write の中は書ける
    for tool in ["net-off", "net-on"] {
        let f = allowed.join(format!("{tool}.txt"));
        let r = call(&c, write(tool, &f)).await;
        assert_eq!(exit_code(&r), Some(0), "{tool}: {}", text(&r));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "hello");
    }
    // write の外は書けない（読み取りは絞らない = tool 自体は動く）
    for tool in ["net-off", "net-on", "default"] {
        let f = home(&sb).join("denied").join(format!("{tool}.txt"));
        let r = call(&c, write(tool, &f)).await;
        assert_eq!(exit_code(&r), Some(3), "{tool}: {}", text(&r));
        assert!(text(&r).contains("write failed"), "{}", text(&r));
        assert!(!f.exists());
    }
    // 項の無い tool: write は既定（置き場と一時の dir）だけなので、書いた物のある home の中は書けない
    // network: true の tool だけが外へ繋げる（宛先は 127.0.0.1 の test の server）
    for (tool, want) in [("net-on", 0), ("net-off", 3), ("default", 3)] {
        let r = call(
            &c,
            json!({"name": format!("{tool}-connect"), "args":{"port": port}}),
        )
        .await;
        assert_eq!(exit_code(&r), Some(want), "{tool}: {}", text(&r));
    }
    c.cancel().await.unwrap();
    drop(l);
}

// --- OS に依らない test ---

#[test]
fn start_fails_with_a_reason_when_the_mechanism_is_missing() {
    if std::env::consts::OS != "linux" {
        eprintln!("SKIP: Linux だけ（macOS の sandbox-exec は在る）");
        return;
    }
    let sb = Sandbox::new("sbx-missing");
    let out = Command::new(env!("CARGO_BIN_EXE_superbash"))
        .env_clear()
        .env("PATH", sb.root.join("empty-path"))
        .env("HOME", home(&sb))
        .args(["--sandbox", "--port", "0"])
        .arg(fixture_dir())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("bwrap") && e.contains("--sandbox"), "{e}");
    assert!(out.stdout.is_empty(), "待ち受けを始めている");
}

#[test]
fn only_install_with_sandbox_is_rejected() {
    let out = Command::new(env!("CARGO_BIN_EXE_superbash"))
        .args(["--only-install", "--sandbox"])
        .arg(fixture_dir())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

fn tarball(sb: &Sandbox) -> PathBuf {
    let npm = sb.npm();
    let f = fetch_toolset(&npm, fixture_dir().to_str().unwrap(), &sb.dir("w")).unwrap();
    let cfg = ToolConfig::load(&f.dir).unwrap();
    generate(
        &npm,
        &GenerateInput {
            toolset_dir: &f.dir,
            name: &f.name,
            version: &f.version,
            config: &cfg,
            allow: &AllowCmd::All,
            with_wrapper: true,
            root: &sb.dir("root"),
        },
    )
    .unwrap()
    .tarball
}

fn npx_env(sb: &Sandbox, tgz: &Path, prefix: Option<&str>, args: &[&str]) -> std::process::Output {
    let mut c = Command::new("npx");
    c.env_clear().env("PATH", std::env::var_os("PATH").unwrap());
    for (k, v) in sb.env() {
        c.env(k, v);
    }
    if let Some(p) = prefix {
        c.env("SUPERBASH_SANDBOX", p);
    }
    c.current_dir(&sb.root)
        .arg("--yes")
        .arg(format!("--package={}", tgz.display()))
        .arg("--")
        .arg(WRAPPER_BIN)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn wrapper_puts_the_prefix_before_the_tool_and_fails_closed() {
    let sb = Sandbox::new("sbx-wrapper");
    let tgz = tarball(&sb);
    let a = ["default-env", r#"{"name":"SBX_MARK"}"#];
    // prefix 無し: 今のまま
    let o = npx_env(&sb, &tgz, None, &a);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "env:(unset)");
    // prefix あり: tool が prefix の中（env が SBX_MARK を足す）で起きる
    let o = npx_env(&sb, &tgz, Some(r#"["/usr/bin/env","SBX_MARK=1"]"#), &a);
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim(),
        "env:1",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    // prefix は tool に渡らない
    let o = npx_env(
        &sb,
        &tgz,
        Some(r#"["/usr/bin/env","SBX_MARK=1"]"#),
        &["default-env", r#"{"name":"SUPERBASH_SANDBOX"}"#],
    );
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "env:(unset)");
    // 読めない prefix は、sandbox 無しで起こさず止まる
    for bad in ["not json", "[]", "[1]", r#"{"a":1}"#] {
        let o = npx_env(&sb, &tgz, Some(bad), &a);
        assert!(!o.status.success(), "{bad}");
        assert!(o.stdout.is_empty(), "{bad}: tool が起きた");
    }
}

#[test]
fn wrapper_runs_the_real_prefix_when_available() {
    let Some(m) = mechanism() else { return };
    let sb = Sandbox::new("sbx-wrapper-real");
    let tgz = tarball(&sb);
    let allowed = sb.dir("ok");
    let denied = sb.dir("no");
    let policy = superbash::sandbox::Policy {
        write: vec![allowed.clone()],
        network: false,
    };
    let prefix: Vec<String> = m
        .prefix(&policy)
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let p = serde_json::to_string(&prefix).unwrap();
    for (dir, want) in [(&allowed, true), (&denied, false)] {
        let f = dir.join("f");
        let args = json!({"path": f.to_str().unwrap()}).to_string();
        let o = npx_env(&sb, &tgz, Some(&p), &["default-write", &args]);
        assert_eq!(f.exists(), want, "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(o.status.success(), want);
    }
}
