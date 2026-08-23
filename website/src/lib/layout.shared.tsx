import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared'
import { Logo } from '@/components/logo'
import { appName, gitConfig, releasesUrl } from './shared'

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="flex items-center gap-2 font-semibold">
          <Logo className="size-6" />
          {appName}
        </span>
      ),
    },
    links: [
      { text: 'Документация', url: '/docs', active: 'nested-url' },
      { text: 'Скачать', url: releasesUrl, external: true },
    ],
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  }
}
