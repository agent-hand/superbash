//! 生成した package の tarball の path が、中身（toolset の code・--allow-cmd・mode）で決まる事。
//! toolset の名前と版が同じでも中身が変われば path が変わり、npx の cache の古い写しを使わない。

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use serde_json::json;
use superbash::cli::AllowCmd;
use superbash::config::ToolConfig;
use superbash::fetch::fetch_toolset;
use superbash::generate::{GenerateInput, generate};

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo-tools")
}

/// toolset（dir）を取得して生成し、tarball の path を返す。root は sb の同じ置き場を使い回す。
fn gen_tarball(sb: &Sandbox, toolset: &Path, allow: AllowCmd, with_wrapper: bool) -> PathBuf {
    let npm = sb.npm();
    let work = sb.dir("fetch-work");
    let f = fetch_toolset(&npm, toolset.to_str().unwrap(), &work).unwrap();
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
    fs::remove_dir_all(&work).unwrap();
    g.tarball
}

#[test]
fn same_content_same_path() {
    let sb = Sandbox::new("cf-same");
    let a = gen_tarball(&sb, &demo(), AllowCmd::All, true);
    let b = gen_tarball(&sb, &demo(), AllowCmd::All, true);
    assert_eq!(a, b);
    // 順序の違うだけの指定でも、実行できる範囲が同じなら中身も path も同じ
    let c = gen_tarball(&sb, &demo(), AllowCmd::parse(Some("hello,fail,nap")), true);
    let d = gen_tarball(&sb, &demo(), AllowCmd::parse(Some("nap, hello,fail")), true);
    assert_eq!(c, d);
    assert_eq!(a, c);
    // 範囲は変わっても使う tool が同じなら package の中身は同じ（demo は tool が 1 つ）
    let e = gen_tarball(&sb, &demo(), AllowCmd::parse(Some("hello")), true);
    assert_eq!(a, e);
}

#[test]
fn allow_cmd_changes_content_and_path() {
    // example-tools は command ごとに別の tool を使う（範囲が変わると bin と tools/ の中身が変わる）
    let sb = Sandbox::new("cf-allow");
    let ex = fixture();
    let all = gen_tarball(&sb, &ex, AllowCmd::All, true);
    let some = gen_tarball(&sb, &ex, AllowCmd::parse(Some("count")), true);
    let none = gen_tarball(&sb, &ex, AllowCmd::parse(Some("")), true);
    assert_ne!(all, some);
    assert_ne!(all, none);
    assert_ne!(some, none);
    // 生成した中身（package.json の bin の範囲）も違う
    let pj =
        |t: &Path| fs::read_to_string(t.parent().unwrap().join("package/package.json")).unwrap();
    assert_ne!(pj(&all), pj(&some));
    assert_ne!(pj(&some), pj(&none));
    // 古い物は残り、消さない
    assert!(all.is_file() && some.is_file() && none.is_file());
}

#[test]
fn mode_changes_path() {
    let sb = Sandbox::new("cf-mode");
    let mcp = gen_tarball(&sb, &demo(), AllowCmd::All, true);
    let install = gen_tarball(&sb, &demo(), AllowCmd::All, false);
    assert_ne!(mcp, install);
}

#[test]
fn tool_code_change_with_same_name_and_version_changes_path() {
    let sb = Sandbox::new("cf-code");
    let ts = sb.root.join("ts");
    copy_dir(&demo(), &ts);
    let before = gen_tarball(&sb, &ts, AllowCmd::All, true);
    let again = gen_tarball(&sb, &ts, AllowCmd::All, true);
    assert_eq!(before, again);

    let main = ts.join("tools/demo-cli/main.js");
    let code = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        code.replace("hello ${rest[0]}", "hello v2 ${rest[0]}"),
    )
    .unwrap();
    let after = gen_tarball(&sb, &ts, AllowCmd::All, true);
    assert_ne!(before, after);
    // 名前と版は同じ（package.json は同じ）
    let name = |t: &Path| t.file_name().unwrap().to_owned();
    assert_eq!(name(&before), name(&after));
}

#[tokio::test]
async fn mcp_runs_new_code_after_toolset_change_with_shared_npm_cache() {
    // 注: この環境の npm（10.9）では、同じ path の tarball の中身が変わっても npx は入れ直すため、
    // 旧実装（path が同じ）でもこの test は通る。path が変わる事の保証は上の path の test が担う。
    // この test は、npm の cache を使い回した 2 回目の起動で新しい code が走る事（挙動）を見る。
    let sb = Sandbox::new("cf-mcp");
    let ts = sb.root.join("ts");
    copy_dir(&demo(), &ts);
    let main = ts.join("tools/demo-cli/main.js");

    // 1 回目: npm の cache に写しができる
    let c = start(&sb, &ts, None).await;
    let r = call(&c, json!({"name": "hello", "args": {"who": "x"}})).await;
    assert!(!is_error(&r), "{}", text(&r));
    assert!(text(&r).contains("hello x") && !text(&r).contains("v2"));
    c.cancel().await.unwrap();

    // 名前と版は同じまま code だけ変える。npm の cache（sb の同じ物）は使い回す
    let code = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        code.replace("hello ${rest[0]}", "hello v2 ${rest[0]}"),
    )
    .unwrap();
    let c = start(&sb, &ts, None).await;
    let r = call(&c, json!({"name": "hello", "args": {"who": "x"}})).await;
    assert!(!is_error(&r), "{}", text(&r));
    assert!(
        text(&r).contains("hello v2 x"),
        "古い code が走った: {}",
        text(&r)
    );
    c.cancel().await.unwrap();
}

/// 同じ toolset の生成を同時に走らせても、どれも成功し、同じ tarball の path を得て、tarball が壊れていない。
#[test]
fn concurrent_generation_shares_one_complete_tarball() {
    const N: usize = 6;
    let sb = Sandbox::new("cf-concurrent");
    let root = sb.dir("root");
    let barrier = std::sync::Barrier::new(N);

    let results: Vec<PathBuf> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let (sb, root, barrier) = (&sb, &root, &barrier);
                s.spawn(move || {
                    let npm = sb.npm();
                    let work = sb.dir(&format!("fetch-work-{i}"));
                    let f = fetch_toolset(&npm, demo().to_str().unwrap(), &work).unwrap();
                    let cfg = ToolConfig::load(&f.dir).unwrap();
                    barrier.wait();
                    let g = generate(
                        &npm,
                        &GenerateInput {
                            toolset_dir: &f.dir,
                            name: &f.name,
                            version: &f.version,
                            config: &cfg,
                            allow: &AllowCmd::All,
                            with_wrapper: true,
                            root,
                        },
                    )
                    .unwrap();
                    // 生成した直後に、その tarball と package が読める（途中の物でない）
                    assert_readable_tarball(&g.tarball);
                    assert!(g.package_dir.join("package.json").is_file());
                    g.tarball
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    assert!(results.iter().all(|t| *t == results[0]), "{results:?}");
    assert_readable_tarball(&results[0]);
    // key の dir は 1 つだけ。作業 dir（.stage-*）は残らない
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        names.iter().all(|n| !n.starts_with(".stage-")),
        "作業 dir が残った: {names:?}"
    );
    assert_eq!(names.len(), 1, "{names:?}");

    // その tarball から npx で起動できる
    let out = sb.npx(&results[0], "demo-cli", &["hello", "x"]);
    assert!(
        out.status.success() && String::from_utf8_lossy(&out.stdout).contains("hello x"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// tarball が gzip + tar として最後まで読め、package/package.json を含む。
fn assert_readable_tarball(tgz: &Path) {
    let f = fs::File::open(tgz).unwrap();
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut names = Vec::new();
    for e in ar.entries().unwrap() {
        let mut e = e.unwrap();
        std::io::copy(&mut e, &mut std::io::sink()).unwrap();
        names.push(e.path().unwrap().to_string_lossy().into_owned());
    }
    assert!(
        names.iter().any(|n| n == "package/package.json"),
        "{names:?}"
    );
}
