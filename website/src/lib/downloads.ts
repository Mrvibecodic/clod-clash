import { gitConfig, releasesUrl } from './shared'

export const repoUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`
export const allReleasesUrl = `${repoUrl}/releases`
export const latestReleaseUrl = releasesUrl
export const latestReleaseApiUrl = `https://api.github.com/repos/${gitConfig.user}/${gitConfig.repo}/releases/latest`

export function latestAssetUrl(file: string) {
  return `${releasesUrl}/download/${file}`
}

export const permanentLinkFiles = [
  'Clod.Clash_x64-setup.exe',
  'Clod.Clash_x64_portable.zip',
  'Clod.Clash_aarch64.dmg',
  'Clod.Clash_x64.dmg',
  'Clod.Clash_amd64.deb',
  'Clod.Clash-x86_64.rpm',
]
