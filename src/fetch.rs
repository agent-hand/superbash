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

/// `workdir`（既存の空 dir）に tarball を取り、`workdir/extract/package` に展開する。
pub fn fetch_toolset(npm: &Npm, toolset: &str, workdir: &Path) -> Result<FetchedToolset> {
    let packed = npm.pack(toolset.as_ref(), workdir, workdir)?;
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
