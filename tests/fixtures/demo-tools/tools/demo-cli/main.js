#!/usr/bin/env node
'use strict';
// MCP の test 用の command。subcommand ごとに挙動が違う。
const [sub, ...rest] = process.argv.slice(2);
if (sub === 'hello') {
  console.log(`hello ${rest[0]}`);
} else if (sub === 'fail') {
  console.error('boom');
  process.exit(3);
} else if (sub === 'nap') {
  const ms = Number(rest[rest.indexOf('--ms') + 1]);
  console.log('started');
  setTimeout(() => console.log('finished'), ms);
} else {
  console.error(`unknown subcommand: ${sub}`);
  process.exit(2);
}
