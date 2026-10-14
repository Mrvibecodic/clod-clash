import Link from 'next/link'
import { Logo } from '@/components/logo'
import { basePath, community } from '@/lib/shared'

const copy = {
  ru: {
    tagline: 'Клиент для Windows, macOS и Linux: вставьте ссылку на подписку и подключайтесь',
    lead:
      'Ядро Mihomo встроено в приложение. Список серверов, остаток трафика и срок действия ' +
      'приходят из вашей подписки, подключение — одной кнопкой.',
    docs: 'Документация',
    download: 'Скачать',
    group: 'Telegram-группа',
    groupNote: '— новости и релизы',
    chat: 'Telegram-чат',
    chatNote: '— помощь и обсуждение',
    shotAlt: 'Главный экран приложения в подключённом состоянии',
    points: [
      ['Одна кнопка', 'Системный прокси или режим TUN для всех программ — включается одним нажатием'],
      ['Свои серверы', 'Выбор сервера, замер задержки, избранное и автовыбор самого быстрого'],
      ['Три системы', 'Windows, macOS и Linux, тёмная тема, трей и запуск вместе с системой'],
    ],
  },
  en: {
    tagline: 'A client for Windows, macOS and Linux: paste your subscription link and connect',
    lead:
      'The Mihomo core is bundled with the app. Server list, remaining traffic and expiry come ' +
      'from your subscription, and you connect with a single button.',
    docs: 'Documentation',
    download: 'Download',
    group: 'Telegram group',
    groupNote: '— news and releases',
    chat: 'Telegram chat',
    chatNote: '— help and discussion',
    shotAlt: 'The app home screen while connected',
    points: [
      ['One button', 'System proxy or TUN mode for every program — on with a single click'],
      ['Your servers', 'Pick a server, measure latency, keep favourites or let it pick the fastest'],
      ['Three systems', 'Windows, macOS and Linux, dark theme, tray and start with the system'],
    ],
  },
} as const

export default async function HomePage({ params }: PageProps<'/[lang]'>) {
  const { lang } = await params
  const text = copy[lang as keyof typeof copy] ?? copy.ru

  return (
    <main className="flex flex-1 flex-col items-center px-4 py-16">
      <div className="flex w-full max-w-5xl flex-col items-center gap-12 md:flex-row md:items-center md:justify-between">
        <div className="flex max-w-xl flex-col gap-6 text-center md:text-left">
          <div className="flex flex-col items-center gap-4 md:flex-row md:items-center">
            <Logo className="size-16 rounded-2xl shadow-lg" />
            <h1 className="text-4xl font-bold tracking-tight md:text-5xl">Clod Clash</h1>
          </div>
          <p className="text-xl text-fd-muted-foreground">{text.tagline}</p>
          <p className="text-fd-muted-foreground">{text.lead}</p>
          <div className="flex flex-row flex-wrap justify-center gap-3 md:justify-start">
            <Link
              href={`/${lang}/docs`}
              className="rounded-full bg-fd-primary px-6 py-2.5 font-medium text-fd-primary-foreground"
            >
              {text.docs}
            </Link>
            <Link
              href={`/${lang}/download`}
              className="rounded-full border border-fd-border px-6 py-2.5 font-medium"
            >
              {text.download}
            </Link>
          </div>
          <div className="flex flex-row flex-wrap justify-center gap-x-2 gap-y-1 text-sm text-fd-muted-foreground md:justify-start">
            <a
              href={community.group}
              className="font-medium text-fd-foreground underline underline-offset-4"
            >
              {text.group}
            </a>
            <span>{text.groupNote}</span>
            <span aria-hidden>·</span>
            <a
              href={community.chat}
              className="font-medium text-fd-foreground underline underline-offset-4"
            >
              {text.chat}
            </a>
            <span>{text.chatNote}</span>
          </div>
        </div>
        <img
          src={`${basePath}/screenshots/ru/home-connected.png`}
          alt={text.shotAlt}
          width={320}
          className="rounded-2xl border border-fd-border shadow-2xl"
        />
      </div>

      <div className="mt-20 grid w-full max-w-5xl gap-6 md:grid-cols-3">
        {text.points.map(([title, description]) => (
          <div key={title} className="rounded-2xl border border-fd-border p-6">
            <h2 className="mb-2 font-semibold">{title}</h2>
            <p className="text-sm text-fd-muted-foreground">{description}</p>
          </div>
        ))}
      </div>
    </main>
  )
}
