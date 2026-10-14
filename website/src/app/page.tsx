import type { Metadata } from 'next'
import { i18n } from '@/lib/i18n'
import { basePath } from '@/lib/shared'

const target = `${basePath}/${i18n.defaultLanguage}/`

export const metadata: Metadata = {
  robots: { index: false, follow: false },
}

export default function RootRedirectPage() {
  return (
    <html lang={i18n.defaultLanguage}>
      <head>
        <meta httpEquiv="refresh" content={`0; url=${target}`} />
      </head>
      <body className="flex min-h-screen flex-col items-center justify-center gap-2 text-center">
        <p>Перенаправление… / Redirecting…</p>
        <a className="font-medium underline" href={target}>
          {target}
        </a>
      </body>
    </html>
  )
}
