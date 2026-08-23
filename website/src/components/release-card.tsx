'use client'

import { useEffect, useState } from 'react'
import {
  latestAssetUrl,
  latestReleaseApiUrl,
  latestReleaseUrl,
} from '@/lib/downloads'

export type PlatformId = 'windows' | 'macos' | 'linux'

export interface ReleaseAsset {
  file: string
  title: string
  note: string
  featured?: boolean
}

export interface ReleasePlatform {
  id: PlatformId
  title: string
  assets: ReleaseAsset[]
}

interface ReleaseCardProps {
  platforms: ReleasePlatform[]
}

interface ReleaseInfo {
  version: string
  published: string
  sizes: Record<string, number>
}

function readRelease(payload: unknown): ReleaseInfo | null {
  if (typeof payload !== 'object' || payload === null) return null

  const data = payload as {
    tag_name?: unknown
    published_at?: unknown
    assets?: unknown
  }
  if (typeof data.tag_name !== 'string') return null

  const sizes: Record<string, number> = {}
  if (Array.isArray(data.assets)) {
    for (const item of data.assets) {
      const asset = item as { name?: unknown; size?: unknown }
      if (typeof asset.name === 'string' && typeof asset.size === 'number') {
        sizes[asset.name] = asset.size
      }
    }
  }

  return {
    version: data.tag_name.replace(/^clod-v/, ''),
    published: typeof data.published_at === 'string' ? data.published_at : '',
    sizes,
  }
}

function detectPlatform(): PlatformId | null {
  if (typeof navigator === 'undefined') return null
  const agent = navigator.userAgent
  if (/Windows/i.test(agent)) return 'windows'
  if (/Mac OS X|Macintosh/i.test(agent)) return 'macos'
  if (/Linux|X11/i.test(agent)) return 'linux'
  return null
}

function formatSize(bytes: number | undefined): string {
  if (!bytes) return ''
  const value = new Intl.NumberFormat('ru-RU', {
    maximumFractionDigits: 1,
  }).format(bytes / 1024 / 1024)
  return `${value} МБ`
}

function formatDate(value: string): string {
  if (!value) return ''
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) return ''
  return new Intl.DateTimeFormat('ru-RU', {
    day: 'numeric',
    month: 'long',
    year: 'numeric',
  }).format(parsed)
}

export function ReleaseCard({ platforms }: ReleaseCardProps) {
  const [release, setRelease] = useState<ReleaseInfo | null>(null)
  const [current, setCurrent] = useState<PlatformId | null>(null)

  useEffect(() => {
    setCurrent(detectPlatform())
  }, [])

  useEffect(() => {
    let active = true

    fetch(latestReleaseApiUrl, {
      headers: { Accept: 'application/vnd.github+json' },
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((payload) => {
        const info = readRelease(payload)
        if (active && info) setRelease(info)
      })
      .catch(() => undefined)

    return () => {
      active = false
    }
  }, [])

  const published = release ? formatDate(release.published) : ''

  return (
    <div className="flex w-full flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border bg-fd-card px-6 py-4">
        <div className="flex flex-wrap items-center gap-3">
          <span className="rounded-full bg-fd-primary px-3 py-1 text-sm font-semibold tabular-nums text-fd-primary-foreground">
            {release ? release.version : 'последняя версия'}
          </span>
          {published ? (
            <span className="text-sm text-fd-muted-foreground">
              от {published}
            </span>
          ) : null}
        </div>
        <a
          href={latestReleaseUrl}
          className="text-sm font-medium text-fd-primary hover:underline"
        >
          Что нового →
        </a>
      </div>

      {platforms.map((platform) => (
        <section
          key={platform.id}
          className={
            current === platform.id
              ? 'overflow-hidden rounded-2xl border border-fd-primary bg-fd-card'
              : 'overflow-hidden rounded-2xl border bg-fd-card'
          }
        >
          <header className="flex flex-wrap items-center gap-3 border-b px-6 py-3">
            <h2 className="font-semibold">{platform.title}</h2>
            {current === platform.id ? (
              <span className="rounded-full border border-fd-primary px-2 py-0.5 text-xs text-fd-primary">
                похоже, ваша система
              </span>
            ) : null}
          </header>

          <ul className="divide-y">
            {platform.assets.map((asset) => (
              <li
                key={asset.file}
                className="flex flex-wrap items-center gap-x-4 gap-y-3 px-6 py-4"
              >
                <div className="min-w-56 flex-1">
                  <div className="font-medium">{asset.title}</div>
                  <div className="mt-1 font-mono text-xs text-fd-muted-foreground">
                    {asset.file}
                  </div>
                  <div className="mt-1 text-sm text-fd-muted-foreground">
                    {asset.note}
                  </div>
                </div>
                <span className="w-20 text-right text-sm tabular-nums text-fd-muted-foreground">
                  {formatSize(release?.sizes[asset.file])}
                </span>
                <a
                  href={latestAssetUrl(asset.file)}
                  className={
                    asset.featured
                      ? 'rounded-full bg-fd-primary px-5 py-2 text-sm font-medium text-fd-primary-foreground'
                      : 'rounded-full border px-5 py-2 text-sm font-medium hover:bg-fd-accent'
                  }
                >
                  Скачать
                </a>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  )
}
