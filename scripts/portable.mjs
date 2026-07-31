import fs from 'fs'
import fsp from 'fs/promises'
import { createRequire } from 'module'
import path from 'path'

import AdmZip from 'adm-zip'

const target = process.argv.slice(2)[0]
const ARCH_MAP = {
  'x86_64-pc-windows-msvc': 'x64',
  'aarch64-pc-windows-msvc': 'arm64',
}

const PROCESS_MAP = {
  x64: 'x64',
  arm64: 'arm64',
}
const arch = target ? ARCH_MAP[target] : PROCESS_MAP[process.arch]
/// Script for ci
/// 打包绿色版/便携版 (only Windows)
async function resolvePortable() {
  if (process.platform !== 'win32') return

  // clod: the fork's cargo workspace lives at the repo root, so the build
  // lands in ./target; keep the upstream location as a fallback.
  const releaseDirCandidates = target
    ? [`./target/${target}/release`, `./src-tauri/target/${target}/release`]
    : [`./target/release`, `./src-tauri/target/release`]
  const releaseDir = releaseDirCandidates.find((dir) => fs.existsSync(dir))
  if (!releaseDir) {
    throw new Error(
      `could not find the release dir (checked: ${releaseDirCandidates.join(', ')})`,
    )
  }
  const configDir = path.join(releaseDir, '.config')

  await fsp.mkdir(configDir, { recursive: true })
  if (!fs.existsSync(path.join(configDir, 'PORTABLE'))) {
    await fsp.writeFile(path.join(configDir, 'PORTABLE'), '')
  }
  const zip = new AdmZip()

  // clod:branding — the binary follows mainBinaryName from tauri.conf.json;
  // fall back to the cargo bin name in case the toolchain leaves it as is.
  const mainExe = ['clod-clash.exe', 'clash-verge.exe']
    .map((name) => path.join(releaseDir, name))
    .find((file) => fs.existsSync(file))
  if (!mainExe) {
    throw new Error('could not find the main executable in the release dir')
  }
  zip.addLocalFile(mainExe)
  zip.addLocalFile(path.join(releaseDir, 'verge-mihomo.exe'))
  zip.addLocalFile(path.join(releaseDir, 'verge-mihomo-alpha.exe'))
  zip.addLocalFolder(path.join(releaseDir, 'resources'), 'resources')
  zip.addLocalFolder(configDir, '.config')

  const require = createRequire(import.meta.url)
  const packageJson = require('../package.json')
  const { version } = packageJson
  // clod:branding — the portable zip carries the fork's name
  const zipFile = `Clod.Clash_${version}_${arch}_portable.zip`
  zip.writeZip(zipFile)
  console.log('[INFO]: create portable zip successfully')
}

resolvePortable().catch((error) => {
  // A missing portable must fail the CI step, not just log.
  console.error(error)
  process.exitCode = 1
})
