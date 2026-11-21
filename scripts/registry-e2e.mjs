// GitHub Actions の runner の中だけで行う、使い捨ての registry（verdaccio）を使った試験。
// 使い方: node scripts/registry-e2e.mjs <build した superbash の path> <作業 dir（無ければ作る）> <verdaccio の bin>
//
// - verdaccio は 127.0.0.1 だけで listen し、uplink（公開 npm への中継）を持たない。storage は作業 dir の中。
// - publish は必ず --registry http://127.0.0.1:<port> を明示する。公開 npm には出さない。
// - npm の cache・prefix・userconfig・HOME は作業 dir の中。userconfig の token は固定の無意味な文字列（secret ではない）。
// - 待ちは registry の起動確認の `npm ping`（npm の fetch-retries の上限つき・1 回の command。loop は書かない）だけ。
//   終わりに verdaccio を止める。
//
// 試験 1: @scope 付きと無しの toolset を publish し、superbash に名前で渡して取得・list・call が通る。
// 試験 2: 本体 package と、この OS・CPU の platform package を publish し、`npx -y superbash@<版> <toolset>` で
//         MCP（Streamable HTTP）の server が起動し initialize・tools/list・tools/call が通る。
import { spawn, spawnSync } from "node:child_process";
import { chmodSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { mcpHttpSmoke } from "./mcp-http-client.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [binArg, workArg, vdArg] = process.argv.slice(2);
if (!binArg || !workArg || !vdArg) {
  console.error("usage: registry-e2e.mjs <superbash binary> <work dir> <verdaccio bin>");
  process.exit(2);
}
const binary = resolve(binArg);
const work = resolve(workArg);
const verdaccio = resolve(vdArg);
const key = `${process.platform}-${process.arch}`;
const PORT = 4873;
const REGISTRY = `http://127.0.0.1:${PORT}`;
const SCOPE_NAME = "@sbtest/demo-tools";

const fail = (m) => {
  console.error(`FAIL: ${m}`);
  process.exitCode = 1;
  throw new Error(m);
};
const readJson = (p) => JSON.parse(readFileSync(p, "utf8"));

// 作業 dir と npm の環境（全部作業 dir の中）
rmSync(work, { recursive: true, force: true });
for (const d of ["home", "storage", "pkgs"]) mkdirSync(join(work, d), { recursive: true });
const rc = join(work, "npmrc");
const grc = join(work, "npmrc-global");
writeFileSync(grc, "");
writeFileSync(
  rc,
  `registry=${REGISTRY}\n//127.0.0.1:${PORT}/:_authToken=disposable-not-a-secret\n` +
    "update-notifier=false\nfund=false\naudit=false\n",
);
const env = {
  ...process.env,
  HOME: join(work, "home"),
  npm_config_registry: REGISTRY,
  npm_config_cache: join(work, "cache"),
  npm_config_prefix: join(work, "prefix"),
  npm_config_userconfig: rc,
  npm_config_globalconfig: grc,
};
const sh = (cmd, args, cwd = work) => {
  const r = spawnSync(cmd, args, { cwd, env, encoding: "utf8" });
  if (r.status !== 0) fail(`${cmd} ${args.join(" ")}\n${r.stdout}\n${r.stderr}`);
  return r.stdout;
};

// verdaccio（127.0.0.1 だけ・認証無しで publish できる・uplink 無し）
const cfg = join(work, "verdaccio.yaml");
writeFileSync(
  cfg,
  [
    `storage: ${join(work, "storage")}`,
    "auth:",
    "  htpasswd:",
    `    file: ${join(work, "htpasswd")}`,
    "    max_users: -1",
    "packages:",
    "  '**':",
    "    access: $all",
    "    publish: $all",
    "    unpublish: $all",
    "log: { type: stdout, format: pretty, level: warn }",
    "",
  ].join("\n"),
);
const vd = spawn(process.execPath, [verdaccio, "--config", cfg, "--listen", `127.0.0.1:${PORT}`], {
  cwd: work,
  env,
  stdio: ["ignore", "inherit", "inherit"],
});
writeFileSync(join(work, "verdaccio.pid"), String(vd.pid));

// registry は常にこの 127.0.0.1 の 1 つを明示する（公開 npm には出さない）
const publish = (dir) => {
  sh("npm", ["publish", dir, "--registry", REGISTRY, "--access", "public"]);
};

// MCP（Streamable HTTP）の client。superbash を空き port で起動し、address は起動の 1 行から読む
const mcp = (cmd, args, label) =>
  mcpHttpSmoke({ cmd, args, env, cwd: work, label, clientName: "registry-e2e" }).catch((e) => fail(String(e)));

try {
  if (!["darwin-arm64", "darwin-x64", "linux-x64", "linux-arm64"].includes(key)) fail(`対象外: ${key}`);

  // registry の起動確認: npm ping 1 回。接続できない間は npm が fetch-retries の上限まで再試行する
  sh("npm", [
    "ping",
    "--registry", REGISTRY,
    "--fetch-retries", "30",
    "--fetch-retry-mintimeout", "1000",
    "--fetch-retry-maxtimeout", "1000",
  ]);
  console.log("verdaccio: up");

  // 試験 1: toolset（scope 無しと有り）を publish して、名前で渡す
  const fixture = join(root, "tests/fixtures/demo-tools");
  const plain = join(work, "pkgs/demo-tools");
  cpSync(fixture, plain, { recursive: true });
  publish(plain);
  const scoped = join(work, "pkgs/scoped-demo-tools");
  cpSync(fixture, scoped, { recursive: true });
  const sp = readJson(join(scoped, "package.json"));
  sp.name = SCOPE_NAME;
  writeFileSync(join(scoped, "package.json"), JSON.stringify(sp, null, 2));
  publish(scoped);
  await mcp(binary, ["demo-tools"], "toolset-plain");
  await mcp(binary, [`${SCOPE_NAME}@0.1.0`], "toolset-scoped");

  // 試験 2: 本体 package と、この platform の package を publish して npx で起動する
  const main = readJson(join(root, "npm/superbash/package.json"));
  const plat = join(work, "pkgs/plat");
  cpSync(join(root, `npm/platforms/${key}`), plat, { recursive: true });
  mkdirSync(join(plat, "bin"), { recursive: true });
  cpSync(binary, join(plat, "bin/superbash"));
  chmodSync(join(plat, "bin/superbash"), 0o755);
  publish(plat);
  const mainDir = join(work, "pkgs/main");
  cpSync(join(root, "npm/superbash"), mainDir, { recursive: true });
  publish(mainDir);
  await mcp("npx", ["-y", `superbash@${main.version}`, "demo-tools"], "npx-superbash");
  console.log("OK: registry-e2e");
} finally {
  vd.kill();
}
