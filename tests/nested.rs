//! サブコマンド（入れ子 3 段の fixture）: 道具の説明の木の形、並びの名前での呼び出し、argv、
//! 群の呼び出し、`--allow-cmd` の群・葉・知らない名前（toolset の作法 docs/toolset-convention.md）。

mod common;

use std::path::{Path, PathBuf};

use common::*;
use serde_json::json;
use superbash::cli::AllowCmd;
use superbash::config::ToolConfig;
use superbash::fetch::fetch_toolset;
use superbash::generate::{GenerateInput, WRAPPER_BIN, generate};

fn nested() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nested-tools")
}

fn tarball(sb: &Sandbox) -> PathBuf {
    let npm = sb.npm();
    let f = fetch_toolset(&npm, nested().to_str().unwrap(), &sb.dir("w")).unwrap();
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

fn run(sb: &Sandbox, tgz: &Path, name: &str, args: &str) -> (Vec<String>, std::process::Output) {
    let out = sb.npx(tgz, WRAPPER_BIN, &[name, args]);
    let argv = std::str::from_utf8(&out.stdout)
        .ok()
        .and_then(|s| serde_json::from_str(s.trim()).ok())
        .unwrap_or_default();
    (argv, out)
}

#[test]
fn nested_names_decide_argv() {
    let sb = Sandbox::new("nested-wrapper");
    let tgz = tarball(&sb);

    // spec.subcommand、並びの名前（上の要素より下）、options、`--`、positionals の順
    let args = r#"{"title":"v1.0.0","draft":true,"repo":"example/app","tag":"v1.0.0"}"#;
    let want = [
        "github",
        "release",
        "create",
        "--title",
        "v1.0.0",
        "--draft",
        "--",
        "example/app",
        "v1.0.0",
    ];
    let (argv, out) = run(&sb, &tgz, "github release create", args);
    assert_eq!(argv, want, "{}", String::from_utf8_lossy(&out.stderr));
    // 前後の空白は捨て、続いた空白は 1 つの区切り
    let (argv, _) = run(&sb, &tgz, "  github   release \t create  ", args);
    assert_eq!(argv, want);

    // 別の枝（同じ名前 create でも args は呼ぶ葉の spec.in だけで読む）
    let (argv, _) = run(&sb, &tgz, "github repo create", r#"{"repo":"x/y"}"#);
    assert_eq!(argv, ["github", "repo", "create", "--", "x/y"]);
    let (_, out) = run(
        &sb,
        &tgz,
        "github repo create",
        r#"{"title":"t","repo":"x/y"}"#,
    );
    assert_eq!(out.status.code(), Some(2), "他の葉の option は未定義");

    // positional を渡さない葉には `--` を入れない
    let (argv, _) = run(&sb, &tgz, "github release list", "{}");
    assert_eq!(argv, ["github", "release", "list"]);

    // spec.subcommand の無い上の要素: 下の名前だけ
    let (argv, _) = run(&sb, &tgz, "plain ls", "{}");
    assert_eq!(argv, ["ls"]);
}

#[test]
fn dash_leading_positionals_stay_one_arg_after_double_dash() {
    let sb = Sandbox::new("nested-dash");
    let tgz = tarball(&sb);
    for v in ["-x", "--help", "-h", "-", "--", "--title", "a b"] {
        let args = json!({ "repo": v, "tag": "-t", "title": "-T" }).to_string();
        let (argv, out) = run(&sb, &tgz, "github release create", &args);
        assert_eq!(
            argv,
            [
                "github", "release", "create", "--title", "-T", "--", v, "-t"
            ],
            "value {v:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn group_and_unknown_names_fail_with_usage_exit_code() {
    let sb = Sandbox::new("nested-bad");
    let tgz = tarball(&sb);
    for (name, args, hint) in [
        ("github", "{}", Some("repo, release")),
        ("github release", "{}", Some("create, list")),
        ("plain", "{}", Some("ls")),
        ("github nope", "{}", None),
        ("github release create extra", "{}", None),
        ("release create", "{}", None),
        ("   ", "{}", None),
    ] {
        let (_, out) = run(&sb, &tgz, name, args);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{name:?}: {err}");
        assert!(out.stdout.is_empty());
        if let Some(h) = hint {
            assert!(err.contains(h), "{name:?} の誤りに子の一覧が無い: {err}");
        }
    }
}

#[tokio::test]
async fn tools_list_is_a_tree_and_tools_call_uses_sequence_names() {
    let sb = Sandbox::new("nested-mcp");
    let c = start(&sb, nested(), None).await;

    let d = description(&c).await;
    for want in [
        "- name: github",
        "    - name: release",
        "        - name: create（呼ぶ name: \"github release create\"）",
        "        - name: create（呼ぶ name: \"github repo create\"）",
        "usage: gh-cli github release create [--title <title>] [--draft] <repo> <tag>",
        "title (String option --title): 題。省略時は tag",
        "draft (Boolean option --draft): 下書きにする",
        "tag (String positional): tag の名前",
        // out は短く（stdout.format と終了 code の意味だけ）
        "out: stdout=json; exit 0=作った, 1=失敗, 2=使い方の誤り",
        "- name: plain",
    ] {
        // repo create の行は repo の下にある
        if want.contains("github repo create") {
            assert!(d.contains("（呼ぶ name: \"github repo create\"）"), "{d}");
            continue;
        }
        assert!(d.contains(want), "説明に {want} が無い:\n{d}");
    }
    assert!(
        !d.contains("LONG-OUT-DESCRIPTION"),
        "out の長文が残っている:\n{d}"
    );
    // 木の順: github → repo → release（子は親の後、深さの順）
    let pos = |s: &str| d.find(s).unwrap_or_else(|| panic!("{s} が無い:\n{d}"));
    assert!(pos("- name: github") < pos("    - name: repo"));
    assert!(pos("    - name: repo") < pos("    - name: release"));
    assert!(pos("    - name: release") < pos("- name: plain"));

    // input schema の name の説明
    let tools = c.list_tools(None).await.unwrap().tools;
    let name_desc = tools[0].input_schema["properties"]["name"]["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(name_desc.contains("空白で区切った並び"), "{name_desc}");

    // 並びの名前で呼べて、argv が決まりどおり
    let r = call(
        &c,
        json!({"name": "github release create", "args": {
            "title": "t", "draft": true, "repo": "-r", "tag": "v1"}}),
    )
    .await;
    assert!(!is_error(&r), "{}", text(&r));
    let s = r.structured_content.unwrap();
    let argv: Vec<String> = serde_json::from_str(s["stdout"].as_str().unwrap().trim()).unwrap();
    assert_eq!(
        argv,
        [
            "github", "release", "create", "--title", "t", "--draft", "--", "-r", "v1"
        ]
    );

    // 群を呼ぶと、子の名前の一覧の誤り
    for (name, hint) in [
        ("github release", "create, list"),
        ("github", "repo, release"),
    ] {
        let r = call(&c, json!({"name": name})).await;
        assert!(is_error(&r), "{name}");
        let t = text(&r);
        assert!(t.contains("葉だけ") && t.contains(hint), "{t}");
    }
    // 知らない名前・葉の下の名前
    for name in ["nope", "github release create x", "github nope"] {
        let r = call(&c, json!({"name": name})).await;
        assert!(is_error(&r), "{name}");
        assert!(text(&r).contains("実行できない command"), "{}", text(&r));
    }
    c.cancel().await.unwrap();
}

#[tokio::test]
async fn allow_cmd_accepts_groups_leaves_and_warns_on_unknown() {
    let sb = Sandbox::new("nested-allow");

    // 群: その下の全部（repo は無い）
    let c = start(&sb, nested(), Some("github release")).await;
    let d = description(&c).await;
    assert!(
        d.contains("    - name: release") && d.contains("- name: list"),
        "{d}"
    );
    assert!(
        !d.contains("    - name: repo") && !d.contains("- name: plain"),
        "{d}"
    );
    for n in ["github release create", "github release list"] {
        let r = call(&c, json!({"name": n, "args": {"repo": "a", "tag": "b"}})).await;
        assert!(!is_error(&r) || n.ends_with("list"), "{n}: {}", text(&r));
    }
    let r = call(
        &c,
        json!({"name": "github repo create", "args": {"repo": "a"}}),
    )
    .await;
    assert!(is_error(&r) && text(&r).contains("実行できない command"));
    c.cancel().await.unwrap();

    // 葉（空白は正規化）: その葉だけ
    let c = start(&sb, nested(), Some("  github   release   list ")).await;
    let d = description(&c).await;
    assert!(d.contains("- name: list"), "{d}");
    assert!(
        !d.contains("- name: create") && !d.contains("- name: repo"),
        "{d}"
    );
    assert!(!is_error(
        &call(&c, json!({"name": "github release list"})).await
    ));
    let r = call(
        &c,
        json!({"name": "github release create", "args": {"repo": "a", "tag": "b"}}),
    )
    .await;
    assert!(is_error(&r));
    c.cancel().await.unwrap();

    // 知らない名前は警告だけ（起動する）。知っている名前は効く
    let c = start(&sb, nested(), Some("nope,github nope,plain")).await;
    let d = description(&c).await;
    assert!(
        d.contains("- name: plain") && !d.contains("- name: github"),
        "{d}"
    );
    assert!(!is_error(&call(&c, json!({"name": "plain ls"})).await));
    c.cancel().await.unwrap();

    // 全部知らない名前: 実行できる command は無い（起動はする）
    let c = start(&sb, nested(), Some("nope")).await;
    assert!(description(&c).await.contains("実行できる command は無い"));
    c.cancel().await.unwrap();
}

fn bins(name: &str, allow: &str) -> Vec<String> {
    let sb = Sandbox::new(name);
    let npm = sb.npm();
    let f = fetch_toolset(&npm, nested().to_str().unwrap(), &sb.dir("w")).unwrap();
    let cfg = ToolConfig::load(&f.dir).unwrap();
    let g = generate(
        &npm,
        &GenerateInput {
            toolset_dir: &f.dir,
            name: &f.name,
            version: &f.version,
            config: &cfg,
            allow: &AllowCmd::parse(Some(allow)),
            with_wrapper: false,
            root: &sb.dir("root"),
        },
    )
    .unwrap();
    g.bins.keys().cloned().collect()
}

#[test]
fn only_install_narrows_to_tool_bin_not_to_leaves() {
    // 葉で絞っても bin は tool の単位（tool 全体が入る）。知らない名前だけなら入らない
    assert_eq!(bins("nested-bins-leaf", "github release list"), ["gh-cli"]);
    assert_eq!(bins("nested-bins-group", "plain"), ["gh-cli"]);
    assert!(bins("nested-bins-none", "nope").is_empty());
}
