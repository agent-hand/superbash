//! 実際の入口からの確認: repo の名前（手元の bare repo）で渡す事と、Claude Code から道具を呼ぶ事。
//!
//! registry（verdaccio）からの取得の test は、publish の扱いが決まるまで置かない。

mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use common::*;
use serde_json::{Value, json};

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo-tools")
}

/// 手元の bare repo を repo の名前（git+file://）として渡し、MCP の mode で command を呼べる。
#[tokio::test]
async fn bare_repo_name_works_through_the_binary() {
    let sb = Sandbox::new("e2e-bare");
    let repo = make_bare_repo(&sb);
    let c = start(&sb, &repo, None).await;
    let d = description(&c).await;
    for n in ["lint", "lint-list", "count"] {
        assert!(d.contains(&format!("name: {n}")), "{d}");
    }
    let r = call(
        &c,
        json!({"name": "lint", "args": {"fix": true, "target": "src"}}),
    )
    .await;
    assert!(!is_error(&r), "{}", text(&r));
    assert!(text(&r).contains(r#""--fix","src""#), "{}", text(&r));
    c.cancel().await.unwrap();

    // --allow-cmd も repo の名前で効く
    let c = start(&sb, &repo, Some("count")).await;
    let d = description(&c).await;
    assert!(
        d.contains("name: count") && !d.contains("name: lint"),
        "{d}"
    );
    c.cancel().await.unwrap();
}

/// `claude` の場所。環境変数 SUPERBASH_E2E_CLAUDE か PATH から探す。
fn find_claude() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SUPERBASH_E2E_CLAUDE") {
        return Some(PathBuf::from(p));
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("claude"))
        .find(|p| p.is_file())
}

/// Claude Code（`claude -p`）に MCP の設定（作業 dir 内の file）で superbash を渡し、
/// superbash-command を呼ばせて、command の出力が返る事を確かめる。
/// claude が無い環境では、その事を出して skip する。host の claude の設定は変えない。
#[tokio::test]
async fn claude_code_calls_superbash_command() {
    let Some(claude) = find_claude() else {
        eprintln!(
            "SKIP: claude が PATH にも SUPERBASH_E2E_CLAUDE にも無いので、Claude Code の test を skip する"
        );
        return;
    };
    let sb = Sandbox::new("e2e-claude");
    let env: serde_json::Map<String, Value> = sb
        .env()
        .into_iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .chain([("PATH".to_string(), json!(std::env::var("PATH").unwrap()))])
        .collect();
    let config = sb.root.join("mcp.json");
    let body = json!({"mcpServers": {"superbash": {
        "type": "stdio",
        "command": env!("CARGO_BIN_EXE_superbash"),
        "args": [demo()],
        "env": env,
    }}});
    std::fs::write(&config, serde_json::to_string_pretty(&body).unwrap()).unwrap();

    let marker = format!("e2e-marker-{}", std::process::id());
    let prompt = format!(
        "MCP の道具 superbash-command を 1 回だけ呼べ。name は hello、args は {{\"who\": \"{marker}\"}}。道具の出力をそのまま返せ。"
    );
    let mut cmd = tokio::process::Command::new(&claude);
    cmd.current_dir(&sb.root)
        .args(["-p", &prompt])
        .arg("--mcp-config")
        .arg(&config)
        .args([
            "--strict-mcp-config",
            "--allowedTools",
            "mcp__superbash__superbash-command",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
        ])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(240), cmd.output())
        .await
        .expect("claude が 240 秒で終わらない")
        .expect("claude を起動できない");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let events: Vec<Value> = stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let blocks = |ty: &str| -> Vec<Value> {
        events
            .iter()
            .filter_map(|e| e["message"]["content"].as_array())
            .flatten()
            .filter(|b| b["type"] == ty)
            .cloned()
            .collect()
    };
    let called = blocks("tool_use")
        .iter()
        .any(|b| b["name"] == "mcp__superbash__superbash-command");
    let returned = blocks("tool_result")
        .iter()
        .any(|b| b.to_string().contains(&format!("hello {marker}")));
    assert!(
        called && returned,
        "called={called} returned={returned}\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
