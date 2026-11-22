//! MCP の mode（initialize・tools/list・tools/call）を、実際の MCP client から確かめる。

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::*;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo-tools")
}

#[tokio::test]
async fn initialize_list_and_call() {
    let sb = Sandbox::new("mcp-basic");
    let c = start(&sb, demo(), None).await;
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
    let c = start(&sb, demo(), None).await;
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
    let c = start(&sb, demo(), Some("hello,fail")).await;
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
    let c = start(&sb, demo(), Some("")).await;
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
    let c = start(&sb, demo(), None).await;
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

/// 状態 file を読む（まだ無い・途中なら None）。
fn read_status(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[tokio::test]
async fn background_returns_id_and_readable_output() {
    let sb = Sandbox::new("mcp-bg");
    let c = start(&sb, demo(), None).await;
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
    // CARGO_TARGET_DIR が未正規化・相対でも比べられるよう、両方を正規化する。
    // output file はまだ無い事があるので、親 dir を正規化して file 名を足す。
    let out_norm = out
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(out.file_name().unwrap());
    let cache_norm = sb.cache().canonicalize().unwrap();
    assert!(out_norm.starts_with(&cache_norm), "{}", out.display());

    // 状態が running でなくなるのを観測して待つ
    wait_until("background の終了", Duration::from_secs(60), || {
        read_status(&status).is_some_and(|s| s["state"] != "running")
    })
    .await;
    let output = std::fs::read_to_string(&out).unwrap();
    assert!(
        output.contains("started") && output.contains("finished"),
        "{output}"
    );
    let st = read_status(&status).unwrap();
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
    let c = start(&sb, demo(), None).await;
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
    wait_until("background の終了", Duration::from_secs(60), || {
        read_status(&status).is_some_and(|s| s["state"] != "running")
    })
    .await;
    let st = read_status(&status).unwrap();
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

#[tokio::test]
async fn background_does_not_validate_timeout() {
    let sb = Sandbox::new("mcp-bg-bad-timeout");
    let c = start(&sb, demo(), None).await;
    // background の時は timeout を見ない（型が不正でも error にしない）
    let r = call(
        &c,
        json!({"name": "nap", "args": {"ms": 1}, "run_in_background": true, "timeout": "x"}),
    )
    .await;
    assert!(!is_error(&r), "{}", text(&r));
    let status = PathBuf::from(
        r.structured_content.unwrap()["status_path"]
            .as_str()
            .unwrap(),
    );
    wait_until("background の終了", Duration::from_secs(60), || {
        read_status(&status).is_some_and(|s| s["state"] != "running")
    })
    .await;
    // foreground では、不正な timeout は error のまま
    let r = call(
        &c,
        json!({"name": "nap", "args": {"ms": 1}, "timeout": "x"}),
    )
    .await;
    assert!(is_error(&r) && text(&r).contains("timeout"), "{}", text(&r));
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn server_exit_stops_running_background() {
    let sb = Sandbox::new("mcp-bg-server-exit");
    let c = start(&sb, demo(), None).await;
    call(&c, json!({"name": "nap", "args": {"ms": 1}})).await; // 初回の展開

    let r = call(
        &c,
        json!({"name": "nap", "args": {"ms": 600000}, "run_in_background": true}),
    )
    .await;
    let sc = r.structured_content.unwrap();
    let out = PathBuf::from(sc["output_path"].as_str().unwrap());
    // command が走り出し、pid を出力するまで待つ
    let pid = wait_for(
        "background の command の pid",
        Duration::from_secs(60),
        || {
            std::fs::read_to_string(&out)
                .ok()?
                .lines()
                .find_map(|l| l.strip_prefix("pid ")?.trim().parse::<i32>().ok())
        },
    )
    .await;
    assert!(process_alive(pid), "起動直後に pid {pid} が居ない");

    // client が閉じる = server の終了
    c.cancel().await.unwrap();
    wait_until("background の停止", Duration::from_secs(30), || {
        !process_alive(pid)
    })
    .await;
}

/// 生の HTTP/1.1 で `POST <path>`（initialize）を送り、status 行の code を返す。
async fn post_status(addr: std::net::SocketAddr, path: &str, extra: &[(&str, &str)]) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let body = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-03-26","capabilities":{},
        "clientInfo":{"name":"raw","version":"0"}}})
    .to_string();
    let mut headers = vec![
        ("Host", addr.to_string()),
        ("Content-Type", "application/json".into()),
        ("Accept", "application/json, text/event-stream".into()),
        ("Content-Length", body.len().to_string()),
        ("Connection", "close".into()),
    ];
    headers.retain(|(k, _)| !extra.iter().any(|(e, _)| e.eq_ignore_ascii_case(k)));
    headers.extend(extra.iter().map(|(k, v)| (*k, v.to_string())));
    let mut req = format!("POST {path} HTTP/1.1\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&body);

    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut head = vec![0u8; 12];
    s.read_exact(&mut head).await.unwrap();
    // "HTTP/1.1 200"
    String::from_utf8(head).unwrap()[9..12].parse().unwrap()
}

#[tokio::test]
async fn http_endpoint_origin_and_host_checks() {
    let sb = Sandbox::new("mcp-origin");
    let server = start_server(&sb, demo(), None).await;
    let addr = server.addr;
    assert!(addr.ip().is_loopback());

    // Origin 無し（非 browser の client）と、loopback の Origin は通る
    assert_eq!(post_status(addr, "/mcp", &[]).await, 200);
    let local = format!("http://localhost:{}", addr.port());
    assert_eq!(post_status(addr, "/mcp", &[("Origin", &local)]).await, 200);
    let ip = format!("http://127.0.0.1:{}", addr.port());
    assert_eq!(post_status(addr, "/mcp", &[("Origin", &ip)]).await, 200);

    // 別の Origin・壊れた Origin は 403（DNS rebinding・cross-origin の対策）
    for origin in ["http://evil.example", "https://localhost:1", "null", "x"] {
        assert_eq!(
            post_status(addr, "/mcp", &[("Origin", origin)]).await,
            403,
            "Origin: {origin}"
        );
    }
    // loopback 以外の Host は 403
    assert_eq!(
        post_status(addr, "/mcp", &[("Host", "evil.example")]).await,
        403
    );
    // endpoint は /mcp だけ
    assert_eq!(post_status(addr, "/other", &[]).await, 404);
    server.stop().await;
}

/// 相対 path の toolset は、npm pack の作業 dir ではなく起動した dir（= sb.root）から解く。
#[tokio::test]
async fn relative_path_toolset_resolves_from_launch_dir() {
    for (name, spec) in [
        ("mcp-relpath-dot", "./probe-tools"),
        ("mcp-relpath-dotdot", "../mcp-relpath-dotdot/probe-tools"),
        ("mcp-relpath-file", "file:./probe-tools"),
        ("mcp-relpath-file-bare", "file:probe-tools"),
        (
            "mcp-relpath-file-slash",
            "file:/../mcp-relpath-file-slash/probe-tools",
        ),
    ] {
        let sb = Sandbox::new(name);
        copy_dir(&demo(), &sb.root.join("probe-tools"));
        let c = start(&sb, spec, None).await;
        let d = description(&c).await;
        assert!(
            d.contains("name: hello"),
            "{spec}: 説明に hello が無い:\n{d}"
        );
        let r = call(&c, json!({"name": "hello", "args": {"who": "rel"}})).await;
        assert!(!is_error(&r), "{spec}: {}", text(&r));
        assert!(text(&r).contains("hello rel"), "{spec}: {}", text(&r));
        c.cancel().await.unwrap();
    }
}
