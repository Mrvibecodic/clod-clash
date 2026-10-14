import type { Metadata } from 'next'
import Link from 'next/link'
import {
  ReleaseCard,
  type ReleaseCardText,
  type ReleasePlatform,
} from '@/components/release-card'
import {
  allReleasesUrl,
  latestAssetUrl,
  permanentLinkFiles,
} from '@/lib/downloads'
import { basePath, community } from '@/lib/shared'

interface Copy {
  title: string
  lead: string
  card: ReleaseCardText
  platforms: ReleasePlatform[]
  stepsTitle: string
  steps: string[]
  docsLink: string
  updatesTitle: string
  updates: string
  releases: string
  announces: string
  linksTitle: string
  linksLead: string
}

const copy: Record<'ru' | 'en', Copy> = {
  ru: {
    title: 'Скачать',
    lead:
      'Последняя обычная версия для Windows, macOS и Linux. Ссылки постоянные: когда выходит новая версия, они начинают вести на неё, менять закладку не нужно.',
    card: {
      locale: 'ru-RU',
      pending: 'последняя версия',
      date: 'от',
      notes: 'Что нового →',
      download: 'Скачать',
      yours: 'похоже, ваша система',
      megabytes: 'МБ',
    },
    platforms: [
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
    ],
    stepsTitle: 'Что дальше',
    steps: [
      'Установите приложение и запустите его.',
      'Вставьте ссылку на подписку.',
      'Выберите сервер и нажмите кнопку подключения.',
    ],
    docsLink: 'Подробная инструкция по установке',
    updatesTitle: 'Обновления',
    updates:
      'Приложение проверяет обновления само и предлагает поставить новую версию. Вручную: Настройки → Основные → строка «Версия Clod» → «Проверить обновления».',
    releases: 'Все релизы на GitHub',
    announces: 'Анонсы версий в Telegram-группе',
    linksTitle: 'Постоянные ссылки',
    linksLead:
      'В адресе нет номера версии: GitHub сам отдаёт файл из последнего обычного релиза. Предварительные сборки под эту ссылку не попадают.',
  },
  en: {
    title: 'Download',
    lead:
      'The latest regular release for Windows, macOS and Linux. The links are permanent: when a new version comes out they lead to it, so there is no bookmark to change.',
    card: {
      locale: 'en-US',
      pending: 'latest version',
      date: 'from',
      notes: 'What is new →',
      download: 'Download',
      yours: 'looks like your system',
      megabytes: 'MB',
    },
    platforms: [
      {
        id: 'windows',
        title: 'Windows',
        assets: [
          {
            file: 'Clod.Clash_x64-setup.exe',
            title: 'Installer',
            note: 'Installs the app and the background service for TUN mode.',
            featured: true,
          },
          {
            file: 'Clod.Clash_x64_portable.zip',
            title: 'Portable build',
            note: 'No installation. The TUN service is not installed; rights are requested when TUN is turned on.',
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
            note: 'Computers with M1 and newer.',
            featured: true,
          },
          {
            file: 'Clod.Clash_x64.dmg',
            title: 'Intel',
            note: 'Mac computers with Intel processors.',
          },
        ],
      },
      {
        id: 'linux',
        title: 'Linux',
        assets: [
          {
            file: 'Clod.Clash_amd64.deb',
            title: '.deb package',
            note: 'Debian, Ubuntu, Mint and related systems.',
            featured: true,
          },
          {
            file: 'Clod.Clash-x86_64.rpm',
            title: '.rpm package',
            note: 'Fedora, openSUSE and related systems.',
          },
        ],
      },
    ],
    stepsTitle: 'What next',
    steps: [
      'Install the app and start it.',
      'Paste your subscription link.',
      'Pick a server and press the connect button.',
    ],
    docsLink: 'Detailed installation guide',
    updatesTitle: 'Updates',
    updates:
      'The app checks for updates by itself and offers to install a new version. By hand: Settings → General → the “Clod version” row → “Check for updates”.',
    releases: 'All releases on GitHub',
    announces: 'Release announcements in the Telegram group',
    linksTitle: 'Permanent links',
    linksLead:
      'The address has no version number: GitHub serves the file from the latest regular release. Pre-release builds are not behind these links.',
  },
}

const textFor = (lang: string) => copy[lang as keyof typeof copy] ?? copy.ru

export async function generateMetadata({
  params,
}: PageProps<'/[lang]/download'>): Promise<Metadata> {
  const { lang } = await params
  const text = textFor(lang)

  return {
    title: text.title,
    description: text.lead,
    alternates: {
      canonical: `${basePath}/${lang}/download/`,
      languages: { ru: `${basePath}/ru/download/`, en: `${basePath}/en/download/` },
    },
  }
}

export default async function DownloadPage({ params }: PageProps<'/[lang]/download'>) {
  const { lang } = await params
  const text = textFor(lang)

  return (
    <main className="flex flex-1 flex-col items-center px-6 py-16">
      <div className="flex w-full max-w-3xl flex-col gap-10">
        <div className="flex flex-col gap-4">
          <h1 className="text-4xl font-bold tracking-tight">{text.title}</h1>
          <p className="text-lg text-fd-muted-foreground">{text.lead}</p>
        </div>

        <ReleaseCard platforms={text.platforms} text={text.card} />

        <div className="grid gap-6 md:grid-cols-2">
          <div className="rounded-2xl border p-6">
            <h2 className="mb-3 font-semibold">{text.stepsTitle}</h2>
            <ol className="flex list-decimal flex-col gap-2 pl-5 text-sm text-fd-muted-foreground">
              {text.steps.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ol>
            <Link
              href={`/${lang}/docs/user/install`}
              className="mt-4 inline-block text-sm font-medium text-fd-primary hover:underline"
            >
              {text.docsLink} →
            </Link>
          </div>

          <div className="rounded-2xl border p-6">
            <h2 className="mb-3 font-semibold">{text.updatesTitle}</h2>
            <p className="text-sm text-fd-muted-foreground">{text.updates}</p>
            <a
              href={allReleasesUrl}
              className="mt-4 inline-block text-sm font-medium text-fd-primary hover:underline"
            >
              {text.releases} →
            </a>
            <a
              href={community.group}
              className="mt-2 block text-sm font-medium text-fd-primary hover:underline"
            >
              {text.announces} →
            </a>
          </div>
        </div>

        <div className="rounded-2xl border p-6">
          <h2 className="mb-3 font-semibold">{text.linksTitle}</h2>
          <p className="mb-4 text-sm text-fd-muted-foreground">{text.linksLead}</p>
          <pre className="whitespace-pre-wrap break-all rounded-xl bg-fd-muted p-4 font-mono text-xs leading-6">
            {permanentLinkFiles.map((file) => `${latestAssetUrl(file)}\n`).join('')}
          </pre>
        </div>
      </div>
    </main>
  )
}
