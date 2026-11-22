//! sandbox の中での実行（設計 2 章「sandbox の中での実行」。`--sandbox`）。
//!
//! 絞るのは書き込みとネットワークだけ（読み取りは絞らない）。
//! macOS は `/usr/bin/sandbox-exec` に生成した profile（SBPL）を渡し、Linux は PATH の bwrap（bubblewrap）に
//! 引数を渡す。どちらも引数の配列で組み、shell を通さない。path の展開（`~`・`$TMPDIR`・`.`）は superbash が行う。
//! 組んだ物は「tool の process の前に置く引数の列」（prefix）で、wrapper が `prefix + program + argv` を起動する。

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::config::{SandboxDef, ToolDef};

/// wrapper に prefix（JSON の文字列の配列）を渡す環境変数。
pub const SANDBOX_ENV: &str = "SUPERBASH_SANDBOX";

/// macOS の sandbox-exec（固定の path）。
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// 方針の path の書き方（`~`・`$TMPDIR`・`.` で始める、または絶対 path。`..` は持てない）。
pub fn check_path_form(p: &str) -> Result<()> {
    let ok = p.starts_with('/')
        || ["~", "$TMPDIR", "."]
            .iter()
            .any(|h| p == *h || p.strip_prefix(h).is_some_and(|r| r.starts_with('/')));
    if !ok {
        bail!("path は ~・$TMPDIR・. で始めるか絶対 path: {p:?}");
    }
    if p.contains('\0') || Path::new(p).components().any(|c| c == Component::ParentDir) {
        bail!("path に .. や NUL は持てない: {p:?}");
    }
    Ok(())
}

/// 展開に使う値。
#[derive(Debug, Clone)]
pub struct PathEnv {
    pub home: Option<PathBuf>,
    /// `$TMPDIR`（無ければ /tmp）
    pub tmpdir: PathBuf,
    /// superbash の作業 dir（`.`）
    pub cwd: PathBuf,
}

impl PathEnv {
    pub fn from_process() -> Result<Self> {
        Self::from_values(
            std::env::var_os("HOME").as_deref(),
            std::env::var_os("TMPDIR").as_deref(),
            std::env::current_dir().context("作業 dir を知れない")?,
        )
    }

    /// HOME・TMPDIR は、無ければ（HOME は展開の時に誤り、TMPDIR は /tmp）。在るのに空・相対 path なら、
    /// path の展開に使わず理由つきの誤り（絶対 path を要求する）。
    pub fn from_values(home: Option<&OsStr>, tmpdir: Option<&OsStr>, cwd: PathBuf) -> Result<Self> {
        let abs = |name: &str, v: &OsStr| -> Result<PathBuf> {
            let p = PathBuf::from(v);
            if !p.is_absolute() {
                bail!(
                    "--sandbox: {name} は絶対 path にして（空・相対 path は展開に使えない）: {v:?}"
                );
            }
            Ok(p)
        };
        Ok(PathEnv {
            home: home.map(|h| abs("HOME", h)).transpose()?,
            tmpdir: tmpdir
                .map(|t| abs("TMPDIR", t))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from("/tmp")),
            cwd,
        })
    }

    /// `~`・`$TMPDIR`・`.` を展開する（絶対 path はそのまま）。
    pub fn expand(&self, p: &str) -> Result<PathBuf> {
        check_path_form(p)?;
        if p.starts_with('/') {
            return Ok(PathBuf::from(p));
        }
        let (base, rest) = if let Some(r) = p.strip_prefix('~') {
            let home = self
                .home
                .as_ref()
                .with_context(|| format!("HOME が無く {p} を展開できない"))?;
            (home.as_path(), r)
        } else if let Some(r) = p.strip_prefix("$TMPDIR") {
            (self.tmpdir.as_path(), r)
        } else {
            (self.cwd.as_path(), &p[1..])
        };
        let rest = rest.trim_start_matches('/');
        Ok(if rest.is_empty() {
            base.to_path_buf()
        } else {
            base.join(rest)
        })
    }
}

/// 展開した後の方針。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub write: Vec<PathBuf>,
    pub network: bool,
}

/// sandbox の仕組み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mechanism {
    /// macOS の sandbox-exec
    SandboxExec(PathBuf),
    /// Linux の bwrap（bubblewrap）
    Bwrap(PathBuf),
}

impl Mechanism {
    /// この OS の仕組みを探す。無ければ理由つきの誤り。
    pub fn detect() -> Result<Self> {
        Self::detect_for(
            std::env::consts::OS,
            std::env::var_os("PATH").as_deref(),
            Path::new(SANDBOX_EXEC),
        )
    }

    pub fn detect_for(os: &str, path_var: Option<&OsStr>, sandbox_exec: &Path) -> Result<Self> {
        match os {
            "macos" => {
                if sandbox_exec.is_file() {
                    Ok(Mechanism::SandboxExec(sandbox_exec.to_path_buf()))
                } else {
                    bail!("--sandbox: {} が無い", sandbox_exec.display())
                }
            }
            "linux" => {
                let found = path_var.and_then(|p| {
                    // 空の要素・相対の要素は捨てる（cwd の bwrap を拾わない）。絶対 path の要素だけから探す
                    std::env::split_paths(p)
                        .filter(|d| d.is_absolute())
                        .map(|d| d.join("bwrap"))
                        .find(|f| f.is_file())
                });
                found.map(Mechanism::Bwrap).context(
                    "--sandbox: bwrap（bubblewrap）が PATH に無い。入れてから起動して（例: apt install bubblewrap）",
                )
            }
            other => bail!(
                "--sandbox: この OS（{other}）の sandbox の仕組みは無い（macOS と Linux だけ）"
            ),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Mechanism::SandboxExec(_) => "sandbox-exec",
            Mechanism::Bwrap(_) => "bwrap",
        }
    }

    /// 起動の時の確かめ。仕組みはあっても使えない（user namespace が許されない等）なら理由つきの誤り。
    pub fn probe(&self) -> Result<()> {
        let empty = Policy {
            write: Vec::new(),
            network: false,
        };
        let mut argv = self.prefix(&empty);
        argv.push("/usr/bin/true".into());
        let out = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("{} を起動できない", self.name()))?;
        if !out.status.success() {
            bail!(
                "--sandbox: {} が使えない（{}）: {}",
                self.name(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    /// tool の process の前に置く引数の列（先頭が仕組みの実行 file）。
    /// path は、あれば実体（symlink を解決）にし、bwrap は無い path を落とす（bind できない）。
    pub fn prefix(&self, policy: &Policy) -> Vec<OsString> {
        match self {
            Mechanism::SandboxExec(exe) => {
                let write: Vec<PathBuf> = policy.write.iter().map(|p| real_path(p)).collect();
                vec![
                    exe.clone().into_os_string(),
                    "-p".into(),
                    sbpl_profile(&write, policy.network).into(),
                ]
            }
            Mechanism::Bwrap(exe) => {
                let write: Vec<PathBuf> = policy
                    .write
                    .iter()
                    .filter(|p| p.exists())
                    .map(|p| real_path(p))
                    .collect();
                let mut v = vec![exe.clone().into_os_string()];
                v.extend(bwrap_args(&write, policy.network));
                v
            }
        }
    }
}

/// 在る所まで symlink を解決した path（macOS の /tmp → /private/tmp など。SBPL は実体で書く）。
fn real_path(p: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(c) = cur.canonicalize() {
            return tail.iter().rev().fold(c, |acc, n| acc.join(n));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// bwrap の引数（実行 file の後ろ、program の前）。末尾は `--`。
/// 読み取りは絞らない（`--ro-bind / /`）。write の path だけ `--bind`。network false なら `--unshare-net`。
/// `--new-session` で端末から切り離す（端末への入力の注入を防ぐ）。
pub fn bwrap_args(write: &[PathBuf], network: bool) -> Vec<OsString> {
    let mut v: Vec<OsString> = ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]
        .iter()
        .map(OsString::from)
        .collect();
    for p in write {
        v.push("--bind".into());
        v.push(p.clone().into_os_string());
        v.push(p.clone().into_os_string());
    }
    if !network {
        v.push("--unshare-net".into());
    }
    v.push("--new-session".into());
    v.push("--die-with-parent".into());
    v.push("--".into());
    v
}

/// sandbox-exec に渡す profile（SBPL）。既定は許し、書き込みを全部拒み、write の path の下だけ許す。
/// network false なら network を拒む（unix socket は許す）。
pub fn sbpl_profile(write: &[PathBuf], network: bool) -> String {
    let mut s =
        String::from("(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*\n");
    for dev in ["/dev/null", "/dev/zero", "/dev/dtracehelper", "/dev/tty"] {
        s.push_str(&format!("  (literal {})\n", sbpl_str(dev)));
    }
    for p in write {
        s.push_str(&format!("  (subpath {})\n", sbpl_str(&p.to_string_lossy())));
    }
    s.push_str(")\n");
    if !network {
        s.push_str("(deny network*)\n");
        s.push_str("(allow network-outbound (remote unix-socket))\n");
        s.push_str("(allow network-bind (local unix-socket))\n");
        s.push_str("(allow network-inbound (local unix-socket))\n");
    }
    s
}

/// SBPL の文字列（`"` と `\` を逃がす）。
fn sbpl_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// superbash が持つ sandbox の設定（tool ごとの方針を引く）。
#[derive(Debug, Clone)]
pub struct Sandbox {
    mech: Mechanism,
    env: PathEnv,
    /// 項が無い tool の write（superbash の置き場と一時の dir）
    default_write: Vec<PathBuf>,
    tools: BTreeMap<String, Option<SandboxDef>>,
}

impl Sandbox {
    /// `package_dir` は生成した package の dir、`runs_dir` は background の出力の dir。
    pub fn new(
        mech: Mechanism,
        env: PathEnv,
        package_dir: &Path,
        runs_dir: &Path,
        tools: &BTreeMap<String, ToolDef>,
    ) -> Self {
        let mut default_write = vec![package_dir.to_path_buf(), runs_dir.to_path_buf()];
        for t in [
            env.tmpdir.clone(),
            PathBuf::from("/tmp"),
            PathBuf::from("/var/tmp"),
        ] {
            if !default_write.contains(&t) {
                default_write.push(t);
            }
        }
        Sandbox {
            mech,
            env,
            default_write,
            tools: tools
                .iter()
                .map(|(k, t)| (k.clone(), t.sandbox.clone()))
                .collect(),
        }
    }

    pub fn mechanism(&self) -> &Mechanism {
        &self.mech
    }

    /// tool の方針（項が無ければ既定: network false、write は置き場と一時の dir だけ）。
    pub fn policy(&self, tool: &str) -> Result<Policy> {
        match self.tools.get(tool) {
            Some(Some(def)) => Ok(Policy {
                write: def
                    .write
                    .iter()
                    .map(|p| self.env.expand(p))
                    .collect::<Result<_>>()
                    .with_context(|| format!("tools.{tool}.sandbox.write"))?,
                network: def.network,
            }),
            _ => Ok(Policy {
                write: self.default_write.clone(),
                network: false,
            }),
        }
    }

    /// 項のある tool の write で、展開した path が無い物（tool 名, path）。bwrap はこの path を write に入れない
    /// （bind できない）ので、起動の時に利用者へ知らせる。展開できない物は、呼ぶ時の誤りに任せて数えない。
    pub fn missing_write_paths(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        for (name, def) in &self.tools {
            if def.is_none() {
                continue;
            }
            if let Ok(pol) = self.policy(name) {
                out.extend(
                    pol.write
                        .into_iter()
                        .filter(|p| !p.exists())
                        .map(|p| (name.clone(), p)),
                );
            }
        }
        out
    }

    pub fn prefix_for(&self, tool: &str) -> Result<Vec<OsString>> {
        Ok(self.mech.prefix(&self.policy(tool)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> PathEnv {
        PathEnv {
            home: Some("/home/u".into()),
            tmpdir: "/var/tmp/t".into(),
            cwd: "/srv/work".into(),
        }
    }

    #[test]
    fn path_forms() {
        for ok in ["/a", "~", "~/.npm", "$TMPDIR", "$TMPDIR/x", ".", "./out"] {
            assert!(check_path_form(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "a", "~u", ".x", "$HOME", "$TMPDIRX", "../x", "./../x", "~/../x", "/a/../b",
        ] {
            assert!(check_path_form(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn expansion() {
        let e = env();
        let x = |p: &str| e.expand(p).unwrap();
        assert_eq!(x("~"), Path::new("/home/u"));
        assert_eq!(x("~/.npm"), Path::new("/home/u/.npm"));
        assert_eq!(x("$TMPDIR"), Path::new("/var/tmp/t"));
        assert_eq!(x("$TMPDIR/a/b"), Path::new("/var/tmp/t/a/b"));
        assert_eq!(x("."), Path::new("/srv/work"));
        assert_eq!(x("./out"), Path::new("/srv/work/out"));
        assert_eq!(x("/abs/p"), Path::new("/abs/p"));
        let no_home = PathEnv {
            home: None,
            ..env()
        };
        assert!(no_home.expand("~/x").is_err());
        assert!(no_home.expand("/x").is_ok());
    }

    fn strs(v: &[OsString]) -> Vec<String> {
        v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn bwrap_argument_list() {
        let w = [PathBuf::from("/home/u"), PathBuf::from("/tmp")];
        assert_eq!(
            strs(&bwrap_args(&w, false)),
            [
                "--ro-bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--bind",
                "/home/u",
                "/home/u",
                "--bind",
                "/tmp",
                "/tmp",
                "--unshare-net",
                "--new-session",
                "--die-with-parent",
                "--"
            ]
        );
        let open = strs(&bwrap_args(&[], true));
        assert!(!open.contains(&"--unshare-net".to_string()));
        assert!(open.contains(&"--new-session".to_string()));
        assert_eq!(open.last().unwrap(), "--");
        // 空白・引用符を含む path も 1 引数のまま
        let odd = bwrap_args(&[PathBuf::from("/a b/\"c\"")], true);
        assert!(odd.contains(&OsString::from("/a b/\"c\"")));
    }

    #[test]
    fn sbpl_profile_shape() {
        let p = sbpl_profile(
            &[PathBuf::from("/Users/u"), PathBuf::from("/a \"b\"\\c")],
            false,
        );
        assert!(p.starts_with("(version 1)\n(allow default)\n(deny file-write*)\n"));
        assert!(p.contains("(subpath \"/Users/u\")"));
        assert!(p.contains(r#"(subpath "/a \"b\"\\c")"#));
        assert!(p.contains("(literal \"/dev/null\")"));
        assert!(p.contains("(deny network*)"));
        // 読み取りは絞らない
        assert!(!p.contains("file-read"));
        let open = sbpl_profile(&[], true);
        assert!(!open.contains("network"));
        // 括弧の釣り合い
        for s in [&p, &open] {
            assert_eq!(s.matches('(').count(), s.matches(')').count(), "{s}");
        }
    }

    fn tools(json: &str) -> BTreeMap<String, ToolDef> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn policy_default_and_explicit() {
        let t = tools(
            r#"{"plain":{"entry":"m.js"},
                "mac":{"entry":"m.js","sandbox":{"write":["~","$TMPDIR"],"network":true}},
                "none":{"entry":"m.js","sandbox":{}}}"#,
        );
        let sb = Sandbox::new(
            Mechanism::Bwrap("/usr/bin/bwrap".into()),
            env(),
            Path::new("/cache/_superbash/k/package"),
            Path::new("/cache/_superbash/runs"),
            &t,
        );
        // 項が無い: network false、置き場と一時の dir だけ
        let d = sb.policy("plain").unwrap();
        assert!(!d.network);
        let ps: Vec<&str> = d.write.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(
            ps,
            [
                "/cache/_superbash/k/package",
                "/cache/_superbash/runs",
                "/var/tmp/t",
                "/tmp",
                "/var/tmp"
            ]
        );
        // 項がある: 書いた物だけ（展開済み）
        let m = sb.policy("mac").unwrap();
        assert!(m.network);
        assert_eq!(
            m.write,
            [PathBuf::from("/home/u"), PathBuf::from("/var/tmp/t")]
        );
        // 空の項: write 無し・network false
        let n = sb.policy("none").unwrap();
        assert_eq!((n.write.len(), n.network), (0, false));
        // tool 名が config に無い物も既定（fail-closed）
        assert!(!sb.policy("ghost").unwrap().network);
    }

    #[test]
    fn prefix_per_mechanism() {
        let p = Policy {
            write: vec![PathBuf::from("/definitely/missing/dir")],
            network: false,
        };
        // bwrap は無い path を落とす
        let b = Mechanism::Bwrap("/usr/bin/bwrap".into()).prefix(&p);
        let b = strs(&b);
        assert_eq!(b[0], "/usr/bin/bwrap");
        assert!(!b.iter().any(|a| a.contains("missing")));
        assert!(b.contains(&"--unshare-net".to_string()));
        // sandbox-exec は -p と profile の 3 つ
        let s = Mechanism::SandboxExec("/usr/bin/sandbox-exec".into()).prefix(&p);
        assert_eq!(s.len(), 3);
        assert_eq!(s[1], "-p");
        assert!(strs(&s)[2].contains("/definitely/missing/dir"));
    }

    #[test]
    fn home_and_tmpdir_must_be_absolute() {
        let cwd = || PathBuf::from("/w");
        let ok = |h: Option<&str>, t: Option<&str>| {
            PathEnv::from_values(h.map(OsStr::new), t.map(OsStr::new), cwd())
        };
        let e = ok(Some(""), None).unwrap_err().to_string();
        assert!(e.contains("HOME") && e.contains("絶対"), "{e}");
        assert!(ok(Some("rel/home"), None).is_err());
        let e = ok(None, Some("")).unwrap_err().to_string();
        assert!(e.contains("TMPDIR"), "{e}");
        assert!(ok(None, Some("tmp")).is_err());
        // 無い物は誤りにしない（TMPDIR は /tmp、HOME は展開の時に誤り）
        let e = ok(None, None).unwrap();
        assert_eq!((e.home, e.tmpdir), (None, PathBuf::from("/tmp")));
        let e = ok(Some("/h"), Some("/t")).unwrap();
        assert_eq!((e.home, e.tmpdir), (Some("/h".into()), "/t".into()));
    }

    #[test]
    fn missing_write_paths_are_reported_for_explicit_entries() {
        let t = tools(
            r#"{"plain":{"entry":"m.js"},
                "mac":{"entry":"m.js","sandbox":{"write":["~/.nope-sbx","$TMPDIR","/definitely/missing"]}}}"#,
        );
        let sb = Sandbox::new(
            Mechanism::Bwrap("/usr/bin/bwrap".into()),
            PathEnv {
                home: Some("/definitely/home".into()),
                tmpdir: std::env::temp_dir(),
                cwd: "/".into(),
            },
            Path::new("/cache/p"),
            Path::new("/cache/r"),
            &t,
        );
        let m = sb.missing_write_paths();
        assert_eq!(
            m,
            [
                (
                    "mac".to_string(),
                    PathBuf::from("/definitely/home/.nope-sbx")
                ),
                ("mac".to_string(), PathBuf::from("/definitely/missing")),
            ]
        );
    }

    #[test]
    fn detect_ignores_empty_and_relative_path_elements() {
        let none = Path::new("/nonexistent/sandbox-exec");
        let d = std::env::temp_dir().join(format!("sbx-rel-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("bwrap"), "").unwrap();
        // 相対の要素（cwd から見て bwrap が在る）・空の要素は、在っても拾わない
        let cwd = std::env::current_dir().unwrap();
        let rel_to_cwd = pathdiff_rel(&cwd, &d);
        let p = std::env::join_paths([
            PathBuf::from(""),
            rel_to_cwd,
            PathBuf::from("bin"),
            PathBuf::from("/nonexistent"),
        ])
        .unwrap();
        assert!(Mechanism::detect_for("linux", Some(&p), none).is_err());
        // 絶対の要素なら、空・相対の要素が前後に在っても見つかる
        let p = std::env::join_paths([PathBuf::new(), PathBuf::from("x"), d.clone()]).unwrap();
        assert_eq!(
            Mechanism::detect_for("linux", Some(&p), none).unwrap(),
            Mechanism::Bwrap(d.join("bwrap"))
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// `from` から見た `to` の相対 path（`..` を重ねる。test 用）。
    fn pathdiff_rel(from: &Path, to: &Path) -> PathBuf {
        let mut r = PathBuf::new();
        for _ in from.components().skip(1) {
            r.push("..");
        }
        r.join(to.strip_prefix("/").unwrap())
    }

    #[test]
    fn detect_reports_reasons() {
        let none = Path::new("/nonexistent/sandbox-exec");
        let e = Mechanism::detect_for("linux", Some(OsStr::new("/nonexistent")), none).unwrap_err();
        assert!(e.to_string().contains("bwrap"), "{e}");
        assert!(Mechanism::detect_for("linux", None, none).is_err());
        let e = Mechanism::detect_for("macos", None, none).unwrap_err();
        assert!(e.to_string().contains("sandbox-exec"), "{e}");
        let e = Mechanism::detect_for("windows", None, none).unwrap_err();
        assert!(e.to_string().contains("windows"), "{e}");
        // 在れば見つかる
        let d = std::env::temp_dir().join(format!("sbx-detect-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("bwrap"), "").unwrap();
        let m = Mechanism::detect_for("linux", Some(d.as_os_str()), none).unwrap();
        assert_eq!(m, Mechanism::Bwrap(d.join("bwrap")));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
