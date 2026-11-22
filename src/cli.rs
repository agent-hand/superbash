//! CLI: `superbash [option...] <toolset>`（設計 2 章「起動と mode」）。

use clap::Parser;

use crate::config::{Command, all_sequences, split_seq};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 7391;

#[derive(Parser, Debug)]
#[command(name = "superbash", version, about)]
pub struct Cli {
    /// 生成した package を置き場に置き、npm link で入れる（MCP は起動しない）
    #[arg(long)]
    pub only_install: bool,

    /// 実行できる command（指定無し=全部、'a,b'=その名前だけ、''=無し）。名前は並びの名前
    /// （'a b c'）で、群の名前ならその下の全部
    #[arg(long, value_name = "CMDS")]
    pub allow_cmd: Option<String>,

    /// tool の process を sandbox の中で起こす（書き込みとネットワークだけを絞る。MCP の mode だけ。
    /// macOS は sandbox-exec、Linux は PATH の bwrap。仕組みが無ければ理由を出して終了する）
    #[arg(long, conflicts_with = "only_install")]
    pub sandbox: bool,

    /// MCP の mode で待ち受ける host（既定 127.0.0.1）
    #[arg(long, default_value = DEFAULT_HOST)]
    pub host: String,

    /// MCP の mode で待ち受ける port（既定 7391。0 なら空き port）
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// toolset（npm に渡して取る。repo か registry の package か）
    pub toolset: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Mcp,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowCmd {
    /// toolset の全 command
    All,
    /// 指定した command だけ（空 = 無し）
    Only(Vec<String>),
}

impl AllowCmd {
    /// 名前は並びの名前（空白で区切る。前後の空白は捨て、続いた空白は 1 つの区切り）。
    pub fn parse(arg: Option<&str>) -> Self {
        match arg {
            None => AllowCmd::All,
            Some(s) => AllowCmd::Only(
                s.split(',')
                    .map(|n| split_seq(n).join(" "))
                    .filter(|n| !n.is_empty())
                    .collect(),
            ),
        }
    }

    /// 葉の並びの名前 `seq` が実行できるか。許す名前が `seq` の先頭の語の並びなら許す
    /// （群の名前ならその下の全部、葉の名前ならその葉だけ）。
    fn allows(&self, seq: &[&str]) -> bool {
        match self {
            AllowCmd::All => true,
            AllowCmd::Only(names) => names.iter().any(|n| seq.starts_with(&split_seq(n))),
        }
    }

    /// toolconfig.json の並び順のまま、実行できる要素だけの木を返す
    /// （実行できる葉を持たない群は落とす）。
    pub fn select(&self, commands: &[Command]) -> Vec<Command> {
        fn prune(a: &AllowCmd, c: &Command, path: &mut Vec<String>) -> Option<Command> {
            path.push(c.name.clone());
            let kept = if c.is_group() {
                let subs: Vec<Command> = c
                    .subcommands
                    .iter()
                    .filter_map(|s| prune(a, s, path))
                    .collect();
                (!subs.is_empty()).then(|| Command {
                    subcommands: subs,
                    ..c.clone()
                })
            } else {
                let seq: Vec<&str> = path.iter().map(String::as_str).collect();
                a.allows(&seq).then(|| c.clone())
            };
            path.pop();
            kept
        }
        commands
            .iter()
            .filter_map(|c| prune(self, c, &mut Vec::new()))
            .collect()
    }

    /// toolset に無い名前（typo の気付き用。拒絶はしない）。群・葉のどの並びの名前とも一致しない物。
    pub fn unknown_names<'a>(&'a self, commands: &[Command]) -> Vec<&'a str> {
        match self {
            AllowCmd::All => Vec::new(),
            AllowCmd::Only(names) => {
                let known = all_sequences(commands);
                names
                    .iter()
                    .filter(|n| !known.contains(n))
                    .map(String::as_str)
                    .collect()
            }
        }
    }
}

impl Cli {
    pub fn mode(&self) -> Mode {
        if self.only_install {
            Mode::Install
        } else {
            Mode::Mcp
        }
    }

    pub fn allow(&self) -> AllowCmd {
        AllowCmd::parse(self.allow_cmd.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("superbash").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn default_is_mcp_all() {
        let c = parse(&["@example/tools@1.2.0"]);
        assert_eq!(c.mode(), Mode::Mcp);
        assert_eq!(c.allow(), AllowCmd::All);
        assert_eq!(c.toolset, "@example/tools@1.2.0");
        assert_eq!((c.host.as_str(), c.port), ("127.0.0.1", 7391));
    }

    #[test]
    fn sandbox_is_off_by_default_and_not_for_only_install() {
        assert!(!parse(&["t"]).sandbox);
        assert!(parse(&["--sandbox", "t"]).sandbox);
        assert!(Cli::try_parse_from(["superbash", "--sandbox", "--only-install", "t"]).is_err());
    }

    #[test]
    fn host_and_port_options() {
        let c = parse(&["--host", "::1", "--port", "0", "t"]);
        assert_eq!((c.host.as_str(), c.port), ("::1", 0));
    }

    #[test]
    fn only_install_and_allow_list() {
        let c = parse(&["--only-install", "t", "--allow-cmd", "a,b"]);
        assert_eq!(c.mode(), Mode::Install);
        assert_eq!(c.allow(), AllowCmd::Only(vec!["a".into(), "b".into()]));
    }

    #[test]
    fn empty_allow_cmd_means_none() {
        let c = parse(&["t", "--allow-cmd", ""]);
        assert_eq!(c.allow(), AllowCmd::Only(vec![]));
    }

    const TREE: &str = r#"[
      {"name":"gh","tool":"x","spec":{"subcommand":"gh"},"subcommands":[
        {"name":"repo","subcommands":[{"name":"create","spec":{}}]},
        {"name":"release","subcommands":[{"name":"create","spec":{}},{"name":"list","spec":{}}]}]},
      {"name":"old","tool":"x","spec":{}}]"#;

    #[test]
    fn allow_group_leaf_and_unknown_names() {
        let t: Vec<Command> = serde_json::from_str(TREE).unwrap();
        let sel = |a: &str| all_sequences(&AllowCmd::parse(Some(a)).select(&t));
        // 群: その下の全部
        assert_eq!(
            sel("gh release"),
            ["gh", "gh release", "gh release create", "gh release list"]
        );
        // 葉: その葉だけ（空白は正規化）
        assert_eq!(
            sel("  gh   release  list "),
            ["gh", "gh release", "gh release list"]
        );
        // 上の要素: 下の全部
        assert_eq!(sel("gh").len(), 6);
        // 複数
        assert_eq!(
            sel("gh repo create,old"),
            ["gh", "gh repo", "gh repo create", "old"]
        );
        // 知らない名前は何も選ばず、警告の対象になる
        assert!(sel("nope,gh nope").is_empty());
        let a = AllowCmd::parse(Some("nope,gh release,gh release x,old"));
        assert_eq!(a.unknown_names(&t), ["nope", "gh release x"]);
        // 全部・無し
        assert_eq!(all_sequences(&AllowCmd::All.select(&t)).len(), 7);
        assert!(AllowCmd::parse(Some("")).select(&t).is_empty());
    }

    #[test]
    fn toolset_is_required_and_single() {
        assert!(Cli::try_parse_from(["superbash"]).is_err());
        assert!(Cli::try_parse_from(["superbash", "a", "b"]).is_err());
    }
}
