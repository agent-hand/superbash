#!/usr/bin/env node
// OS・CPU ごとの package の実行 file を、shell を通さず直接起動する。引数と stdio は素通し。
"use strict";
const { spawn } = require("node:child_process");

const key = `${process.platform}-${process.arch}`;
const pkg = `superbash-${key}`;

let bin;
try {
  bin = require.resolve(`${pkg}/bin/superbash`);
} catch {
  process.stderr.write(
    `superbash: ${key} 用の package（${pkg}）が入っていない。` +
      `対象は darwin-arm64・darwin-x64・linux-x64・linux-arm64。\n`,
  );
  process.exit(1);
}

const child = spawn(bin, process.argv.slice(2), { stdio: "inherit" });
for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => child.kill(sig));
}
child.on("error", (e) => {
  process.stderr.write(`superbash: 起動できない: ${e.message}\n`);
  process.exit(1);
});
child.on("exit", (code, signal) => {
  if (signal) {
    process.removeAllListeners(signal);
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});
