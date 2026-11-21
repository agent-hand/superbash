#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use superbash::npm::Npm;

pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/example-tools")
}

/// test ごとの使い捨ての dir（target の下）。home・npm の cache・prefix をここに置く。
pub struct Sandbox {
    pub root: PathBuf,
}

impl Sandbox {
    pub fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("home")).unwrap();
        Sandbox { root }
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("npm-cache")
    }

    pub fn env(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            ("HOME", self.root.join("home")),
            ("npm_config_cache", self.cache()),
            ("npm_config_prefix", self.root.join("prefix")),
            ("npm_config_userconfig", self.root.join("home/.npmrc")),
            ("npm_config_update_notifier", PathBuf::from("false")),
        ]
    }

    /// 環境を引き継がず、使い捨ての cache・prefix に向けた npm。
    pub fn npm(&self) -> Npm {
        Npm {
            clear_env: true,
            envs: self
                .env()
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// `npx --yes --package=<tgz> -- <bin> <args...>` を shell を通さず起動する。
    pub fn npx(&self, tgz: &Path, bin: &str, args: &[&str]) -> std::process::Output {
        let mut c = Command::new("npx");
        c.env_clear().env("PATH", std::env::var_os("PATH").unwrap());
        for (k, v) in self.env() {
            c.env(k, v);
        }
        c.current_dir(&self.root)
            .arg("--yes")
            .arg(format!("--package={}", tgz.display()))
            .arg("--")
            .arg(bin)
            .args(args)
            .output()
            .unwrap()
    }
}

pub fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            fs::copy(e.path(), &to).unwrap(); // 実行の権限も写る
        }
    }
}

pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// fixture から git の bare repo を作り、`git+file://` の指定を返す。
pub fn make_bare_repo(sb: &Sandbox) -> String {
    let work = sb.root.join("repo-work");
    copy_dir(&fixture(), &work);
    git(&work, &["init", "-q"]);
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "fixture"]);
    let bare = sb.root.join("repo.git");
    git(
        &sb.root,
        &[
            "clone",
            "-q",
            "--bare",
            work.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    format!("git+file://{}", bare.display())
}
