//! package の生成（設計 2 章「package の生成」）。
//!
//! 置き場（npm の cache の下の `_superbash/`）に
//! `package.json`・`tools/<name>/`・`.manifest/toolconfig.json`・wrapper を組み、npm pack で tarball にする。

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::cli::AllowCmd;
use crate::config::{TOOLCONFIG_PATH, ToolConfig};
use crate::npm::Npm;

/// wrapper の bin の名前（MCP の mode だけ入る）。
pub const WRAPPER_BIN: &str = "superbash-run";
const WRAPPER_PATH: &str = "wrapper/run.js";
const WRAPPER_SRC: &str = include_str!("../wrapper/run.js");
/// 振り分けの file（module のある tool がある時だけ置く。作法 5.4）。
pub const DISPATCH_PATH: &str = "dispatch/run.mjs";
const DISPATCH_SRC: &str = include_str!("../wrapper/dispatch.mjs");

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
        let tool = c.tool_name();
        let def = &input.config.tools[tool];
        // module のある tool の bin は振り分けの file。無ければ entry を直に向ける（互換）。
        let target = if def.module.is_some() {
            DISPATCH_PATH.to_string()
        } else {
            format!("tools/{tool}/{}", def.entry)
        };
        bins.insert(tool.to_string(), target);
    }
    if input.with_wrapper {
        if bins.contains_key(WRAPPER_BIN) {
            bail!("tool の名前が wrapper の bin と衝突: {WRAPPER_BIN}");
        }
        bins.insert(WRAPPER_BIN.into(), WRAPPER_PATH.into());
    }

    // 組み立てと npm pack は、他と衝突しない名前の作業 dir（root 直下の .stage-*）の中で終わらせる。
    // key の dir には、tarball まで出来上がった物だけを rename（不可分）で置く。
    let stage = unique_stage_dir(input.root);
    let pkg = stage.join("package");
    let made = (|| -> Result<(String, Option<PathBuf>)> {
        fs::create_dir_all(&pkg)?;
        build_package(input, &selected, &bins, &pkg)?;
        let hash = hash_dir(&pkg)?;
        let key_dir = input.root.join(key_dir_name(input, &hash));
        if key_dir.is_dir() {
            // 他の process（または前回）が先に置いた物がある。pack せずそれを使う。
            return Ok((hash, None));
        }
        let packed = npm.pack(".".as_ref(), &pkg, &stage)?;
        let tarball = stage.join(&packed.filename);
        if !tarball.is_file() {
            bail!("npm pack の tarball が無い: {}", tarball.display());
        }
        Ok((hash, Some(PathBuf::from(packed.filename))))
    })();
    let (hash, packed_name) = match made {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&stage);
            return Err(e);
        }
    };

    let key_dir = input.root.join(key_dir_name(input, &hash));
    if let Some(name) = packed_name {
        // 置き先が既にある空でない dir なら rename は失敗する。その時は先に置いた物を使う。
        match fs::rename(&stage, &key_dir) {
            Ok(()) => return Ok(placed(&key_dir, &name, bins)),
            Err(e) => {
                let _ = fs::remove_dir_all(&stage);
                if !key_dir.is_dir() {
                    return Err(e).with_context(|| format!("{} に置けない", key_dir.display()));
                }
            }
        }
    } else {
        let _ = fs::remove_dir_all(&stage);
    }
    // 先に置かれていた key の dir を使う（中身は消さない・作り直さない）。
    let tarball = find_tarball(&key_dir)
        .with_context(|| format!("{} に tarball が無い", key_dir.display()))?;
    Ok(Generated {
        package_dir: key_dir.join("package"),
        tarball,
        bins,
    })
}

fn placed(key_dir: &Path, tarball_name: &Path, bins: BTreeMap<String, String>) -> Generated {
    Generated {
        package_dir: key_dir.join("package"),
        tarball: key_dir.join(tarball_name),
        bins,
    }
}

/// 衝突しない作業 dir の名前（pid・時刻・process 内の連番）。dir 自体はまだ作らない。
fn unique_stage_dir(root: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    root.join(format!(
        ".stage-{}-{nanos}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// key_dir にある tarball（npm pack の結果）。key の dir は完成した物しか置かれないので、あれば完全。
fn find_tarball(key_dir: &Path) -> Option<PathBuf> {
    let rd = fs::read_dir(key_dir).ok()?;
    let mut found: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "tgz"))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// 生成する package の中身を pkg に組む。
fn build_package(
    input: &GenerateInput,
    selected: &[crate::config::Command],
    bins: &BTreeMap<String, String>,
    pkg: &Path,
) -> Result<()> {
    // tools/<name>/
    for tool in bins_tools(selected) {
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

    // 振り分けの file（module のある tool が 1 つでもある時）
    if selected
        .iter()
        .any(|c| input.config.tools[c.tool_name()].module.is_some())
    {
        let d = pkg.join(DISPATCH_PATH);
        fs::create_dir_all(d.parent().unwrap())?;
        fs::write(&d, DISPATCH_SRC)?;
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755))?;
    }

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
    Ok(())
}

/// package の dir の中身の hash（sha256 の hex）。
/// 入れる物: 各 entry の相対 path（名前順）・種別・実行の権限・file の中身・symlink の向き先、
/// と superbash 自身の版。package.json（名前・版・bin・dependencies）・tools の code・
/// toolconfig・振り分けの file（module のある tool がある時だけ）・wrapper（MCP の mode だけ）は dir の中身なので、--allow-cmd と mode の違いも中身に出る。
fn hash_dir(root: &Path) -> Result<String> {
    let mut h = Sha256::new();
    h.update(b"superbash\0");
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(b"\0");
    hash_walk(root, Path::new(""), &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn hash_walk(base: &Path, rel: &Path, h: &mut Sha256) -> Result<()> {
    let mut names: Vec<_> = fs::read_dir(base.join(rel))?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<_>>()?;
    names.sort();
    for name in names {
        let rel = rel.join(&name);
        let full = base.join(&rel);
        let meta = fs::symlink_metadata(&full)?;
        let ty = meta.file_type();
        h.update(rel.to_string_lossy().as_bytes());
        h.update(b"\0");
        if ty.is_symlink() {
            h.update(b"l\0");
            h.update(fs::read_link(&full)?.to_string_lossy().as_bytes());
            h.update(b"\0");
        } else if ty.is_dir() {
            h.update(b"d\0");
            hash_walk(base, &rel, h)?;
        } else {
            let exec = meta.permissions().mode() & 0o111 != 0;
            let body = fs::read(&full)?;
            h.update(if exec { b"x\0" } else { b"f\0" });
            h.update((body.len() as u64).to_le_bytes());
            h.update(&body);
        }
    }
    Ok(())
}

fn bins_tools(selected: &[crate::config::Command]) -> Vec<String> {
    let mut v: Vec<String> = selected.iter().map(|c| c.tool_name().to_string()).collect();
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

/// 置き場の下の dir 名: `<name>-<version>-<mode>-<中身の hash の先頭 16 桁>`。
/// 中身が変われば dir 名（= tarball の path）が変わり、npx の cache の古い写しを使わない。
fn key_dir_name(input: &GenerateInput, hash: &str) -> String {
    let mode = if input.with_wrapper { "mcp" } else { "install" };
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
    format!("{sanitized}-{mode}-{}", &hash[..16])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成した中身の hash に、振り分けの file が入る（有る無し・中身の違いで hash が変わる）。
    #[test]
    fn dispatcher_is_part_of_the_content_hash() {
        let dir = std::env::temp_dir().join(format!("sb-hash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tools")).unwrap();
        fs::write(dir.join("package.json"), "{}").unwrap();
        let base = hash_dir(&dir).unwrap();
        assert_eq!(base, hash_dir(&dir).unwrap());

        let d = dir.join(DISPATCH_PATH);
        fs::create_dir_all(d.parent().unwrap()).unwrap();
        fs::write(&d, DISPATCH_SRC).unwrap();
        let with = hash_dir(&dir).unwrap();
        assert_ne!(base, with, "振り分けの file の有無が hash に出ない");

        fs::write(&d, format!("{DISPATCH_SRC}\n// changed")).unwrap();
        assert_ne!(
            with,
            hash_dir(&dir).unwrap(),
            "振り分けの file の中身が hash に出ない"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
