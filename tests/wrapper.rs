mod common;

use common::*;
use superbash::cli::AllowCmd;
use superbash::config::ToolConfig;
use superbash::fetch::fetch_toolset;
use superbash::generate::{GenerateInput, WRAPPER_BIN, generate};

fn tarball(sb: &Sandbox) -> std::path::PathBuf {
    let npm = sb.npm();
    let f = fetch_toolset(&npm, fixture().to_str().unwrap(), &sb.dir("w")).unwrap();
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

fn run(
    sb: &Sandbox,
    tgz: &std::path::Path,
    name: &str,
    args: &str,
) -> (Vec<String>, std::process::Output) {
    let out = sb.npx(tgz, WRAPPER_BIN, &[name, args]);
    let argv = std::str::from_utf8(&out.stdout)
        .ok()
        .and_then(|s| serde_json::from_str(s.trim()).ok())
        .unwrap_or_default();
    (argv, out)
}

#[test]
fn argv_is_built_by_spec_in_and_special_values_stay_one_arg() {
    let sb = Sandbox::new("wrapper");
    let tgz = tarball(&sb);

    // 設計 3.3 の例
    let (argv, out) = run(&sb, &tgz, "lint", r#"{"fix":true,"target":"src"}"#);
    assert_eq!(
        argv,
        ["check", "--fix", "--", "src"],
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // 3.3 の例の command 名は lint-cli: bin は tool 名で入っている
    let (argv, _) = run(&sb, &tgz, "lint", r#"{}"#);
    assert_eq!(argv, ["check"]);
    let (argv, _) = run(&sb, &tgz, "lint", r#"{"fix":false}"#);
    assert_eq!(argv, ["check"]);
    // 同じ tool の別の command
    let (argv, _) = run(&sb, &tgz, "lint-list", "{}");
    assert_eq!(argv, ["list"]);
    // options は定義の順、string・number は flag と値の 2 引数、positionals は定義の順
    let (argv, _) = run(
        &sb,
        &tgz,
        "count",
        r#"{"more":"m","text":"t","label":"L","limit":5}"#,
    );
    assert_eq!(
        argv,
        ["tally", "--limit", "5", "--label", "L", "--", "t", "m"]
    );

    for v in ["a b", "--x", ";ls", "$(id)", "`id`", "a'b", "*", "x\"y"] {
        let args = serde_json::json!({ "target": v, "fix": true }).to_string();
        let (argv, out) = run(&sb, &tgz, "lint", &args);
        assert_eq!(
            argv,
            ["check", "--fix", "--", v],
            "value {v:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // 値は option の値としても 1 引数のまま
        let args = serde_json::json!({ "label": v, "text": v }).to_string();
        let (argv, _) = run(&sb, &tgz, "count", &args);
        assert_eq!(argv, ["tally", "--label", v, "--", v], "value {v:?}");
    }
}

#[test]
fn bad_calls_fail_with_usage_exit_code() {
    let sb = Sandbox::new("wrapper-bad");
    let tgz = tarball(&sb);
    for (name, args) in [
        ("nope", "{}"),
        ("lint", r#"{"unknown":1}"#),
        ("count", "{}"),
        ("count", r#"{"text":1}"#),
        ("lint", "not json"),
    ] {
        let (_, out) = run(&sb, &tgz, name, args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{name} {args}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
    }
}
