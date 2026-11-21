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

#[derive(Debug, Clone, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    /// `tools` の key
    pub tool: String,
    pub version: Option<String>,
    pub spec: Spec,
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
    pub options: BTreeMap<String, OptionDef>,
    #[serde(default)]
    pub positionals: Vec<PositionalDef>,
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

    /// 参照の整合（command の名前の重複、tool の参照先）だけを見る。
    fn validate(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for c in &self.commands {
            if !seen.insert(c.name.as_str()) {
                bail!("command の名前が重複: {}", c.name);
            }
            if !self.tools.contains_key(&c.tool) {
                bail!("command {} の tool {} が tools に無い", c.name, c.tool);
            }
        }
        Ok(())
    }
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
        assert!(c.commands[0].spec.input.positionals.is_empty());
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
}
