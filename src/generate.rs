//! package の生成（設計 2 章「package の生成」）。
//!
//! 置き場（npm の cache の下の `_superbash/`）に
//! `package.json`・`tools/<name>/`・`.manifest/toolconfig.json`・wrapper を組み、npm pack で tarball にする。

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::cli::AllowCmd;
use crate::config::{TOOLCONFIG_PATH, ToolConfig};
use crate::npm::Npm;

/// wrapper の bin の名前（MCP の mode だけ入る）。
pub const WRAPPER_BIN: &str = "superbash-run";
const WRAPPER_PATH: &str = "wrapper/run.js";
const WRAPPER_SRC: &str = include_str!("../wrapper/run.js");

pub struct GenerateInput<'a> {
    /// 展開した toolset の root
    pub toolset_dir: &'a Path,
    pub name: &'a str,
    pub version: &'a str,
    pub config: &'a ToolConfig,
    pub allow: &'a AllowCmd,
    /// MCP の mode は true、only-install は false
    pub with_wrapper: bool,
    /// `<npm cache>/_superbash`
    pub root: &'a Path,
}

#[derive(Debug, Clone)]
pub struct Generated {
    /// 生成した package の dir（only-install はここで npm link する）
    pub package_dir: PathBuf,
    /// npm pack で作った tarball（npx に渡す）
    pub tarball: PathBuf,
    /// package.json の bin
    pub bins: BTreeMap<String, String>,
}

pub fn generate(npm: &Npm, input: &GenerateInput) -> Result<Generated> {
    let selected = input.allow.select(&input.config.commands);

    // bin は、実行できる command が使う tool ごとに 1 つ。
    let mut bins: BTreeMap<String, String> = BTreeMap::new();
    for c in &selected {
        let entry = &input.config.tools[&c.tool].entry;
        bins.insert(c.tool.clone(), format!("tools/{}/{}", c.tool, entry));
    }
    if input.with_wrapper {
        if bins.contains_key(WRAPPER_BIN) {
            bail!("tool の名前が wrapper の bin と衝突: {WRAPPER_BIN}");
        }
        bins.insert(WRAPPER_BIN.into(), WRAPPER_PATH.into());
    }

    let key = key_dir_name(
        input,
        &selected.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
    );
    let key_dir = input.root.join(key);
    if key_dir.exists() {
        fs::remove_dir_all(&key_dir)
            .with_context(|| format!("{} を作り直せない", key_dir.display()))?;
    }
    let pkg = key_dir.join("package");
    fs::create_dir_all(&pkg)?;

    // tools/<name>/
    for tool in bins_tools(&selected) {
        let src = input.toolset_dir.join("tools").join(&tool);
        if !src.is_dir() {
            bail!("toolset に tools/{tool}/ が無い");
        }
        copy_dir(&src, &pkg.join("tools").join(&tool))?;
    }

    // .manifest/toolconfig.json（写す）
    let dst = pkg.join(TOOLCONFIG_PATH);
    fs::create_dir_all(dst.parent().unwrap())?;
    fs::copy(input.toolset_dir.join(TOOLCONFIG_PATH), &dst)?;

    // wrapper
    if input.with_wrapper {
        let w = pkg.join(WRAPPER_PATH);
        fs::create_dir_all(w.parent().unwrap())?;
        fs::write(&w, WRAPPER_SRC)?;
        fs::set_permissions(&w, fs::Permissions::from_mode(0o755))?;
    }

    // package.json（dependencies は toolset の package.json から写す）
    let mut pkg_json = json!({
        "name": input.name,
        "version": input.version,
        "bin": bins,
    });
    let deps = toolset_dependencies(input.toolset_dir)?;
    if let Some(deps) = deps {
        pkg_json["dependencies"] = deps;
    }
    fs::write(
        pkg.join("package.json"),
        serde_json::to_string_pretty(&pkg_json)? + "\n",
    )?;

    let packed = npm.pack(".".as_ref(), &pkg, &key_dir)?;
    let tarball = key_dir.join(&packed.filename);
    if !tarball.is_file() {
        bail!("npm pack の tarball が無い: {}", tarball.display());
    }
    Ok(Generated {
        package_dir: pkg,
        tarball,
        bins,
    })
}

fn bins_tools(selected: &[&crate::config::Command]) -> Vec<String> {
    let mut v: Vec<String> = selected.iter().map(|c| c.tool.clone()).collect();
    v.sort();
    v.dedup();
    v
}

fn toolset_dependencies(toolset_dir: &Path) -> Result<Option<Value>> {
    let path = toolset_dir.join("package.json");
    let text =
        fs::read_to_string(&path).with_context(|| format!("{} を読めない", path.display()))?;
    let v: Value =
        serde_json::from_str(&text).with_context(|| format!("{} が不正", path.display()))?;
    Ok(v.get("dependencies")
        .cloned()
        .filter(|d| d.as_object().is_some_and(|o| !o.is_empty())))
}

/// 置き場の下の dir 名。同じ toolset でも mode と command の範囲が違えば別の dir にする。
fn key_dir_name(input: &GenerateInput, selected: &[&str]) -> String {
    let mode = if input.with_wrapper { "mcp" } else { "install" };
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a
    for part in std::iter::once(mode).chain(selected.iter().copied()) {
        for b in part.bytes().chain(std::iter::once(0)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    let sanitized: String = format!("{}-{}", input.name, input.version)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._@-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{sanitized}-{mode}-{:08x}", h as u32)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            symlink(fs::read_link(entry.path())?, &to)?;
        } else if ty.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?; // 実行の権限も写る
        }
    }
    Ok(())
}
