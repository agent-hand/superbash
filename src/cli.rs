//! CLI: `superbash [option...] <toolset>`（設計 2 章「起動と mode」）。

use clap::Parser;

use crate::config::Command;

#[derive(Parser, Debug)]
#[command(name = "superbash", version, about)]
pub struct Cli {
    /// 生成した package を置き場に置き、npm link で入れる（MCP は起動しない）
    #[arg(long)]
    pub only_install: bool,

    /// 実行できる command（指定無し=全部、'a,b'=その command だけ、''=無し）
    #[arg(long, value_name = "CMDS")]
    pub allow_cmd: Option<String>,

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
    pub fn parse(arg: Option<&str>) -> Self {
        match arg {
            None => AllowCmd::All,
            Some(s) => AllowCmd::Only(
                s.split(',')
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(String::from)
                    .collect(),
            ),
        }
    }

    /// toolconfig.json の並び順のまま、実行できる command を返す。
    pub fn select<'a>(&self, commands: &'a [Command]) -> Vec<&'a Command> {
        match self {
            AllowCmd::All => commands.iter().collect(),
            AllowCmd::Only(names) => commands
                .iter()
                .filter(|c| names.iter().any(|n| n == &c.name))
                .collect(),
        }
    }

    /// toolset に無い名前（typo の気付き用。拒絶はしない）。
    pub fn unknown_names<'a>(&'a self, commands: &[Command]) -> Vec<&'a str> {
        match self {
            AllowCmd::All => Vec::new(),
            AllowCmd::Only(names) => names
                .iter()
                .filter(|n| !commands.iter().any(|c| &c.name == *n))
                .map(String::as_str)
                .collect(),
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

    #[test]
    fn toolset_is_required_and_single() {
        assert!(Cli::try_parse_from(["superbash"]).is_err());
        assert!(Cli::try_parse_from(["superbash", "a", "b"]).is_err());
    }
}
