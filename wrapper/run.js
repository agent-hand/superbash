#!/usr/bin/env node
'use strict';
// superbash の wrapper（MCP の mode 用）。
// 使い方: superbash-run <command 名> [<args の JSON>]
// toolconfig.json の spec.in の規則で argv の配列を組み、command の bin を shell を通さず起動する。

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..');

class UsageError extends Error {}

// argv = [subcommand] + options（定義の順。boolean は true の時だけ flag、string/number は flag と値の 2 引数）
//        + positionals（定義の順。省略できるのは optional のものだけ）
function buildArgv(command, args) {
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

  const argv = [];
  if (spec.subcommand) argv.push(spec.subcommand);

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
  for (const p of positionals) {
    const v = args[p.name];
    if (v === undefined || v === null) {
      if (!p.optional) throw new UsageError(`必須の引数が無い: ${p.name}`);
      skipped = skipped || p.name;
      continue;
    }
    if (skipped) throw new UsageError(`${skipped} を省略して後ろの ${p.name} は渡せない`);
    argv.push(scalar(p.name, p.type, v));
  }
  return argv;
}

function scalar(name, type, v) {
  if (type === 'string' && typeof v === 'string') return v;
  if (type === 'number' && typeof v === 'number' && Number.isFinite(v)) return String(v);
  throw new UsageError(`${name} は ${type}`);
}

function main(argv) {
  const [name, argsJson = '{}'] = argv;
  if (!name || argv.length > 2) {
    throw new UsageError('使い方: superbash-run <command 名> [<args の JSON>]');
  }
  const config = JSON.parse(fs.readFileSync(path.join(ROOT, '.manifest', 'toolconfig.json'), 'utf8'));
  const command = (config.commands || []).find((c) => c.name === name);
  if (!command) throw new UsageError(`command が無い: ${name}`);
  let args;
  try {
    args = JSON.parse(argsJson);
  } catch (e) {
    throw new UsageError(`args が JSON でない: ${e.message}`);
  }
  const childArgv = buildArgv(command, args);
  const entry = path.join(ROOT, 'tools', command.tool, config.tools[command.tool].entry);

  const child = spawn(entry, childArgv, { stdio: 'inherit' }); // shell を通さない
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

module.exports = { buildArgv };
