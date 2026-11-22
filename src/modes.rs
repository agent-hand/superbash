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

/// MCP の mode（既定）。Streamable HTTP の MCP server として `host:port` で待ち受ける（path は /mcp）。
/// `sandbox` が true なら、tool の process を sandbox の中で起こす（仕組みが使えなければ、起動せず誤りで返す）。
pub fn mcp(
    npm: &Npm,
    toolset: &str,
    allow: &AllowCmd,
    host: &str,
    port: u16,
    sandbox: bool,
) -> Result<()> {
    use crate::sandbox::{Mechanism, PathEnv, Sandbox};

    use crate::mcp::exec::Runner;
    use crate::mcp::http;
    use crate::mcp::server::SuperbashServer;

    // HOME・TMPDIR が使えない値なら、待ち受ける前に理由を出して終わる
    let path_env = if sandbox {
        Some(PathEnv::from_process()?)
    } else {
        None
    };
    let mech = if sandbox {
        let m = Mechanism::detect()?;
        m.probe()?;
        eprintln!("superbash: sandbox: {}", m.name());
        Some(m)
    } else {
        None
    };
    let generated = prepare(npm, toolset, allow, true)?;
    let config = ToolConfig::load(&generated.package_dir)?;
    let commands = allow.select(&config.commands);
    let runs_dir = npm.cache_dir()?.join("_superbash").join("runs");
    let mut runner = Runner::new(npm.clone(), generated.tarball, runs_dir.clone());
    if let (Some(mech), Some(path_env)) = (mech, path_env) {
        let sb = Sandbox::new(
            mech,
            path_env,
            &generated.package_dir,
            &runs_dir,
            &config.tools,
        );
        for (tool, p) in sb.missing_write_paths() {
            eprintln!(
                "superbash: sandbox: 警告: tools.{tool}.sandbox.write の {} が無い（この path には書き込めない）",
                p.display()
            );
        }
        runner = runner.with_sandbox(sb, config.commands.clone());
    }
    let server = SuperbashServer::new(commands, runner);

    let rt = tokio::runtime::Runtime::new().context("tokio を起動できない")?;
    rt.block_on(http::serve(server, host, port))
}

/// `--only-install`（wrapper 無しで生成し、置き場の dir で npm link する）。
pub fn install(npm: &Npm, toolset: &str, allow: &AllowCmd) -> Result<()> {
    let generated = prepare(npm, toolset, allow, false)?;
    crate::install::link(npm, &generated)
}
