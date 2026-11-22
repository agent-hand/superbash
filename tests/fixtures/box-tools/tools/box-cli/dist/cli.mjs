#!/usr/bin/env node
import { run } from './main.mjs';
process.exitCode = await run(process.argv.slice(2));
