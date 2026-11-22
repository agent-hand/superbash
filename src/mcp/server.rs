//! MCP server（Streamable HTTP のサービスが包む処理本体）。道具は `superbash-command` 1 つ（設計 6 章）。

use std::fmt::Write as _;
use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use serde_json::{Map, Value, json};

use super::exec::{Outcome, Runner, resolve_timeout};
use crate::config::{Command, PositionalDef, Resolved, resolve, split_seq};

pub const TOOL_NAME: &str = "superbash-command";

#[derive(Clone)]
pub struct SuperbashServer {
    inner: Arc<Inner>,
}

struct Inner {
    /// 実行できる command（toolconfig.json の並び順）
    commands: Vec<Command>,
    runner: Runner,
}

impl SuperbashServer {
    pub fn new(commands: Vec<Command>, runner: Runner) -> Self {
        SuperbashServer {
            inner: Arc::new(Inner { commands, runner }),
        }
    }

    fn tool(&self) -> Tool {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "実行する command の名前。サブコマンドは空白で区切った並び（例: \"a b c\"）。呼べるのは葉だけ"
                },
                "args": {
                    "type": "object",
                    "description": "command への引数（JSON の object）。command ごとの in の定義に従う。省略時は {}",
                    "additionalProperties": true
                },
                "timeout": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "foreground の実行の時間の上限（ミリ秒）。省略時は 600000（10 分）。run_in_background の時は使わない（background に上限は無い）"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "true なら background で実行し、実行の id と出力 file の path を返す（出力はその file を読む）"
                }
            },
            "required": ["name"]
        });
        let Value::Object(schema) = schema else {
            unreachable!()
        };
        Tool::new(
            TOOL_NAME,
            describe_commands(&self.inner.commands),
            Arc::new(schema),
        )
    }

    async fn call(&self, args: Map<String, Value>) -> CallToolResult {
        let Some(name) = args.get("name").and_then(Value::as_str) else {
            return error_result("name（string）が必要");
        };
        let words = split_seq(name);
        let name = words.join(" ");
        let name = name.as_str();
        match resolve(&self.inner.commands, &words) {
            Resolved::Leaf { .. } => {}
            Resolved::Group(g) => {
                return error_result(&format!(
                    "{name} は子を持つ群で、呼べるのは葉だけ。子を指して: [{}]",
                    g.subcommands
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Resolved::NotFound => {
                let mut leaves = Vec::new();
                for c in &self.inner.commands {
                    leaf_sequences(c, "", &mut leaves);
                }
                return error_result(&format!(
                    "実行できない command: {}。実行できる command: [{}]",
                    args.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim(),
                    leaves.join(", ")
                ));
            }
        }
        let args_json = match args.get("args") {
            None | Some(Value::Null) => "{}".to_string(),
            Some(v @ Value::Object(_)) => v.to_string(),
            Some(_) => return error_result("args は JSON の object"),
        };
        let background = args
            .get("run_in_background")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let runner = &self.inner.runner;

        if background {
            return match runner.start_background(name, &args_json).await {
                Ok(bg) => {
                    let mut r = CallToolResult::success(vec![ContentBlock::text(format!(
                        "background で起動した。id: {}\n出力: {}\n状態: {}",
                        bg.id,
                        bg.output.display(),
                        bg.status.display()
                    ))]);
                    r.structured_content = Some(json!({
                        "id": bg.id,
                        "output_path": bg.output,
                        "status_path": bg.status,
                    }));
                    r
                }
                Err(e) => error_result(&format!("起動できない: {e:#}")),
            };
        }
        // timeout は foreground の時だけ見る（background の時は検証もしない）
        let timeout = match args.get("timeout") {
            None | Some(Value::Null) => resolve_timeout(None),
            Some(v) => match v.as_u64() {
                Some(ms) => resolve_timeout(Some(ms)),
                None => return error_result("timeout はミリ秒の整数"),
            },
        };
        match runner.run(name, &args_json, timeout).await {
            Ok(o) => outcome_result(o),
            Err(e) => error_result(&format!("実行できない: {e:#}")),
        }
    }
}

fn error_result(msg: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(msg)])
}

fn outcome_result(o: Outcome) -> CallToolResult {
    let mut text = String::new();
    if !o.stdout.is_empty() {
        let _ = write!(text, "stdout:\n{}", ensure_nl(&o.stdout));
    }
    if !o.stderr.is_empty() {
        let _ = write!(text, "stderr:\n{}", ensure_nl(&o.stderr));
    }
    if o.timed_out {
        text.push_str("時間切れ（時間の上限を超えたので止めた。出力はそれまで）\n");
    } else if let Some(code) = o.exit_code {
        let _ = writeln!(text, "終了 code: {code}");
    }
    let structured = json!({
        "stdout": o.stdout,
        "stderr": o.stderr,
        "exit_code": o.exit_code,
        "timed_out": o.timed_out,
    });
    let mut r = if o.timed_out {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    };
    r.structured_content = Some(structured);
    r
}

fn ensure_nl(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// 葉の並びの名前を集める。
fn leaf_sequences(c: &Command, prefix: &str, out: &mut Vec<String>) {
    let seq = if prefix.is_empty() {
        c.name.clone()
    } else {
        format!("{prefix} {}", c.name)
    };
    if c.is_group() {
        for s in &c.subcommands {
            leaf_sequences(s, &seq, out);
        }
    } else {
        out.push(seq);
    }
}

/// 道具の説明。実行できる command を木の形で並べる（設計 6 章）。
/// 葉は name・description・usage・in を全部、out は短く（stdout.format と終了 code の意味だけ）。
pub fn describe_commands(commands: &[Command]) -> String {
    let mut s = String::from(
        "toolset の command を 1 つ実行する。name に command の名前（サブコマンドは空白で区切った並び）、args に引数（JSON の object）を渡す。\n",
    );
    if commands.is_empty() {
        s.push_str("\n実行できる command は無い。\n");
        return s;
    }
    s.push_str("\n実行できる command（木の形。呼べるのは葉だけ）:\n");
    for c in commands {
        write_node(&mut s, c, "", 0);
    }
    s
}

fn write_node(s: &mut String, c: &Command, prefix: &str, depth: usize) {
    let seq = if prefix.is_empty() {
        c.name.clone()
    } else {
        format!("{prefix} {}", c.name)
    };
    let ind = " ".repeat(depth * 4);
    let _ = write!(s, "\n{ind}- name: {}", c.name);
    if depth > 0 && !c.is_group() {
        let _ = write!(s, "（呼ぶ name: \"{seq}\"）");
    }
    s.push('\n');
    if let Some(d) = &c.description {
        let _ = writeln!(s, "{ind}  description: {d}");
    }
    if c.is_group() {
        for sub in &c.subcommands {
            write_node(s, sub, &seq, depth + 1);
        }
        return;
    }
    let Some(spec) = &c.spec else { return };
    if let Some(u) = &spec.usage {
        let _ = writeln!(s, "{ind}  usage: {u}");
    }
    let input = &spec.input;
    if input.options.is_empty() && input.positionals.is_empty() {
        let _ = writeln!(s, "{ind}  in: 引数無し（args は {{}}）");
    } else {
        let _ = writeln!(s, "{ind}  in:");
        for (key, o) in input.options.iter() {
            let _ = writeln!(
                s,
                "{ind}    {key} ({:?} option {}){}",
                o.ty,
                o.flag,
                desc_suffix(o.description.as_deref())
            );
        }
        for p in &input.positionals {
            let _ = writeln!(s, "{ind}    {}", positional_line(p));
        }
    }
    if let Some(out) = short_out(spec.out.as_ref()) {
        let _ = writeln!(s, "{ind}  out: {out}");
    }
}

/// out の短い形（stdout.format と終了 code の意味だけ）。
fn short_out(out: Option<&Value>) -> Option<String> {
    let out = out?;
    let mut parts = Vec::new();
    if let Some(f) = out.pointer("/stdout/format").and_then(Value::as_str) {
        parts.push(format!("stdout={f}"));
    }
    if let Some(codes) = out.get("exitCodes").and_then(Value::as_object) {
        let list: Vec<String> = codes
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default()))
            .collect();
        if !list.is_empty() {
            parts.push(format!("exit {}", list.join(", ")));
        }
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn positional_line(p: &PositionalDef) -> String {
    format!(
        "{} ({:?} positional{}){}",
        p.name,
        p.ty,
        if p.optional { ", optional" } else { "" },
        desc_suffix(p.description.as_deref())
    )
}

fn desc_suffix(d: Option<&str>) -> String {
    d.map(|d| format!(": {d}")).unwrap_or_default()
}

impl ServerHandler for SuperbashServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("superbash", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(vec![self.tool()]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if request.name != TOOL_NAME {
            return Err(McpError::invalid_params(
                format!("道具が無い: {}", request.name),
                None,
            ));
        }
        Ok(self
            .call(request.arguments.unwrap_or_default())
            .await
            .into())
    }
}
