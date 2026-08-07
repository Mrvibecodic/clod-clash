import {
  check,
  type CheckOptions,
  type Update,
} from '@tauri-apps/plugin-updater'

import { getVergeConfig } from '@/services/cmds'
import { version as appVersion } from '@root/package.json'

type VersionParts = {
  main: number[]
  pre: (number | string)[]
}

const SEMVER_FULL_REGEX =
  /^\d+(?:\.\d+){1,2}(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/
const SEMVER_SEARCH_REGEX =
  /v?\d+(?:\.\d+){1,2}(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?/i

const normalizeVersion = (input: string | null | undefined): string | null => {
  if (typeof input !== 'string') return null
  const trimmed = input.trim()
  if (!trimmed) return null
  return trimmed.replace(/^v/i, '')
}

const ensureSemver = (input: string | null | undefined): string | null => {
  const normalized = normalizeVersion(input)
  if (!normalized) return null
  return SEMVER_FULL_REGEX.test(normalized) ? normalized : null
}

const extractSemver = (input: string | null | undefined): string | null => {
  if (typeof input !== 'string') return null
  const match = input.match(SEMVER_SEARCH_REGEX)
  if (!match) return null
  return normalizeVersion(match[0])
}

const splitVersion = (version: string | null): VersionParts | null => {
  if (!version) return null
  const [mainPart, preRelease] = version.split('-')
  const main = mainPart
    .split('.')
    .map((part) => Number.parseInt(part, 10))
    .map((num) => (Number.isNaN(num) ? 0 : num))

  const pre =
    preRelease?.split('.').map((token) => {
      const numeric = Number.parseInt(token, 10)
      return Number.isNaN(numeric) ? token : numeric
    }) ?? []

  return { main, pre }
}

const compareVersionParts = (a: VersionParts, b: VersionParts): number => {
  const length = Math.max(a.main.length, b.main.length)
  for (let i = 0; i < length; i += 1) {
    const diff = (a.main[i] ?? 0) - (b.main[i] ?? 0)
    if (diff !== 0) return diff > 0 ? 1 : -1
  }

  if (a.pre.length === 0 && b.pre.length === 0) return 0
  if (a.pre.length === 0) return 1
  if (b.pre.length === 0) return -1

  const preLen = Math.max(a.pre.length, b.pre.length)
  for (let i = 0; i < preLen; i += 1) {
    const aToken = a.pre[i]
    const bToken = b.pre[i]
    if (aToken === undefined) return -1
    if (bToken === undefined) return 1

    if (typeof aToken === 'number' && typeof bToken === 'number') {
      if (aToken > bToken) return 1
      if (aToken < bToken) return -1
      continue
    }

    if (typeof aToken === 'number') return -1
    if (typeof bToken === 'number') return 1

    if (aToken > bToken) return 1
    if (aToken < bToken) return -1
  }

  return 0
}

const compareVersions = (a: string | null, b: string | null): number | null => {
  const partsA = splitVersion(a)
  const partsB = splitVersion(b)
  if (!partsA || !partsB) return null
  return compareVersionParts(partsA, partsB)
}

const resolveRemoteVersion = (update: Update): string | null => {
  const primary = ensureSemver(update.version)
  if (primary) return primary

  const fallbackPrimary = extractSemver(update.version)
  if (fallbackPrimary) return fallbackPrimary

  const raw = update.rawJson ?? {}
  const rawVersion = ensureSemver(
    typeof raw.version === 'string' ? raw.version : null,
  )
  if (rawVersion) return rawVersion

  const tagVersion = extractSemver(
    typeof raw.tag_name === 'string' ? raw.tag_name : null,
  )
  if (tagVersion) return tagVersion

  const nameVersion = extractSemver(
    typeof raw.name === 'string' ? raw.name : null,
  )
  if (nameVersion) return nameVersion

  return null
}

const localVersionNormalized = normalizeVersion(appVersion)

// clod:prereleases — версия помечена как предварительная semver-суффиксом
// (`0.0.24-alpha`, `1.0.0-rc.1`). Тот же признак, что и на стороне Rust:
// манифест обновления один на все каналы, и различить сборки можно только по
// номеру версии.
const isPrereleaseVersion = (version: string | null | undefined): boolean => {
  const parts = splitVersion(normalizeVersion(version))
  return !!parts && parts.pre.length > 0
}

// clod:prereleases — «получать предварительные сборки» спрашиваем здесь, а не
// у трёх вызывающих: проверка обновлений идёт и из настроек, и из диалога, и
// из фонового хука, и разъехавшийся ответ был бы хуже лишнего чтения конфига.
const prereleasesAllowed = async (): Promise<boolean> => {
  try {
    const verge = await getVergeConfig()
    return verge?.receive_prereleases !== false
  } catch (err) {
    // Не смогли спросить — ведём себя как раньше: пропускаем обновление.
    console.warn('[updater] failed to read the pre-release preference', err)
    return true
  }
}

const discard = async (result: Update): Promise<null> => {
  try {
    await result.close()
  } catch (err) {
    console.warn('[updater] failed to close stale update resource', err)
  }
  return null
}

export const checkUpdateSafe = async (
  options?: CheckOptions,
): Promise<Update | null> => {
  const result = await check({ ...(options ?? {}), allowDowngrades: false })
  if (!result) return null

  const remoteVersion = resolveRemoteVersion(result)
  const comparison = compareVersions(remoteVersion, localVersionNormalized)

  if (comparison !== null && comparison <= 0) {
    return discard(result)
  }

  if (isPrereleaseVersion(remoteVersion) && !(await prereleasesAllowed())) {
    return discard(result)
  }

  return result
}

export type { CheckOptions }
