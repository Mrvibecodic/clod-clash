import { i18nProvider } from 'fumadocs-ui/i18n'
import type { Metadata } from 'next'
import { Inter } from 'next/font/google'
import { Provider } from '@/components/provider'
import { i18n, translations } from '@/lib/i18n'
import { appName, basePath, metaFor, promoUrl, siteOrigin } from '@/lib/shared'

const inter = Inter({ subsets: ['latin', 'cyrillic'] })

export async function generateMetadata({
  params,
}: LayoutProps<'/[lang]'>): Promise<Metadata> {
  const { lang } = await params
  const meta = metaFor(lang)

  return {
    metadataBase: new URL(siteOrigin),
    alternates: {
      canonical: `${basePath}/${lang}/`,
      languages: { ru: `${basePath}/ru/`, en: `${basePath}/en/` },
    },
    title: { default: meta.title, template: `%s · ${appName}` },
    description: meta.description,
    applicationName: appName,
    openGraph: {
      type: 'website',
      siteName: appName,
      locale: lang === 'ru' ? 'ru_RU' : 'en_US',
      url: `${basePath}/${lang}/`,
      title: meta.title,
      description: meta.description,
      images: [{ url: promoUrl, alt: appName }],
    },
  }
}

export default async function Layout({ params, children }: LayoutProps<'/[lang]'>) {
  const { lang } = await params

  return (
    <html lang={lang} className={inter.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider i18n={i18nProvider(translations, lang)}>{children}</Provider>
      </body>
    </html>
  )
}

export function generateStaticParams() {
  return i18n.languages.map((lang) => ({ lang }))
}
