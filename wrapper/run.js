#!/usr/bin/env node
'use strict';
// superbash の wrapper（MCP の mode 用）。
// 使い方: superbash-run <command 名（サブコマンドは空白で区切った並び）> [<args の JSON>]
// toolconfig.json の spec.in の規則で argv の配列を組み、command の bin を shell を通さず起動する。

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');

class UsageError extends Error {}

// 並びの名前（空白で区切った並び）で葉を引く。前後の空白は捨て、続いた空白は 1 つの区切り。
// 戻り値は { top, node, head }。head は argv の頭（上の要素の spec.subcommand と、上の要素より下の名前）。
// 群は呼べない（子の名前の一覧を誤りで返す）。
function resolveLeaf(commands, name) {
  const words = String(name).split(/\s+/).filter((w) => w !== '');
  const top = words.length > 0 ? commands.find((c) => c.name === words[0]) : undefined;
  if (!top) throw new UsageError(`command が無い: ${name}`);
  let node = top;
  const head = top.spec && top.spec.subcommand ? [top.spec.subcommand] : [];
  for (const w of words.slice(1)) {
    const child = (node.subcommands || []).find((c) => c.name === w);
    if (!child) throw new UsageError(`command が無い: ${words.join(' ')}`);
    head.push(child.name);
    node = child;
  }
  const subs = node.subcommands || [];
  if (subs.length > 0) {
    throw new UsageError(
      `${words.join(' ')} は子を持つ群で、呼べるのは葉だけ。子を指して: [${subs.map((c) => c.name).join(', ')}]`,
    );
  }
  return { top, node, head };
}

// argv = head（spec.subcommand と、上の要素より下の名前）
//        + options（定義の順。boolean は true の時だけ flag、string/number は flag と値の 2 引数）
//        + [`--`（positional を渡す時）] + positionals（定義の順。省略できるのは optional のものだけ）
// args は呼ぶ葉の spec.in だけで読む。
function buildArgv(command, args, head = []) {
  const spec = command.spec || {};
  const input = spec.in || {};
  const options = input.options || {};
  const positionals = input.positionals || [];

  if (args === null || typeof args !== 'object' || Array.isArray(args)) {
    throw new UsageError('args は JSON の object でなければならない');
  }
  const known = new Set([...Object.keys(options), ...positionals.map((p) => p.name)]);
  for (const key of Object.keys(args)) {
    if (!known.has(key)) throw new UsageError(`未定義の引数: ${key}`);
  }

  const argv = [...head];

  for (const [name, def] of Object.entries(options)) {
    const v = args[name];
    if (v === undefined || v === null) continue;
    if (def.type === 'boolean') {
      if (typeof v !== 'boolean') throw new UsageError(`${name} は boolean`);
      if (v) argv.push(def.flag);
    } else {
      argv.push(def.flag, scalar(name, def.type, v));
    }
  }

  let skipped = null;
  let separated = false;
  for (const p of positionals) {
    const v = args[p.name];
    if (v === undefined || v === null) {
      if (!p.optional) throw new UsageError(`必須の引数が無い: ${p.name}`);
      skipped = skipped || p.name;
      continue;
    }
    if (skipped) throw new UsageError(`${skipped} を省略して後ろの ${p.name} は渡せない`);
    // `-` で始まる値が option と読まれないよう、最初の positional の前に `--` を入れる
    if (!separated) {
      argv.push('--');
      separated = true;
    }
    argv.push(scalar(p.name, p.type, v));
  }
  return argv;
}

function scalar(name, type, v) {
  if (type === 'string' && typeof v === 'string') return v;
  if (type === 'number' && typeof v === 'number' && Number.isFinite(v)) return String(v);
  throw new UsageError(`${name} は ${type}`);
}

function withSandbox(program, programArgv) {
  const raw = process.env.SUPERBASH_SANDBOX;
  if (raw === undefined) return [program, programArgv];
  delete process.env.SUPERBASH_SANDBOX; // tool には渡さない
  let prefix;
  try {
    prefix = JSON.parse(raw);
  } catch (e) {
    throw new Error(`SUPERBASH_SANDBOX が JSON でない: ${e.message}`);
  }
  if (!Array.isArray(prefix) || prefix.length === 0 || prefix.some((a) => typeof a !== 'string')) {
    throw new Error('SUPERBASH_SANDBOX は空でない文字列の配列でなければならない');
  }
  return [prefix[0], [...prefix.slice(1), program, ...programArgv]];
}

function main(argv) {
  const [name, argsJson = '{}'] = argv;
  if (!name || argv.length > 2) {
    throw new UsageError('使い方: superbash-run <command 名> [<args の JSON>]');
  }
  const config = JSON.parse(fs.readFileSync(path.join(ROOT, '.manifest', 'toolconfig.json'), 'utf8'));
  const { top, node, head } = resolveLeaf(config.commands || [], name);
  let args;
  try {
    args = JSON.parse(argsJson);
  } catch (e) {
    throw new UsageError(`args が JSON でない: ${e.message}`);
  }
  const childArgv = buildArgv(node, args, head);
  const def = config.tools[top.tool];
  // module のある tool は、振り分けの file を tool の名前つきで起動する。無ければ entry を直に起動する（互換）。
  const [program, programArgv] = def.module
    ? [path.join(ROOT, 'dispatch', 'run.mjs'), [top.tool, ...childArgv]]
    : [path.join(ROOT, 'tools', top.tool, def.entry), childArgv];

  // --sandbox の時、superbash が組んだ prefix（JSON の文字列の配列。先頭が仕組みの実行 file）を前に置いて起動する。
  // prefix が読めない時は、sandbox 無しで起こさず止める。
  const [exe, exeArgv] = withSandbox(program, programArgv);
  const child = spawn(exe, exeArgv, { stdio: 'inherit' }); // shell を通さない
  child.on('error', (e) => {
    process.stderr.write(`superbash-run: 起動できない: ${e.message}\n`);
    process.exit(127);
  });
  child.on('exit', (code, signal) => {
    if (signal) process.exit(128 + (os.constants.signals[signal] || 0));
    process.exit(code === null ? 1 : code);
  });
}

if (require.main === module) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    process.stderr.write(`superbash-run: ${e.message}\n`);
    process.exit(e instanceof UsageError ? 2 : 1);
  }
}

module.exports = { buildArgv, resolveLeaf };
