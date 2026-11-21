mod common;

use std::path::PathBuf;
use std::process::Command;

use common::*;
use superbash::cli::AllowCmd;
use superbash::modes;

/// 使い捨ての prefix に only-install し、prefix/bin の bin の名前（昇順）を返す。
fn install(sb: &Sandbox, allow: AllowCmd) -> Vec<String> {
    modes::install(&sb.npm(), fixture().to_str().unwrap(), &allow).unwrap();
    let bin = sb.root.join("prefix/bin");
    let mut names: Vec<String> = match std::fs::read_dir(&bin) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// shell を通さず、使い捨ての環境で command を起動する。`path_bin` が true なら prefix/bin を PATH に足す。
fn run(sb: &Sandbox, program: &str, path_bin: bool, args: &[&str]) -> std::process::Output {
    let mut path = std::ffi::OsString::new();
    if path_bin {
        path.push(sb.root.join("prefix/bin"));
        path.push(":");
    }
    path.push(std::env::var_os("PATH").unwrap());
    let mut c = Command::new(program);
    c.env_clear().env("PATH", path);
    for (k, v) in sb.env() {
        c.env(k, v);
    }
    c.current_dir(&sb.root).args(args).output().unwrap()
}

fn stdout(o: &std::process::Output) -> String {
    assert!(
        o.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout.clone()).unwrap()
}

#[test]
fn only_install_links_bins_and_both_ways_run() {
    let sb = Sandbox::new("install-run");
    assert_eq!(install(&sb, AllowCmd::All), ["count-cli", "lint-cli"]);

    // `cmdA 引数`（PATH に global の bin がある）
    let out = run(&sb, "lint-cli", true, &["check", "--fix", "a b;ls"]);
    assert_eq!(stdout(&out).trim(), r#"["check","--fix","a b;ls"]"#);

    // `npx cmdA 引数`（PATH に無くても、registry に行かず動く）
    let out = run(&sb, "npx", false, &["--offline", "count-cli", "x", "y"]);
    assert_eq!(stdout(&out).trim(), r#"["x","y"]"#);
}

#[test]
fn only_install_has_no_wrapper() {
    let sb = Sandbox::new("install-nowrapper");
    install(&sb, AllowCmd::All);
    assert!(!sb.root.join("prefix/bin/superbash-run").exists());
    let pkg: PathBuf = sb.root.join("prefix/lib/node_modules/example-tools");
    assert!(pkg.join("package.json").is_file());
    assert!(!pkg.join("wrapper").exists());
}

#[test]
fn only_install_allow_cmd_three_ways() {
    // 指定無し: 全部
    assert_eq!(
        install(&Sandbox::new("install-all"), AllowCmd::All),
        ["count-cli", "lint-cli"]
    );
    // 一覧: その command が使う tool の bin だけ
    assert_eq!(
        install(
            &Sandbox::new("install-some"),
            AllowCmd::parse(Some("count"))
        ),
        ["count-cli"]
    );
    // '': 入れる command 無し
    assert!(install(&Sandbox::new("install-none"), AllowCmd::parse(Some(""))).is_empty());
}
