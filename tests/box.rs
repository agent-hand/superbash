//! busybox 化（作法 5.4）: module のある tool は振り分けの file から動き、module の無い tool は今のまま動く。

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::*;
use superbash::cli::AllowCmd;
use superbash::config::ToolConfig;
use superbash::fetch::fetch_toolset;
use superbash::generate::{DISPATCH_PATH, GenerateInput, WRAPPER_BIN, generate};
use superbash::modes;

fn box_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/box-tools")
}

fn generated(sb: &Sandbox, with_wrapper: bool) -> superbash::generate::Generated {
    let npm = sb.npm();
    let f = fetch_toolset(&npm, box_fixture().to_str().unwrap(), &sb.dir("w")).unwrap();
    let cfg = ToolConfig::load(&f.dir).unwrap();
    generate(
        &npm,
        &GenerateInput {
            toolset_dir: &f.dir,
            name: &f.name,
            version: &f.version,
            config: &cfg,
            allow: &AllowCmd::All,
            with_wrapper,
            root: &sb.dir("root"),
        },
    )
    .unwrap()
}

fn stdout_json(o: &Output) -> serde_json::Value {
    assert!(
        o.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn bin_of_a_module_tool_points_to_the_dispatcher() {
    let sb = Sandbox::new("box-bins");
    let g = generated(&sb, true);
    assert_eq!(g.bins["box-cli"], DISPATCH_PATH);
    assert_eq!(g.bins["plain-cli"], "tools/plain-cli/main.js");
    assert_eq!(g.bins[WRAPPER_BIN], "wrapper/run.js");
    let d = g.package_dir.join(DISPATCH_PATH);
    assert!(d.is_file());
    assert_ne!(d.metadata().unwrap().permissions().mode() & 0o111, 0);
    // 振り分けの file は citty を持たない
    let src = std::fs::read_to_string(d).unwrap();
    assert!(!src.contains("from 'citty'") && !src.contains("import('citty')"));
}

#[test]
fn package_without_module_has_no_dispatcher() {
    let sb = Sandbox::new("box-none");
    let npm = sb.npm();
    let f = fetch_toolset(&npm, fixture().to_str().unwrap(), &sb.dir("w")).unwrap();
    let cfg = ToolConfig::load(&f.dir).unwrap();
    let g = generate(
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
    .unwrap();
    assert!(!g.package_dir.join("dispatch").exists());
}

#[test]
fn mcp_mode_wrapper_runs_module_tool_through_dispatcher() {
    let sb = Sandbox::new("box-mcp");
    let tgz = generated(&sb, true).tarball;

    let out = sb.npx(&tgz, WRAPPER_BIN, &["echo", r#"{"tag":"t","word":"--w"}"#]);
    let v = stdout_json(&out);
    assert_eq!(v["via"], "module");
    assert_eq!(
        v["argv"],
        serde_json::json!(["echo", "--tag", "t", "--", "--w"])
    );

    // 終了 code: 使い方の誤りは 2、その他は 1（module の run の返す数）
    let out = sb.npx(&tgz, WRAPPER_BIN, &["bad", "{}"]);
    assert_eq!(out.status.code(), Some(2));
    let out = sb.npx(&tgz, WRAPPER_BIN, &["fail", "{}"]);
    assert_eq!(out.status.code(), Some(1));

    // module の無い tool は entry を直に起動する（今のまま）
    let v = stdout_json(&sb.npx(&tgz, WRAPPER_BIN, &["plain", "{}"]));
    assert_eq!(v["via"], "entry");
    assert_eq!(v["argv"], serde_json::json!(["plain"]));
}

#[test]
fn dispatcher_selects_tool_by_first_argument_and_rejects_unknown() {
    let sb = Sandbox::new("box-first-arg");
    let g = generated(&sb, true);
    let d = g.package_dir.join(DISPATCH_PATH);
    let run = |args: &[&str]| Command::new(&d).args(args).output().unwrap();

    let v = stdout_json(&run(&["box-cli", "echo", "a"]));
    assert_eq!(v["argv"], serde_json::json!(["echo", "a"]));
    // module の無い tool・知らない名前・名前無しは使い方の誤り（2）
    for args in [&["plain-cli"][..], &["nope"], &[]] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn only_install_link_names_select_the_tool() {
    let sb = Sandbox::new("box-install");
    modes::install(&sb.npm(), box_fixture().to_str().unwrap(), &AllowCmd::All).unwrap();
    let bin = sb.root.join("prefix/bin");
    let mut names: Vec<String> = std::fs::read_dir(&bin)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["box-cli", "plain-cli"]);

    let mut path = std::ffi::OsString::from(&bin);
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap());
    let run = |program: &str, args: &[&str]| {
        let mut c = Command::new(program);
        c.env_clear().env("PATH", &path);
        for (k, v) in sb.env() {
            c.env(k, v);
        }
        c.current_dir(&sb.root).args(args).output().unwrap()
    };

    // link の名前（box-cli）で module の tool が動く。第 1 引数は tool の名前ではなく argv
    let v = stdout_json(&run("box-cli", &["echo", "x y"]));
    assert_eq!(v["via"], "module");
    assert_eq!(v["argv"], serde_json::json!(["echo", "x y"]));
    assert_eq!(run("box-cli", &["bad"]).status.code(), Some(2));
    assert_eq!(run("box-cli", &["fail"]).status.code(), Some(1));

    // npx 経由（registry に行かない）でも link の名前で選ばれる
    let v = stdout_json(&run("npx", &["--offline", "box-cli", "echo", "z"]));
    assert_eq!(v["via"], "module");

    // module の無い tool は今のまま
    let v = stdout_json(&run("plain-cli", &["p"]));
    assert_eq!(v["via"], "entry");
    assert_eq!(v["argv"], serde_json::json!(["p"]));

    // only-install の package に wrapper は無く、振り分けの file はある
    let pkg = sb.root.join("prefix/lib/node_modules/box-tools");
    assert!(!pkg.join("wrapper").exists());
    assert!(pkg.join(DISPATCH_PATH).is_file());
}
