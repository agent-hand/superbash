//! mode 振り分けの入口。MCP server 本体と npm link は次のまとまり。

use std::fs;

use anyhow::{Context, Result, bail};

use crate::cli::AllowCmd;
use crate::config::ToolConfig;
use crate::fetch::fetch_toolset;
use crate::generate::{GenerateInput, Generated, generate};
use crate::npm::Npm;

/// 取得 → 定義の読み込み → package の生成（両 mode 共通）。
pub fn prepare(
    npm: &Npm,
    toolset: &str,
    allow: &AllowCmd,
    with_wrapper: bool,
) -> Result<Generated> {
    let root = npm.cache_dir()?.join("_superbash");
    fs::create_dir_all(&root)?;
    let work = root.join(format!(".fetch-{}", std::process::id()));
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    fs::create_dir_all(&work)?;

    let result = (|| {
        let fetched = fetch_toolset(npm, toolset, &work)?;
        let config = ToolConfig::load(&fetched.dir)?;
        for n in allow.unknown_names(&config.commands) {
            eprintln!("superbash: --allow-cmd の {n} は toolset に無い");
        }
        generate(
            npm,
            &GenerateInput {
                toolset_dir: &fetched.dir,
                name: &fetched.name,
                version: &fetched.version,
                config: &config,
                allow,
                with_wrapper,
                root: &root,
            },
        )
    })();
    fs::remove_dir_all(&work).context("取得の作業 dir を消せない")?;
    result
}

/// MCP の mode（既定）。server 本体は次のまとまり。
pub fn mcp(npm: &Npm, toolset: &str, allow: &AllowCmd) -> Result<()> {
    let generated = prepare(npm, toolset, allow, true)?;
    bail!(
        "MCP server は未実装（次のまとまり）。生成した tarball: {}",
        generated.tarball.display()
    )
}

/// `--only-install`。npm link は次のまとまり。
pub fn install(npm: &Npm, toolset: &str, allow: &AllowCmd) -> Result<()> {
    let generated = prepare(npm, toolset, allow, false)?;
    bail!(
        "npm link は未実装（次のまとまり）。生成した package: {}",
        generated.package_dir.display()
    )
}
