//! toolset の取得（設計 2 章「toolset の取得」）。
//! `npm pack <toolset> --json` で tarball を取り、展開する。

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use tar::Archive;

use crate::npm::Npm;

#[derive(Debug, Clone)]
pub struct FetchedToolset {
    /// 展開した toolset の root（tarball の `package/`）
    pub dir: PathBuf,
    pub name: String,
    pub version: String,
    pub tarball: PathBuf,
}

/// toolset の指定が local の path なら、相対 path を `base`（superbash を起動した dir）から解いて絶対にする。
/// local の判定は npm に合わせる: `.` で始まる物（`./` `../`）、`~/`、`/`、`file:`。
/// `~/` と絶対 path、`file:` の絶対（`file:/x` `file:///x` `file:~/x`）はそのまま。
/// `file:x` `file:`（のみ）と `file:/../x` `file:///./x` は相対として解く。
/// それ以外（`github:`・`user/repo`・`name@版` など）も変えない。
pub fn resolve_toolset(spec: &str, base: &Path) -> String {
    if let Some(rest) = spec.strip_prefix("file:") {
        // `file:` の直後が `/` 無しなら、`~`・`~/` 以外は相対（`file:x` `file:` も起動 dir 基点）。
        // `/{1,3}` の後に `.`・`..` が続く形は npm が相対として扱う（`file:/../x` は `file:../x` 相当）。
        let trimmed = rest.trim_start_matches('/');
        let slashes = rest.len() - trimmed.len();
        let relative = if slashes == 0 {
            rest != "~" && !rest.starts_with("~/")
        } else {
            slashes <= 3 && is_dot_relative(trimmed)
        };
        if relative {
            return format!("file:{}", base.join(trimmed).display());
        }
        return spec.to_string();
    }
    if spec.starts_with('.') {
        return base.join(spec).display().to_string();
    }
    spec.to_string()
}

/// `.` か `..` で、`/` か末尾が続く形（`./x` `../x` `.` `..`）。
fn is_dot_relative(p: &str) -> bool {
    let first = p.split('/').next().unwrap_or("");
    first == "." || first == ".."
}

/// `workdir`（既存の空 dir）に tarball を取り、`workdir/extract/package` に展開する。
/// 相対 path の toolset は、npm pack の cwd（workdir）ではなく起動した dir から解く。
pub fn fetch_toolset(npm: &Npm, toolset: &str, workdir: &Path) -> Result<FetchedToolset> {
    let cwd = std::env::current_dir().context("起動した dir を取れない")?;
    let spec = resolve_toolset(toolset, &cwd);
    let packed = npm.pack(spec.as_ref(), workdir, workdir)?;
    let tarball = workdir.join(&packed.filename);
    let extract = workdir.join("extract");
    fs::create_dir_all(&extract)?;
    let gz = File::open(&tarball).with_context(|| format!("{} を開けない", tarball.display()))?;
    Archive::new(GzDecoder::new(gz))
        .unpack(&extract)
        .with_context(|| format!("{} を展開できない", tarball.display()))?;
    let dir = extract.join("package");
    if !dir.is_dir() {
        bail!("tarball に package/ が無い: {}", tarball.display());
    }
    Ok(FetchedToolset {
        dir,
        name: packed.name,
        version: packed.version,
        tarball,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        resolve_toolset(s, Path::new("/work/proj"))
    }

    #[test]
    fn relative_paths_are_resolved_from_base() {
        assert_eq!(r("./probe-tools"), "/work/proj/./probe-tools");
        assert_eq!(r("../probe-tools"), "/work/proj/../probe-tools");
        assert_eq!(r("."), "/work/proj/.");
        assert_eq!(r("file:./x"), "file:/work/proj/./x");
        assert_eq!(r("file:../x"), "file:/work/proj/../x");
        assert_eq!(r("file:/./x"), "file:/work/proj/./x");
        assert_eq!(r("file:/../x"), "file:/work/proj/../x");
        assert_eq!(r("file:///../x"), "file:/work/proj/../x");
        assert_eq!(r("file:.."), "file:/work/proj/..");
        assert_eq!(r(".hid"), "/work/proj/.hid");
        assert_eq!(r("./.hid"), "/work/proj/./.hid");
        assert_eq!(r("file:x"), "file:/work/proj/x");
        assert_eq!(r("file:"), "file:/work/proj/");
        assert_eq!(r("file:///./x"), "file:/work/proj/./x");
    }

    #[test]
    fn absolute_and_non_local_specs_are_unchanged() {
        for s in [
            "/abs/tools",
            "~/tools",
            "file:/abs/x",
            "file:///abs/x",
            "~",
            "file:/.hid",
            "file:////../x",
            "file:~/x",
            "github:agent-hand/superbash-bantou",
            "agent-hand/superbash-bantou",
            "@example/tools@1.2.0",
            "tools@1.0.0",
            "tools",
        ] {
            assert_eq!(r(s), s);
        }
    }
}
