// stage-npm.mjs で組み立てた 5 つの package を `npm pack --dry-run --json` で確かめる（token 不要・registry に繋がない）。
// 使い方: node scripts/pack-check.mjs <stage した dir>
// 各 package に package.json・LICENSE と実行 file（本体は bin/superbash.js、OS・CPU ごとは bin/superbash）が入り、
// license が MIT で LICENSE の本文も MIT License で、version が揃っている事を見る。足りなければ fail。
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

if (!process.argv[2]) {
  console.error("usage: pack-check.mjs <staged dir>");
  process.exit(2);
}
const stage = resolve(process.argv[2]);
const ALL = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"];
let bad = 0;
const fail = (m) => {
  console.error(`FAIL: ${m}`);
  bad++;
};

const check = (dir, name, expectBin) => {
  const pj = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  if (pj.name !== name) fail(`${name}: package.json の name が ${pj.name}`);
  if (pj.license !== "MIT") fail(`${name}: license が ${pj.license}`);
  const r = spawnSync("npm", ["pack", "--dry-run", "--json"], { cwd: dir, encoding: "utf8" });
  if (r.status !== 0) {
    fail(`${name}: npm pack --dry-run\n${r.stdout}\n${r.stderr}`);
    return pj;
  }
  const files = new Map(JSON.parse(r.stdout)[0].files.map((f) => [f.path, f]));
  for (const need of ["package.json", "LICENSE", expectBin]) {
    if (!files.has(need)) fail(`${name}: tarball に ${need} が無い（${[...files.keys()].join(", ")}）`);
  }
  // LICENSE の本文が MIT License の本文である事（先頭の行と許諾の文の要点）。
  const lic = readFileSync(join(dir, "LICENSE"), "utf8");
  if (lic.split("\n")[0].trim() !== "MIT License") fail(`${name}: LICENSE の先頭の行が "MIT License" でない`);
  for (const phrase of [
    "Permission is hereby granted, free of charge, to any person obtaining a copy",
    "to deal in the Software without restriction",
    "The above copyright notice and this permission notice shall be included in all",
    'THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND',
  ]) {
    if (!lic.replace(/\s+/g, " ").includes(phrase)) fail(`${name}: LICENSE に MIT License の本文（${phrase}）が無い`);
  }
  const b = files.get(expectBin);
  if (b && (b.mode & 0o111) === 0) fail(`${name}: ${expectBin} に実行 bit が無い（mode=${b.mode.toString(8)}）`);
  if (b && b.size === 0) fail(`${name}: ${expectBin} が空`);
  console.log(`ok ${name}@${pj.version}: ${[...files.keys()].join(", ")}`);
  return pj;
};

const main = check(join(stage, "superbash"), "superbash", "bin/superbash.js");
for (const k of ALL) {
  const p = check(join(stage, `superbash-${k}`), `superbash-${k}`, "bin/superbash");
  if (p.version !== main.version) fail(`superbash-${k}: version ${p.version} が本体 ${main.version} と違う`);
  if (main.optionalDependencies?.[p.name] !== p.version) fail(`本体の optionalDependencies: ${p.name}`);
}
process.exit(bad ? 1 : 0);
