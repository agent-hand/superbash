// npm に出す 5 つの package（本体 + OS・CPU ごと 4 つ）を、作業 dir の中に組み立てる。
// 使い方: node scripts/stage-npm.mjs <出力 dir> <key=実行 file の path>...
//   key は darwin-arm64 / darwin-x64 / linux-arm64 / linux-x64。
// 出力: <出力 dir>/superbash と <出力 dir>/superbash-<key>。各 package に LICENSE と実行 file を入れる。
// repo の中は変えない（組み立ては写しの上でだけ）。registry には繋がない。
import { chmodSync, cpSync, copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ALL = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"];
const [outArg, ...pairs] = process.argv.slice(2);
if (!outArg || pairs.length === 0) {
  console.error("usage: stage-npm.mjs <out dir> <key=binary>...");
  process.exit(2);
}
const out = resolve(outArg);
const bins = new Map(pairs.map((p) => p.split(/=(.*)/s).slice(0, 2)));
const fail = (m) => {
  console.error(`FAIL: ${m}`);
  process.exit(1);
};
for (const k of ALL) {
  const b = bins.get(k);
  if (!b) fail(`実行 file の指定が無い: ${k}`);
  if (!existsSync(b)) fail(`実行 file が無い: ${k}=${b}`);
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const main = join(out, "superbash");
cpSync(join(root, "npm/superbash"), main, { recursive: true });
copyFileSync(join(root, "LICENSE"), join(main, "LICENSE"));

for (const k of ALL) {
  const dir = join(out, `superbash-${k}`);
  cpSync(join(root, `npm/platforms/${k}`), dir, { recursive: true });
  mkdirSync(join(dir, "bin"), { recursive: true });
  const dst = join(dir, "bin/superbash");
  copyFileSync(resolve(bins.get(k)), dst);
  chmodSync(dst, 0o755);
  copyFileSync(join(root, "LICENSE"), join(dir, "LICENSE"));
}
console.log(`staged: ${out}`);
