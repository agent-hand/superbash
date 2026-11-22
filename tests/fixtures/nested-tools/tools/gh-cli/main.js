#!/usr/bin/env node
'use strict';
// test 用の command。help は spec の要約を出し、それ以外は受け取った argv を JSON で 1 行出す。
const argv = process.argv.slice(2);
if (argv[0] === 'help') {
  console.log('lint-cli: see .manifest/toolconfig.json');
} else {
  console.log(JSON.stringify(argv));
}
