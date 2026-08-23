import type { Metadata } from 'next'
import { Inter } from 'next/font/google'
import { Provider } from '@/components/provider'
import './global.css'

const inter = Inter({ subsets: ['latin', 'cyrillic'] })

export const metadata: Metadata = {
  title: { default: 'Clod Clash', template: '%s · Clod Clash' },
  description:
    'Клиент для подписок Remnawave на ядре Mihomo: Windows, macOS и Linux.',
  metadataBase: new URL('https://mrvibecodic.github.io/clod-clash/'),
  openGraph: { images: ['/screenshots/og.png'] },
}

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <html lang="ru" className={inter.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider>{children}</Provider>
      </body>
    </html>
  )
}
