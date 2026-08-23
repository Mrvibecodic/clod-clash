import type { Metadata } from 'next'
import Link from 'next/link'
import { ReleaseCard, type ReleasePlatform } from '@/components/release-card'
import {
  allReleasesUrl,
  latestAssetUrl,
  permanentLinkFiles,
} from '@/lib/downloads'

const lead =
  'Последняя обычная версия для Windows, macOS и Linux. Ссылки постоянные: когда выходит новая версия, они начинают вести на неё, менять закладку не нужно.'

export const metadata: Metadata = {
  title: 'Скачать',
  description: lead,
}

const platforms: ReleasePlatform[] = [
  {
    id: 'windows',
    title: 'Windows',
    assets: [
      {
        file: 'Clod.Clash_x64-setup.exe',
        title: 'Установщик',
        note: 'Ставит приложение и фоновую службу для режима TUN.',
        featured: true,
      },
      {
        file: 'Clod.Clash_x64_portable.zip',
        title: 'Портативная сборка',
        note: 'Без установки. Служба TUN не ставится, права запрашиваются при включении.',
      },
    ],
  },
  {
    id: 'macos',
    title: 'macOS',
    assets: [
      {
        file: 'Clod.Clash_aarch64.dmg',
        title: 'Apple Silicon',
        note: 'Компьютеры на M1 и новее.',
        featured: true,
      },
      {
        file: 'Clod.Clash_x64.dmg',
        title: 'Intel',
        note: 'Компьютеры Mac на процессорах Intel.',
      },
    ],
  },
  {
    id: 'linux',
    title: 'Linux',
    assets: [
      {
        file: 'Clod.Clash_amd64.deb',
        title: 'Пакет .deb',
        note: 'Debian, Ubuntu, Mint и родственные системы.',
        featured: true,
      },
      {
        file: 'Clod.Clash-x86_64.rpm',
        title: 'Пакет .rpm',
        note: 'Fedora, openSUSE и родственные системы.',
      },
    ],
  },
]

export default function DownloadPage() {
  return (
    <main className="flex flex-1 flex-col items-center px-6 py-16">
      <div className="flex w-full max-w-3xl flex-col gap-10">
        <div className="flex flex-col gap-4">
          <h1 className="text-4xl font-bold tracking-tight">Скачать</h1>
          <p className="text-lg text-fd-muted-foreground">{lead}</p>
        </div>

        <ReleaseCard platforms={platforms} />

        <div className="grid gap-6 md:grid-cols-2">
          <div className="rounded-2xl border p-6">
            <h2 className="mb-3 font-semibold">Что дальше</h2>
            <ol className="flex list-decimal flex-col gap-2 pl-5 text-sm text-fd-muted-foreground">
              <li>Установите приложение и запустите его.</li>
              <li>Вставьте ссылку на подписку.</li>
              <li>Выберите сервер и нажмите кнопку подключения.</li>
            </ol>
            <Link
              href="/docs/install"
              className="mt-4 inline-block text-sm font-medium text-fd-primary hover:underline"
            >
              Подробная инструкция по установке →
            </Link>
          </div>

          <div className="rounded-2xl border p-6">
            <h2 className="mb-3 font-semibold">Обновления</h2>
            <p className="text-sm text-fd-muted-foreground">
              Приложение проверяет обновления само и предлагает поставить новую
              версию. Вручную: Настройки → Продвинутые настройки → Проверить
              обновления.
            </p>
            <a
              href={allReleasesUrl}
              className="mt-4 inline-block text-sm font-medium text-fd-primary hover:underline"
            >
              Все релизы на GitHub →
            </a>
          </div>
        </div>

        <div className="rounded-2xl border p-6">
          <h2 className="mb-3 font-semibold">Постоянные ссылки</h2>
          <p className="mb-4 text-sm text-fd-muted-foreground">
            В адресе нет номера версии: GitHub сам отдаёт файл из последнего
            обычного релиза. Предварительные сборки под эту ссылку не попадают.
          </p>
          <pre className="whitespace-pre-wrap break-all rounded-xl bg-fd-muted p-4 font-mono text-xs leading-6">
            {permanentLinkFiles.map((file) => `${latestAssetUrl(file)}\n`).join('')}
          </pre>
        </div>
      </div>
    </main>
  )
}
