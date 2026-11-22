//! `.manifest/toolconfig.json` の読み込み（設計 3 章）。形は toolconfig.schema.json と対応する。

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const TOOLCONFIG_PATH: &str = ".manifest/toolconfig.json";

#[derive(Debug, Clone, Deserialize)]
pub struct ToolConfig {
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub owner: Option<serde_json::Value>,
    pub tools: BTreeMap<String, ToolDef>,
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolDef {
    /// `tools/<name>/` からの相対 path の入口
    pub entry: String,
}

/// 要素（設計 3.2）。`commands[]` の 1 つ（上の要素）か、その下の `subcommands[]` の 1 つ。
/// 同じ形で、`tool`・`category`・`version`・`spec.subcommand` は上の要素だけが持つ。
#[derive(Debug, Clone, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    /// `tools` の key（上の要素は必須。検査は validate）
    pub tool: Option<String>,
    pub version: Option<String>,
    /// 葉は必須、群（子を持つ要素）は省ける（検査は validate）
    pub spec: Option<Spec>,
    #[serde(default)]
    pub subcommands: Vec<Command>,
}

impl Command {
    /// 群（子を持つ要素）か。
    pub fn is_group(&self) -> bool {
        !self.subcommands.is_empty()
    }

    /// 上の要素の `tool`（検査済みの config では必ずある）。
    pub fn tool_name(&self) -> &str {
        self.tool.as_deref().unwrap_or_default()
    }
}

/// 並びの名前（空白で区切った並び）を語に分ける。前後の空白は捨て、続いた空白は 1 つの区切り。
pub fn split_seq(name: &str) -> Vec<&str> {
    name.split_whitespace().collect()
}

/// 並びの名前を引いた結果。
#[derive(Debug)]
pub enum Resolved<'a> {
    /// 葉。`head` は argv の頭（上の要素の `spec.subcommand` と、上の要素より下の名前）
    Leaf {
        node: &'a Command,
        head: Vec<&'a str>,
    },
    /// 群（呼べない）
    Group(&'a Command),
    NotFound,
}

/// 上の要素の並び（`commands`）から、並びの名前の語で要素を引く。
pub fn resolve<'a>(commands: &'a [Command], words: &[&str]) -> Resolved<'a> {
    let Some((first, rest)) = words.split_first() else {
        return Resolved::NotFound;
    };
    let Some(top) = commands.iter().find(|c| c.name == *first) else {
        return Resolved::NotFound;
    };
    let mut node = top;
    let mut head: Vec<&str> = top
        .spec
        .as_ref()
        .and_then(|s| s.subcommand.as_deref())
        .into_iter()
        .collect();
    for w in rest {
        let Some(child) = node.subcommands.iter().find(|c| c.name == *w) else {
            return Resolved::NotFound;
        };
        head.push(child.name.as_str());
        node = child;
    }
    if node.is_group() {
        Resolved::Group(node)
    } else {
        Resolved::Leaf { node, head }
    }
}

/// 群と葉の全部の並びの名前（`--allow-cmd` の名前の照合用）。
pub fn all_sequences(commands: &[Command]) -> Vec<String> {
    fn walk(c: &Command, prefix: &str, out: &mut Vec<String>) {
        let name = if prefix.is_empty() {
            c.name.clone()
        } else {
            format!("{prefix} {}", c.name)
        };
        out.push(name.clone());
        for s in &c.subcommands {
            walk(s, &name, out);
        }
    }
    let mut out = Vec::new();
    for c in commands {
        walk(c, "", &mut out);
    }
    out
}

#[derive(Debug, Clone, Deserialize)]
pub struct Spec {
    pub subcommand: Option<String>,
    pub usage: Option<String>,
    #[serde(rename = "in", default)]
    pub input: SpecIn,
    pub out: Option<serde_json::Value>,
}

/// spec.in の形（設計 3.3 の例を基に確定）。
/// wrapper は、subcommand、options（定義の順）、positionals（定義の順）の順に argv を組む。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SpecIn {
    #[serde(default)]
    pub options: OrderedMap<OptionDef>,
    #[serde(default)]
    pub positionals: Vec<PositionalDef>,
}

/// JSON の object を、書いた順のまま持つ map（BTreeMap は辞書順になる）。
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedMap<T>(pub Vec<(String, T)>);

impl<T> Default for OrderedMap<T> {
    fn default() -> Self {
        OrderedMap(Vec::new())
    }
}

impl<T> OrderedMap<T> {
    /// 定義の順に (key, 値) を返す。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OrderedMap<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for V<T> {
            type Value = OrderedMap<T>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut m: A,
            ) -> Result<Self::Value, A::Error> {
                let mut v = Vec::new();
                while let Some(kv) = m.next_entry::<String, T>()? {
                    v.push(kv);
                }
                Ok(OrderedMap(v))
            }
        }
        d.deserialize_map(V(std::marker::PhantomData))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OptionDef {
    pub flag: String,
    #[serde(rename = "type")]
    pub ty: ArgType,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PositionalDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ArgType,
    pub description: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgType {
    Boolean,
    String,
    Number,
}

impl ToolConfig {
    pub fn load(toolset_dir: &Path) -> Result<Self> {
        let path = toolset_dir.join(TOOLCONFIG_PATH);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("{} を読めない", path.display()))?;
        let cfg: ToolConfig = serde_json::from_str(&text)
            .with_context(|| format!("{} の形が不正", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// 参照の整合と、要素の形の検査（設計 3.2）。
    /// 兄弟の名前の重複、`name` の空白、`spec.subcommand` は上の要素だけ、葉は `spec` が必須、
    /// `help`（bin の直後の予約の語）、tool の参照先。
    fn validate(&self) -> Result<()> {
        check_siblings(&self.commands, "")?;
        for c in &self.commands {
            let Some(tool) = &c.tool else {
                bail!("command {} に tool が無い", c.name);
            };
            if !self.tools.contains_key(tool) {
                bail!("command {} の tool {} が tools に無い", c.name, tool);
            }
            // bin の直後の語。spec.subcommand があればそれ、無ければ 1 段目の子の名前。
            match c.spec.as_ref().and_then(|s| s.subcommand.as_deref()) {
                Some(HELP) => bail!(
                    "command {} の spec.subcommand に help は使えない（予約）",
                    c.name
                ),
                Some(_) => {}
                None => {
                    if c.subcommands.iter().any(|s| s.name == HELP) {
                        bail!(
                            "command {} の子の名前に help は使えない（bin の直後の予約）",
                            c.name
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

/// bin の直後の予約の語（設計 3.4・6.9）。
const HELP: &str = "help";

fn check_siblings(siblings: &[Command], path: &str) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for c in siblings {
        let full = if path.is_empty() {
            c.name.clone()
        } else {
            format!("{path} {}", c.name)
        };
        if c.name.is_empty() || c.name.chars().any(char::is_whitespace) {
            bail!("name に空白を含められない（空も不可）: {full:?}");
        }
        if !seen.insert(c.name.as_str()) {
            bail!("command の名前が重複: {full}");
        }
        if !path.is_empty() && c.spec.as_ref().is_some_and(|s| s.subcommand.is_some()) {
            bail!("spec.subcommand は上の要素だけ: {full}");
        }
        if !c.is_group() && c.spec.is_none() {
            bail!("葉の要素に spec が必要: {full}");
        }
        check_siblings(&c.subcommands, &full)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<ToolConfig> {
        let cfg: ToolConfig = serde_json::from_str(json)?;
        cfg.validate()?;
        Ok(cfg)
    }

    const OK: &str = r#"{"name":"t","tools":{"x":{"entry":"main.js"}},
      "commands":[{"name":"c","tool":"x","spec":{"subcommand":"s"}}]}"#;

    #[test]
    fn minimal_config_loads() {
        let c = parse(OK).unwrap();
        assert!(
            c.commands[0]
                .spec
                .as_ref()
                .unwrap()
                .input
                .positionals
                .is_empty()
        );
    }

    #[test]
    fn options_keep_definition_order() {
        let json = OK.replace(
            r#""spec":{"subcommand":"s"}"#,
            r#""spec":{"in":{"options":{
              "zeta":{"flag":"--zeta","type":"boolean"},
              "alpha":{"flag":"--alpha","type":"string"},
              "mid":{"flag":"--mid","type":"number"}}}}"#,
        );
        let c = parse(&json).unwrap();
        let keys: Vec<&str> = c.commands[0]
            .spec
            .as_ref()
            .unwrap()
            .input
            .options
            .iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["zeta", "alpha", "mid"]);
    }

    #[test]
    fn unknown_tool_is_error() {
        assert!(parse(&OK.replace(r#""tool":"x""#, r#""tool":"y""#)).is_err());
    }

    #[test]
    fn duplicate_command_name_is_error() {
        let dup = OK.replace(r#"]}"#, r#",{"name":"c","tool":"x","spec":{}}]}"#);
        assert!(parse(&dup).is_err());
    }

    const NESTED: &str = r#"{"name":"t","tools":{"x":{"entry":"main.js"}},
      "commands":[{"name":"g","tool":"x","spec":{"subcommand":"g"},"subcommands":[
        {"name":"r","subcommands":[{"name":"c","spec":{}}]}]}]}"#;

    #[test]
    fn nested_group_without_spec_loads_and_resolves() {
        let c = parse(NESTED).unwrap();
        match resolve(&c.commands, &["g", "r", "c"]) {
            Resolved::Leaf { head, .. } => assert_eq!(head, ["g", "r", "c"]),
            r => panic!("{r:?}"),
        }
        assert!(matches!(
            resolve(&c.commands, &["g", "r"]),
            Resolved::Group(_)
        ));
        assert!(matches!(
            resolve(&c.commands, &["g", "x"]),
            Resolved::NotFound
        ));
        assert_eq!(all_sequences(&c.commands), ["g", "g r", "g r c"]);
    }

    #[test]
    fn invalid_elements_are_errors() {
        // 兄弟の重複（子の段）
        let dup = NESTED.replace(
            r#"{"name":"c","spec":{}}"#,
            r#"{"name":"c","spec":{}},{"name":"c","spec":{}}"#,
        );
        assert!(parse(&dup).is_err());
        // name に空白
        assert!(parse(&NESTED.replace(r#""name":"r""#, r#""name":"r s""#)).is_err());
        // spec.subcommand は上の要素だけ
        let sub = NESTED.replace(
            r#""name":"c","spec":{}"#,
            r#""name":"c","spec":{"subcommand":"q"}"#,
        );
        assert!(parse(&sub).is_err());
        // 葉に spec が無い
        assert!(parse(&NESTED.replace(r#","spec":{}}]}]}]}"#, r#"}]}]}]}"#)).is_err());
        // help の予約
        assert!(parse(&OK.replace(r#""subcommand":"s""#, r#""subcommand":"help""#)).is_err());
        let help_child = r#"{"name":"t","tools":{"x":{"entry":"main.js"}},
          "commands":[{"name":"g","tool":"x","subcommands":[{"name":"help","spec":{}}]}]}"#;
        assert!(parse(help_child).is_err());
    }
}
