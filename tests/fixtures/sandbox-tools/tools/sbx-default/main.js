#!/usr/bin/env node
'use strict';
// sandbox の test 用。write <path>: file を書く。connect <port>: 127.0.0.1 へ繋ぐ。env <name>: 環境変数を出す。
// 成功 0、止められた時は 3（理由を stderr に）。
const fs = require('node:fs');
const net = require('node:net');
const [sub, arg] = process.argv.slice(2).filter((a) => a !== '--');
if (sub === 'write') {
  try {
    fs.writeFileSync(arg, 'hello');
    console.log('written');
  } catch (e) {
    console.error(`write failed: ${e.code}`);
    process.exit(3);
  }
} else if (sub === 'connect') {
  const s = net.connect({ host: '127.0.0.1', port: Number(arg) });
  s.on('connect', () => {
    console.log('connected');
    s.destroy();
  });
  s.on('error', (e) => {
    console.error(`connect failed: ${e.code}`);
    process.exit(3);
  });
} else if (sub === 'env') {
  console.log(`env:${process.env[arg] ?? '(unset)'}`);
} else {
  process.exit(2);
}
