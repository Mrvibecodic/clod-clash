import type { Metadata } from 'next'
import { i18n } from '@/lib/i18n'
import { basePath } from '@/lib/shared'

const home = `${basePath}/${i18n.defaultLanguage}/`

// Адреса до перехода сайта на два языка и деления документации на
// «Пользователю» и «Провайдеру»
const moved: Record<string, string> = {
  'docs': 'ru/docs/',
  'download': 'ru/download/',
  'docs/install': 'ru/docs/user/install/',
  'docs/first-run': 'ru/docs/user/subscription/',
  'docs/connection': 'ru/docs/user/connection/',
  'docs/interface': 'ru/docs/user/home/',
  'docs/subscriptions': 'ru/docs/user/subscription/',
  'docs/troubleshooting': 'ru/docs/user/errors/',
  'docs/support': 'ru/docs/user/support/',
  'docs/community': 'ru/docs/user/help/',
  'docs/providers': 'ru/docs/provider/panel/',
  'docs/providers/headers': 'ru/docs/provider/headers/',
  'docs/providers/clod-headers': 'ru/docs/provider/clod-headers/',
}

const redirect = `(function(){var m=${JSON.stringify(moved)};var b=${JSON.stringify(basePath)};var p=location.pathname;if(p.indexOf(b)===0)p=p.slice(b.length);p=p.replace(/^\\/+|\\/+$/g,'');if(m[p])location.replace(b+'/'+m[p]+location.hash);})();`

export const metadata: Metadata = {
  title: '404',
  robots: { index: false, follow: false },
}

export default function NotFound() {
  return (
    <html lang={i18n.defaultLanguage}>
      <head>
        {/* biome-ignore lint/security/noDangerouslySetInnerHtml: статичный скрипт переадресации */}
        <script dangerouslySetInnerHTML={{ __html: redirect }} />
      </head>
      <body className="flex flex-col min-h-screen items-center justify-center gap-2 text-center">
        <h1 className="text-2xl font-bold">404</h1>
        <p>
          Страница не найдена / Page not found — <a href={home}>{home}</a>
        </p>
      </body>
    </html>
  )
}
