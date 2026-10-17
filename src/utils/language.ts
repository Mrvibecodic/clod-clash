export const supportedLanguages = [
  'en',
  'ru',
  'zh',
  'fa',
  'tt',
  'id',
  'ar',
  'ko',
  'tr',
  'de',
  'es',
  'jp',
  'zhtw',
]

export const FALLBACK_LANGUAGE = 'en'

/**
 * Псевдонимы языков системы — та же таблица, что у трея и уведомлений
 * (Rust, `clash-verge-i18n`): окно и трей обязаны выбрать один язык. Сюда же —
 * языки, чьим носителям русский понятнее английского.
 */
const ALIASES: Record<string, string> = {
  ja: 'jp',
  'ja-jp': 'jp',
  'zh-cn': 'zh',
  'zh-hans': 'zh',
  'zh-sg': 'zh',
  'zh-my': 'zh',
  'zh-chs': 'zh',
  'zh-tw': 'zhtw',
  'zh-hk': 'zhtw',
  'zh-hant': 'zhtw',
  'zh-mo': 'zhtw',
  'zh-cht': 'zhtw',
  uk: 'ru',
  be: 'ru',
  kk: 'ru',
  uz: 'ru',
  ky: 'ru',
  tg: 'ru',
}

const normalizeLanguage = (language?: string) =>
  language?.toLowerCase().replace(/_/g, '-')

export const resolveLanguage = (language?: string) => {
  const segments = normalizeLanguage(language)?.split('-') ?? []
  for (let length = segments.length; length > 0; length--) {
    const prefix = segments.slice(0, length).join('-')
    const known = ALIASES[prefix] ?? prefix
    if (supportedLanguages.includes(known)) return known
  }
  return FALLBACK_LANGUAGE
}

const LOCALE_TAGS: Record<string, string> = { jp: 'ja', zhtw: 'zh-TW' }

/** Код языка интерфейса как метка BCP-47 — для `lang` страницы и `Intl`. */
export const localeTag = (language: string) => LOCALE_TAGS[language] ?? language
