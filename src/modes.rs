//! mode 振り分けの入口。MCP server 本体と npm link は次のまとまり。

use std::fs;

use anyhow::{Context, Result};

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

/// MCP の mode（既定）。stdio で MCP server として動く。
pub fn mcp(npm: &Npm, toolset: &str, allow: &AllowCmd) -> Result<()> {
    use rmcp::ServiceExt;

    use crate::mcp::exec::Runner;
    use crate::mcp::server::SuperbashServer;

    let generated = prepare(npm, toolset, allow, true)?;
    let config = ToolConfig::load(&generated.package_dir)?;
    let commands: Vec<_> = allow
        .select(&config.commands)
        .into_iter()
        .cloned()
        .collect();
    let runs_dir = npm.cache_dir()?.join("_superbash").join("runs");
    let runner = Runner::new(npm.clone(), generated.tarball, runs_dir);
    let server = SuperbashServer::new(commands, runner);

    let rt = tokio::runtime::Runtime::new().context("tokio を起動できない")?;
    rt.block_on(async {
        let running = server
            .serve(rmcp::transport::stdio())
            .await
            .context("MCP server を起動できない")?;
        running.waiting().await.context("MCP server が異常終了")?;
        Ok(())
    })
}

/// `--only-install`（wrapper 無しで生成し、置き場の dir で npm link する）。
pub fn install(npm: &Npm, toolset: &str, allow: &AllowCmd) -> Result<()> {
    let generated = prepare(npm, toolset, allow, false)?;
    crate::install::link(npm, &generated)
}
