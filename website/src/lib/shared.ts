export const appName = 'Clod Clash'
export const docsRoute = '/docs'

export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? ''

export const gitConfig = {
  user: 'Mrvibecodic',
  repo: 'clod-clash',
  branch: 'main',
}

export const community = {
  group: 'https://t.me/+2lmP1yhxpCE3MDcy',
  chat: 'https://t.me/+8BJQXYXYLqM4YWYy',
}

export const siteOrigin = `https://${gitConfig.user.toLowerCase()}.github.io`
export const promoUrl = `${basePath}/screenshots/og.png`
export const releasesUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}/releases/latest`

export const siteMeta = {
  ru: {
    title: 'Clod Clash — клиент для Windows, macOS и Linux',
    description:
      'Клиент для подписок Remnawave на ядре Mihomo: вставьте ссылку на подписку и подключайтесь. Windows, macOS и Linux.',
  },
  en: {
    title: 'Clod Clash — a client for Windows, macOS and Linux',
    description:
      'A Remnawave subscription client on the Mihomo core: paste your subscription link and connect. Windows, macOS and Linux.',
  },
} as const

export type SiteLang = keyof typeof siteMeta

export function metaFor(lang: string) {
  return siteMeta[lang as SiteLang] ?? siteMeta.ru
}
