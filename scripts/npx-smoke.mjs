// npm pack した tarball から npx で superbash を起動し、MCP（Streamable HTTP）の initialize・tools/list・tools/call を通す。
// 使い方: node scripts/npx-smoke.mjs <build した superbash の path> <作業 dir（無ければ作る）>
// npm の cache・prefix・userconfig は作業 dir の中。registry には繋がない（本体の optionalDependencies は
// この platform の tarball の file: に書き換えた写しで確かめる）。
import { spawnSync } from "node:child_process";
import { chmodSync, cpSync, mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { mcpHttpSmoke } from "./mcp-http-client.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [binArg, workArg] = process.argv.slice(2);
if (!binArg || !workArg) {
  console.error("usage: npx-smoke.mjs <superbash binary> <work dir>");
  process.exit(2);
}
const binary = resolve(binArg);
const work = resolve(workArg);
const key = `${process.platform}-${process.arch}`;
const ALL = ["darwin-arm64", "darwin-x64", "linux-x64", "linux-arm64"];

const readJson = (p) => JSON.parse(readFileSync(p, "utf8"));
const fail = (m) => {
  console.error(`FAIL: ${m}`);
  process.exit(1);
};

// 1. package の形
const main = readJson(join(root, "npm/superbash/package.json"));
for (const k of ALL) {
  const p = readJson(join(root, `npm/platforms/${k}/package.json`));
  const [os, cpu] = k.split("-");
  if (p.name !== `superbash-${k}`) fail(`name: ${k}`);
  if (p.os?.[0] !== os || p.cpu?.[0] !== cpu) fail(`os/cpu: ${k}`);
  if (main.optionalDependencies?.[p.name] !== p.version) fail(`optionalDependencies: ${p.name}`);
}
if (!ALL.includes(key)) fail(`この環境（${key}）は対象外`);

// 2. npm の環境（作業 dir の中だけ）
mkdirSync(work, { recursive: true });
const rc = join(work, "npmrc");
const grc = join(work, "npmrc-global");
writeFileSync(grc, "");
writeFileSync(rc, "update-notifier=false\nfund=false\naudit=false\n");
const env = {
  ...process.env,
  npm_config_cache: join(work, "cache"),
  npm_config_prefix: join(work, "prefix"),
  npm_config_userconfig: rc,
  npm_config_globalconfig: grc,
};
const run = (cmd, args, cwd) => {
  const r = spawnSync(cmd, args, { cwd, env, encoding: "utf8" });
  if (r.status !== 0) fail(`${cmd} ${args.join(" ")}\n${r.stdout}\n${r.stderr}`);
  return r.stdout;
};
const pack = (dir) => {
  const out = JSON.parse(run("npm", ["pack", "--json", "--pack-destination", work], dir));
  return join(work, out[0].filename);
};

// 3. platform の package（実行 file を置いて pack）と本体の package（file: に書き換えた写しを pack）
const platDir = join(work, "plat");
cpSync(join(root, `npm/platforms/${key}`), platDir, { recursive: true });
mkdirSync(join(platDir, "bin"), { recursive: true });
cpSync(binary, join(platDir, "bin/superbash"));
chmodSync(join(platDir, "bin/superbash"), 0o755);
const platTgz = pack(platDir);

const mainDir = join(work, "main");
cpSync(join(root, "npm/superbash"), mainDir, { recursive: true });
const mp = readJson(join(mainDir, "package.json"));
mp.optionalDependencies = { [`superbash-${key}`]: `file:${platTgz}` };
writeFileSync(join(mainDir, "package.json"), JSON.stringify(mp, null, 2));
const mainTgz = pack(mainDir);
console.log(`packed: ${mainTgz}\npacked: ${platTgz}`);

// 4. npx で起動して MCP（Streamable HTTP）を通す
const toolset = join(root, "tests/fixtures/demo-tools");
try {
  await mcpHttpSmoke({
    cmd: "npx",
    args: ["--yes", `--package=${mainTgz}`, "--", "superbash", toolset],
    env,
    cwd: work,
    label: "npx-smoke",
    clientName: "npx-smoke",
  });
} catch (e) {
  fail(String(e));
}
