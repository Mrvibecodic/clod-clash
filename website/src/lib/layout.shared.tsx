import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared'
import { Logo } from '@/components/logo'
import { appName, community, gitConfig } from './shared'

const titles: Record<string, { download: string; group: string; chat: string }> = {
  ru: { download: 'Скачать', group: 'Группа', chat: 'Чат' },
  en: { download: 'Download', group: 'Group', chat: 'Chat' },
}

export function baseOptions(lang: string): BaseLayoutProps {
  const text = titles[lang] ?? titles.ru

  return {
    nav: {
      title: (
        <span className="flex items-center gap-2 font-semibold">
          <Logo className="size-6" />
          {appName}
        </span>
      ),
      url: `/${lang}`,
    },
    links: [
      { text: text.download, url: `/${lang}/download` },
      { text: text.group, url: community.group },
      { text: text.chat, url: community.chat },
    ],
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  }
}
