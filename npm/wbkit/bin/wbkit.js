#!/usr/bin/env node
// Runs the native wbkit program that npm installed for this system (one optional package per platform).
const { spawnSync } = require('node:child_process')
const name = `@webboxfun/wbkit-${process.platform}-${process.arch}`
let file
try {
  file = require.resolve(`${name}/bin/wbkit${process.platform === 'win32' ? '.exe' : ''}`)
} catch {
  console.error(`wbkit: no program for ${process.platform}-${process.arch} was installed (${name}).\n` +
    'Reinstall without --no-optional, or use https://github.com/thejamesnick/wbkit#install')
  process.exit(1)
}
const result = spawnSync(file, process.argv.slice(2), { stdio: 'inherit' })
if (result.error) { console.error(`wbkit: ${result.error.message}`); process.exit(1) }
process.exit(result.status ?? 1)
