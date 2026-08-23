import Image from 'next/image'
import Link from 'next/link'
import { Logo } from '@/components/logo'
import { releasesUrl } from '@/lib/shared'
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
        <a
          href={releasesUrl}
          className="rounded-full bg-fd-primary px-6 py-2.5 font-medium text-fd-primary-foreground"
        >
          Скачать
        </a>
        <Link
          href="/docs"
          className="rounded-full border px-6 py-2.5 font-medium hover:bg-fd-accent"
        >
          Документация
        </Link>
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
