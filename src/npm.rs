//! host の npm を shell を通さず起動する（引数は配列で渡す）。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// npm の起動の設定。既定は親の環境をそのまま引き継ぐ。
/// test は clear_env と envs で、使い捨ての cache・prefix に向ける。
#[derive(Debug, Clone, Default)]
pub struct Npm {
    pub clear_env: bool,
    pub envs: Vec<(OsString, OsString)>,
}

/// `npm pack --json` の返り値（解決した name・version・filename）。
#[derive(Debug, Clone, Deserialize)]
pub struct PackResult {
    pub name: String,
    pub version: String,
    pub filename: String,
}

impl Npm {
    fn command(&self) -> Command {
        let mut c = Command::new("npm");
        if self.clear_env {
            c.env_clear();
            if let Some(path) = std::env::var_os("PATH") {
                c.env("PATH", path);
            }
        }
        for (k, v) in &self.envs {
            c.env(k, v);
        }
        c
    }

    /// npm の cache の場所（`npm config get cache`）。
    pub fn cache_dir(&self) -> Result<PathBuf> {
        let out = self
            .command()
            .args(["config", "get", "cache"])
            .output()
            .context("npm を起動できない")?;
        if !out.status.success() {
            bail!(
                "npm config get cache が失敗: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let s = String::from_utf8(out.stdout)?;
        let s = s.trim();
        if s.is_empty() {
            bail!("npm config get cache の出力が空");
        }
        Ok(PathBuf::from(s))
    }

    /// `npm pack <spec> --json --pack-destination <dest>`（cwd = cwd）。
    pub fn pack(&self, spec: &OsStr, cwd: &Path, dest: &Path) -> Result<PackResult> {
        let out = self
            .command()
            .current_dir(cwd)
            .args(["pack", "--json", "--pack-destination"])
            .arg(dest)
            .arg("--")
            .arg(spec)
            .output()
            .context("npm を起動できない")?;
        if !out.status.success() {
            bail!(
                "npm pack {} が失敗 ({}): {}",
                spec.to_string_lossy(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        parse_pack_json(&String::from_utf8_lossy(&out.stdout))
    }
}

/// lifecycle script の出力が stdout に混ざる事があるので、全体で読めなければ末尾の JSON 配列を探す。
fn parse_pack_json(stdout: &str) -> Result<PackResult> {
    let parse = |s: &str| serde_json::from_str::<Vec<PackResult>>(s.trim());
    let list = parse(stdout).or_else(|first| {
        let start = stdout.rfind("\n[").map(|i| i + 1).ok_or(first)?;
        parse(&stdout[start..])
    });
    let mut list = list.context("npm pack --json の出力を読めない")?;
    if list.is_empty() {
        bail!("npm pack --json の出力が空");
    }
    Ok(list.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json() {
        let r = parse_pack_json(
            r#"[{"name":"a","version":"1.0.0","filename":"a-1.0.0.tgz","files":[]}]"#,
        )
        .unwrap();
        assert_eq!((r.name.as_str(), r.version.as_str()), ("a", "1.0.0"));
    }

    #[test]
    fn parses_json_after_script_noise() {
        let r = parse_pack_json(
            "building...\n[{\"name\":\"a\",\"version\":\"1.0.0\",\"filename\":\"f.tgz\"}]\n",
        )
        .unwrap();
        assert_eq!(r.filename, "f.tgz");
    }
}
