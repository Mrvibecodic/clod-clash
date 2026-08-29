import Image from 'next/image'
import Link from 'next/link'
import { Logo } from '@/components/logo'
import { community } from '@/lib/shared'
import hero from '../../../public/screenshots/02-home-connected.png'

export default function HomePage() {
  return (
    <main className="flex flex-1 flex-col items-center px-6 py-16 text-center">
      <Logo className="size-20 rounded-3xl shadow-lg" />
      <h1 className="mt-6 text-4xl font-bold tracking-tight">Clod Clash</h1>
      <p className="mt-4 max-w-xl text-lg text-fd-muted-foreground">
        Клиент для подписок Remnawave на ядре Mihomo. Ссылка на подписку, одна
        кнопка — и всё работает. Windows, macOS и Linux.
      </p>
      <div className="mt-8 flex flex-wrap justify-center gap-3">
        <Link
          href="/download"
          className="rounded-full bg-fd-primary px-6 py-2.5 font-medium text-fd-primary-foreground"
        >
          Скачать
        </Link>
        <Link
          href="/docs"
          className="rounded-full border px-6 py-2.5 font-medium hover:bg-fd-accent"
        >
          Документация
        </Link>
      </div>
      <div className="mt-5 flex flex-wrap items-center justify-center gap-x-2 gap-y-1 text-sm text-fd-muted-foreground">
        <a
          href={community.group}
          className="font-medium text-fd-foreground underline underline-offset-4"
        >
          Telegram-группа
        </a>
        <span>— новости и релизы</span>
        <span aria-hidden>·</span>
        <a
          href={community.chat}
          className="font-medium text-fd-foreground underline underline-offset-4"
        >
          Telegram-чат
        </a>
        <span>— помощь и обсуждение</span>
      </div>
      <Image
        src={hero}
        alt="Главный экран Clod Clash"
        className="mt-14 w-full max-w-md rounded-2xl border shadow-2xl"
        priority
      />
    </main>
  )
}
