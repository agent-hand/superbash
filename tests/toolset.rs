mod common;

use common::*;
use superbash::cli::AllowCmd;
use superbash::config::ToolConfig;
use superbash::fetch::fetch_toolset;
use superbash::generate::{GenerateInput, WRAPPER_BIN, generate};

#[test]
fn fetch_from_dir_path_and_git_bare_repo() {
    let sb = Sandbox::new("fetch");
    let npm = sb.npm();

    let from_dir = fetch_toolset(&npm, fixture().to_str().unwrap(), &sb.dir("w-dir")).unwrap();
    let from_git = fetch_toolset(&npm, &make_bare_repo(&sb), &sb.dir("w-git")).unwrap();

    for f in [&from_dir, &from_git] {
        assert_eq!(f.name, "example-tools");
        assert_eq!(f.version, "1.2.0");
        let cfg = ToolConfig::load(&f.dir).unwrap();
        assert_eq!(cfg.commands.len(), 3);
        assert!(f.dir.join("tools/lint-cli/main.js").is_file());
    }
}

fn gen_bins(name: &str, allow: AllowCmd, with_wrapper: bool) -> Vec<String> {
    let sb = Sandbox::new(name);
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
            allow: &allow,
            with_wrapper,
            root: &sb.dir("root"),
        },
    )
    .unwrap();
    assert!(g.tarball.is_file());
    let pj: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(g.package_dir.join("package.json")).unwrap())
            .unwrap();
    let bin = pj["bin"].as_object().unwrap();
    // bin の指す file は実在し、tools は bin に出た物だけ写る
    for p in bin.values() {
        assert!(g.package_dir.join(p.as_str().unwrap()).is_file(), "{p}");
    }
    assert!(g.package_dir.join(".manifest/toolconfig.json").is_file());
    bin.keys().cloned().collect()
}

#[test]
fn allow_cmd_all_lists_every_tool_bin_and_wrapper() {
    assert_eq!(
        gen_bins("allow-all", AllowCmd::All, true),
        ["count-cli", "lint-cli", WRAPPER_BIN]
    );
}

#[test]
fn allow_cmd_list_limits_bins() {
    // lint と lint-list は同じ tool なので bin は 1 つ
    assert_eq!(
        gen_bins("allow-some", AllowCmd::parse(Some("lint,lint-list")), true),
        ["lint-cli", WRAPPER_BIN]
    );
    assert_eq!(
        gen_bins("allow-count", AllowCmd::parse(Some("count")), true),
        ["count-cli", WRAPPER_BIN]
    );
}

#[test]
fn allow_cmd_empty_has_no_command_bins() {
    assert_eq!(
        gen_bins("allow-none", AllowCmd::parse(Some("")), true),
        [WRAPPER_BIN]
    );
}

#[test]
fn install_package_has_no_wrapper() {
    assert_eq!(
        gen_bins("install", AllowCmd::All, false),
        ["count-cli", "lint-cli"]
    );
    assert!(gen_bins("install-none", AllowCmd::parse(Some("")), false).is_empty());
}

#[test]
fn dependencies_are_copied_from_toolset_package_json() {
    let sb = Sandbox::new("deps");
    let npm = sb.npm();
    let ts = sb.root.join("ts");
    copy_dir(&fixture(), &ts);
    std::fs::write(
        ts.join("package.json"),
        r#"{"name":"example-tools","version":"1.2.0","dependencies":{"left-pad":"1.3.0"},"devDependencies":{"x":"1"}}"#,
    )
    .unwrap();
    let cfg = ToolConfig::load(&ts).unwrap();
    let g = generate(
        &npm,
        &GenerateInput {
            toolset_dir: &ts,
            name: "example-tools",
            version: "1.2.0",
            config: &cfg,
            allow: &AllowCmd::All,
            with_wrapper: true,
            root: &sb.dir("root"),
        },
    )
    .unwrap();
    let pj: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(g.package_dir.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(pj["dependencies"]["left-pad"], "1.3.0");
    assert!(pj.get("devDependencies").is_none());
}
