// Builds the npm packages for one release from the downloaded binaries:
//   node npm/build.mjs <version> <dir with wbkit-<platform>-<arch>[.exe]>  ->  npm/out/<package folders>
// The main package depends on one optional package per platform; npm installs only the matching one.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync, chmodSync } from 'node:fs'
import { join } from 'node:path'
const [version, binaries] = process.argv.slice(2)
if (!/^\d+\.\d+\.\d+/.test(version ?? '') || !binaries) throw new Error('usage: node npm/build.mjs <version> <binaries dir>')
const platforms = [['darwin', 'arm64'], ['darwin', 'x64'], ['linux', 'arm64'], ['linux', 'x64'], ['win32', 'arm64'], ['win32', 'x64']]
const here = new URL('.', import.meta.url).pathname, out = join(here, 'out')
rmSync(out, { recursive: true, force: true })
const main = JSON.parse(readFileSync(join(here, 'wbkit/package.json'), 'utf8'))
main.version = version
for (const [os, cpu] of platforms) {
  const name = `@webboxfun/wbkit-${os}-${cpu}`, exe = os === 'win32' ? '.exe' : '', dir = join(out, `wbkit-${os}-${cpu}`)
  mkdirSync(join(dir, 'bin'), { recursive: true })
  cpSync(join(binaries, `wbkit-${os}-${cpu}${exe}`), join(dir, 'bin', `wbkit${exe}`))
  if (!exe) chmodSync(join(dir, 'bin', 'wbkit'), 0o755)
  writeFileSync(join(dir, 'package.json'), JSON.stringify({ name, version, description: `The wbkit program for ${os}-${cpu}. Install @webboxfun/wbkit instead.`, repository: main.repository, license: 'MIT', os: [os], cpu: [cpu], files: ['bin'] }, null, 2) + '\n')
  main.optionalDependencies[name] = version
}
cpSync(join(here, 'wbkit'), join(out, 'wbkit'), { recursive: true })
cpSync(join(here, '..', 'README.md'), join(out, 'wbkit', 'README.md'))
writeFileSync(join(out, 'wbkit', 'package.json'), JSON.stringify(main, null, 2) + '\n')
console.log(`npm packages for ${version} in ${out}`)
