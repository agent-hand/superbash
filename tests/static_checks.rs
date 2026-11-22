//! source に shell を通す起動が無い事（設計 9 章「静的な検査」）。

use std::fs;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else if matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("rs" | "js" | "mjs")
        ) {
            out.push(p);
        }
    }
}

#[test]
fn no_shell_invocation_in_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut all = Vec::new();
    files(&root.join("src"), &mut all);
    files(&root.join("wrapper"), &mut all);
    assert!(all.len() >= 8, "検査の対象が少なすぎる: {all:?}");

    let forbidden = [
        "\"sh\"",
        "'sh'",
        "\"bash\"",
        "'bash'",
        "\"zsh\"",
        "/bin/sh",
        "/bin/bash",
        "\"-c\"",
        "'-c'",
        "shell: true",
        "shell:true",
        "execSync",
        "execFile(",
        "child_process.exec(",
        ".exec(",
        "system(",
        "popen(",
    ];
    for f in all {
        let text = fs::read_to_string(&f).unwrap();
        for pat in forbidden {
            assert!(
                !text.contains(pat),
                "{} に shell を通す起動の疑い: {pat}",
                f.display()
            );
        }
    }
}

#[test]
fn schema_is_valid_json_and_fixture_keys_are_declared() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("toolconfig.schema.json")).unwrap())
            .unwrap();
    let top = schema["properties"].as_object().unwrap();
    let fixture: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("tests/fixtures/example-tools/.manifest/toolconfig.json"))
            .unwrap(),
    )
    .unwrap();
    for k in fixture.as_object().unwrap().keys() {
        assert!(top.contains_key(k), "schema に {k} が無い");
    }
}
