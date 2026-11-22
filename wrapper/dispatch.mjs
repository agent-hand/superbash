#!/usr/bin/env node
// superbash の振り分けの file（busybox 化。docs/toolset-convention.md 5.4）。
// module のある tool（toolconfig の tools.<名前>.module）を、次のどちらかで選んで走らせる。
//   1. 呼ばれた link の名前（only-install。module のある全 tool の bin がこの file を向く）
//   2. 第 1 引数（MCP の mode の wrapper が tool の名前を最初に渡す）
// module の `run(rawArgs)` が返す数を終了 code にする。citty は持たない（module 側が持つ）。

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// import.meta.url は symlink を解決した実体の path なので、link 経由で呼ばれても ROOT は変わらない。
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const config = JSON.parse(fs.readFileSync(path.join(ROOT, '.manifest', 'toolconfig.json'), 'utf8'));
const modules = Object.entries(config.tools || {})
  .filter(([, t]) => typeof t.module === 'string' && t.module !== '')
  .map(([name, t]) => [name, t.module]);
const table = new Map(modules);

function fail(msg, code) {
  process.stderr.write(`superbash-dispatch: ${msg}\n`);
  process.exit(code);
}

// argv[1] は link の path のまま（実体に解決されない）。
const linkName = path.basename(process.argv[1] || '');
let tool;
let args;
if (table.has(linkName)) {
  tool = linkName;
  args = process.argv.slice(2);
} else {
  tool = process.argv[2];
  args = process.argv.slice(3);
  if (!tool || !table.has(tool)) {
    fail(`tool を選べない: ${tool ?? '(無し)'}。選べる物: [${[...table.keys()].join(', ')}]`, 2);
  }
}

let code;
try {
  const file = path.join(ROOT, 'tools', tool, table.get(tool));
  const mod = await import(pathToFileURL(file).href);
  if (typeof mod.run !== 'function') fail(`${tool} の module が run を export していない`, 1);
  code = await mod.run(args);
} catch (e) {
  fail(`${tool}: ${e && e.stack ? e.stack : e}`, 1);
}
// process.exit は stdout の書き出しを切る事があるので、終了 code を置いて自然に終わらせる。
process.exitCode = Number.isInteger(code) && code >= 0 && code <= 255 ? code : 1;
