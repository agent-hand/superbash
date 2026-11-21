// superbash（Streamable HTTP の MCP server）を起動し、MCP の initialize・tools/list・tools/call を通す共通の部品。
// - 空き port（--port 0）で起動し、起動の 1 行（stdout）から address を読む。既定 port には頼らない。
// - 待ちは、起動の 1 行を読む event と、HTTP の応答だけ。loop は書かない。
// - 終わりに、起動した process の group を止める（npx 経由でも残さない）。
import { spawn } from "node:child_process";

const LISTENING = /superbash: listening on (http:\/\/\S+\/mcp)/;

export async function mcpHttpSmoke({ cmd, args, env, cwd, label, clientName, tool }) {
  const child = spawn(cmd, [...args, "--host", "127.0.0.1", "--port", "0"], {
    cwd,
    env,
    detached: true,
    stdio: ["ignore", "pipe", "inherit"],
  });
  const stop = () => {
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {}
  };
  try {
    const url = await new Promise((res, rej) => {
      const t = setTimeout(() => rej(new Error(`${label}: 起動の 1 行が出ない`)), 240_000);
      let buf = "";
      child.stdout.on("data", (d) => {
        buf += d;
        const m = LISTENING.exec(buf);
        if (m) {
          clearTimeout(t);
          res(m[1]);
        }
      });
      child.on("exit", (code) => {
        clearTimeout(t);
        rej(new Error(`${label}: 起動の 1 行が出ずに終了（code ${code}）`));
      });
    });
    if (!url.startsWith("http://127.0.0.1:")) throw new Error(`${label}: 127.0.0.1 以外: ${url}`);

    let session;
    let nextId = 1;
    const post = async (body) => {
      const headers = {
        "content-type": "application/json",
        accept: "application/json, text/event-stream",
      };
      if (session) headers["mcp-session-id"] = session;
      const r = await fetch(url, {
        method: "POST",
        headers,
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(240_000),
      });
      session = r.headers.get("mcp-session-id") ?? session;
      return { status: r.status, type: r.headers.get("content-type") ?? "", text: await r.text() };
    };
    const request = async (method, params) => {
      const id = nextId++;
      const r = await post({ jsonrpc: "2.0", id, method, params });
      if (r.status !== 200) throw new Error(`${label}: ${method}: HTTP ${r.status}: ${r.text}`);
      const payloads = r.type.includes("text/event-stream")
        ? r.text.split("\n").filter((l) => l.startsWith("data:")).map((l) => l.slice(5).trim()).filter(Boolean)
        : [r.text];
      for (const p of payloads) {
        const msg = JSON.parse(p);
        if (msg.id === id) return msg;
      }
      throw new Error(`${label}: ${method}: 応答が無い: ${r.text}`);
    };

    const init = await request("initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: clientName, version: "0" },
    });
    if (init.result?.serverInfo?.name !== "superbash") throw new Error(`${label}: initialize: ${JSON.stringify(init)}`);
    const n = await post({ jsonrpc: "2.0", method: "notifications/initialized" });
    if (n.status !== 202) throw new Error(`${label}: initialized: HTTP ${n.status}`);

    const list = await request("tools/list", {});
    const tools = list.result?.tools ?? [];
    if (tools.length !== 1 || tools[0].name !== "superbash-command") throw new Error(`${label}: tools/list: ${JSON.stringify(list)}`);
    if (!String(tools[0].description).includes("name: hello")) throw new Error(`${label}: command hello が説明に無い`);

    const who = `world-${label.replace(/[^a-z0-9]/gi, "")}`;
    const call = await request("tools/call", {
      name: "superbash-command",
      arguments: { name: "hello", args: { who } },
    });
    const text = (call.result?.content ?? []).map((c) => c.text ?? "").join("\n");
    if (call.result?.isError || !text.includes(`hello ${who}`)) throw new Error(`${label}: tools/call: ${JSON.stringify(call)}`);
    console.log(`OK: ${label}: initialize / tools/list / tools/call (${url})`);
  } finally {
    stop();
  }
}
