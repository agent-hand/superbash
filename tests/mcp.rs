//! MCP の mode（initialize・tools/list・tools/call）を、実際の MCP client から確かめる。

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::*;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};
use tokio::process::Command;

type Client = RunningService<RoleClient, ()>;

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo-tools")
}

async fn start(sb: &Sandbox, toolset: &Path, allow: Option<&str>) -> Client {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_superbash"));
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap());
    for (k, v) in sb.env() {
        cmd.env(k, v);
    }
    cmd.current_dir(&sb.root);
    if let Some(a) = allow {
        cmd.arg("--allow-cmd").arg(a);
    }
    cmd.arg(toolset);
    ().serve(TokioChildProcess::new(cmd).unwrap())
        .await
        .expect("initialize")
}

fn text(r: &CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn call(c: &Client, args: Value) -> CallToolResult {
    let Value::Object(m) = args else { panic!() };
    c.call_tool(CallToolRequestParams::new("superbash-command").with_arguments(m))
        .await
        .expect("tools/call")
}

async fn description(c: &Client) -> String {
    let tools = c.list_tools(None).await.expect("tools/list").tools;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "superbash-command");
    tools[0].description.clone().unwrap().to_string()
}

fn is_error(r: &CallToolResult) -> bool {
    r.is_error == Some(true)
}

#[tokio::test]
async fn initialize_list_and_call() {
    let sb = Sandbox::new("mcp-basic");
    let c = start(&sb, &demo(), None).await;
    assert_eq!(
        c.peer_info().unwrap().server_info.as_ref().unwrap().name,
        "superbash"
    );

    let d = description(&c).await;
    for want in [
        "name: hello",
        "name: fail",
        "name: nap",
        "挨拶を出す。",
        "usage: demo-cli hello <who>",
        "who (String positional): 相手",
        "ms (Number option --ms)",
        "out: ",
    ] {
        assert!(d.contains(want), "説明に {want} が無い:\n{d}");
    }

    let r = call(&c, json!({"name": "hello", "args": {"who": "a b;$(id)"}})).await;
    assert!(!is_error(&r), "{}", text(&r));
    assert!(text(&r).contains("hello a b;$(id)"), "{}", text(&r));
    assert!(text(&r).contains("終了 code: 0"));
    assert_eq!(r.structured_content.as_ref().unwrap()["exit_code"], 0);

    // 終了 code と stderr
    let r = call(&c, json!({"name": "fail"})).await;
    let s = r.structured_content.unwrap();
    assert_eq!(
        (s["exit_code"].as_i64(), s["stderr"].as_str()),
        (Some(3), Some("boom\n"))
    );

    // 道具の名前が違う
    let e = c
        .call_tool(CallToolRequestParams::new("other"))
        .await
        .unwrap_err();
    assert!(format!("{e:?}").contains("道具が無い"));
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn allow_cmd_changes_runnable_commands() {
    let sb = Sandbox::new("mcp-allow");

    // 指定無し: 全部
    let c = start(&sb, &demo(), None).await;
    let d = description(&c).await;
    for n in ["hello", "fail", "nap"] {
        assert!(d.contains(&format!("name: {n}")), "{d}");
    }
    let r = call(&c, json!({"name": "hello", "args": {"who": "x"}})).await;
    assert!(!is_error(&r));
    // 未知の name は error
    let r = call(&c, json!({"name": "nope"})).await;
    assert!(
        is_error(&r) && text(&r).contains("実行できない command: nope"),
        "{}",
        text(&r)
    );
    c.cancel().await.unwrap();

    // 一覧: その command だけ
    let c = start(&sb, &demo(), Some("hello,fail")).await;
    let d = description(&c).await;
    assert!(d.contains("name: hello") && d.contains("name: fail"));
    assert!(!d.contains("name: nap"), "{d}");
    assert!(!is_error(
        &call(&c, json!({"name": "hello", "args": {"who": "x"}})).await
    ));
    let r = call(&c, json!({"name": "nap", "args": {"ms": 1}})).await;
    assert!(is_error(&r) && text(&r).contains("実行できない command: nap"));
    c.cancel().await.unwrap();

    // '': 無し。起動はでき、全部 error
    let c = start(&sb, &demo(), Some("")).await;
    let d = description(&c).await;
    assert!(d.contains("実行できる command は無い"), "{d}");
    for n in ["hello", "nap", "nope"] {
        let r = call(&c, json!({"name": n, "args": {}})).await;
        assert!(is_error(&r), "{n}: {}", text(&r));
    }
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn timeout_stops_and_returns_output_so_far() {
    let sb = Sandbox::new("mcp-timeout");
    let c = start(&sb, &demo(), None).await;
    // npx の初回の展開を済ませておく（timeout の中に入れない）
    let r = call(&c, json!({"name": "nap", "args": {"ms": 1}})).await;
    assert!(text(&r).contains("finished"), "{}", text(&r));

    let t = Instant::now();
    let r = call(
        &c,
        json!({"name": "nap", "args": {"ms": 120000}, "timeout": 3000}),
    )
    .await;
    assert!(t.elapsed() < Duration::from_secs(30), "止まらなかった");
    assert!(is_error(&r));
    let s = text(&r);
    assert!(s.contains("started") && s.contains("時間切れ"), "{s}");
    assert!(!s.contains("finished"));
    let sc = r.structured_content.unwrap();
    assert_eq!(sc["timed_out"], true);
    assert!(sc["stdout"].as_str().unwrap().contains("started"));
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn background_returns_id_and_readable_output() {
    let sb = Sandbox::new("mcp-bg");
    let c = start(&sb, &demo(), None).await;
    call(&c, json!({"name": "nap", "args": {"ms": 1}})).await; // 初回の展開

    let r = call(
        &c,
        json!({"name": "nap", "args": {"ms": 300}, "run_in_background": true}),
    )
    .await;
    assert!(!is_error(&r), "{}", text(&r));
    let sc = r.structured_content.unwrap();
    let id = sc["id"].as_str().unwrap();
    let out = PathBuf::from(sc["output_path"].as_str().unwrap());
    let status = PathBuf::from(sc["status_path"].as_str().unwrap());
    assert!(id.starts_with("bg-"));
    assert!(out.starts_with(sb.cache()), "{}", out.display());

    // 終わるまでの時間は 1 回だけ待つ（npx の起動を含めても十分な長さ）
    tokio::time::sleep(Duration::from_secs(8)).await;
    let output = std::fs::read_to_string(&out).unwrap();
    assert!(
        output.contains("started") && output.contains("finished"),
        "{output}"
    );
    let st: Value = serde_json::from_str(&std::fs::read_to_string(&status).unwrap()).unwrap();
    assert_eq!(
        (st["state"].as_str(), st["exit_code"].as_i64()),
        (Some("exited"), Some(0))
    );
    assert_eq!(st["id"], id);
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn background_ignores_timeout() {
    let sb = Sandbox::new("mcp-bg-no-timeout");
    let c = start(&sb, &demo(), None).await;
    call(&c, json!({"name": "nap", "args": {"ms": 1}})).await;
    // timeout 1000ms より長く（約 3 秒）走る background は、止まらずに最後まで走る
    let r = call(
        &c,
        json!({"name": "nap", "args": {"ms": 3000}, "run_in_background": true, "timeout": 1000}),
    )
    .await;
    let sc = r.structured_content.unwrap();
    let out = PathBuf::from(sc["output_path"].as_str().unwrap());
    let status = PathBuf::from(sc["status_path"].as_str().unwrap());
    tokio::time::sleep(Duration::from_secs(9)).await;
    let st: Value = serde_json::from_str(&std::fs::read_to_string(&status).unwrap()).unwrap();
    assert_eq!(
        (st["state"].as_str(), st["exit_code"].as_i64()),
        (Some("exited"), Some(0)),
        "{st}"
    );
    assert!(st.get("timeout_ms").is_none());
    let output = std::fs::read_to_string(&out).unwrap();
    assert!(
        output.contains("started") && output.contains("finished"),
        "{output}"
    );
    c.cancel().await.unwrap();
}
