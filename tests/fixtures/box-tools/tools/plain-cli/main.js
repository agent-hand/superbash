#!/usr/bin/env node
'use strict';
// module の無い tool。argv を JSON で 1 行出す。
console.log(JSON.stringify({ via: 'entry', argv: process.argv.slice(2) }));
