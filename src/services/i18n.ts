import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import {
  FALLBACK_LANGUAGE,
  localeTag,
  resolveLanguage,
  supportedLanguages,
} from '@/utils/language'
import { setTrafficLocale } from '@/utils/parse-traffic'

export { FALLBACK_LANGUAGE, resolveLanguage, supportedLanguages }

const LANGUAGE_STORAGE_KEY = 'verge-language'

const getLanguageStorage = () => {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

export const cacheLanguage = (language: string) => {
  const storage = getLanguageStorage()
  if (!storage) return

  try {
    storage.setItem(LANGUAGE_STORAGE_KEY, resolveLanguage(language))
  } catch (error) {
    console.warn('[i18n] Failed to cache language:', error)
  }
}

/**
 * clod:language — забыть запомненный язык, если это был не выбор пользователя.
 *
 * `changeLanguage` запоминает язык всегда — иначе первая отрисовка после
 * перезапуска ждала бы ответа бэкенда. Но если конфиг на этом запуске не
 * прочитался, запомнена всего лишь системная локаль, и оставлять её нельзя:
 * при следующем запуске кэш отвечает раньше конфига и перебивает выбор.
 */
export const forgetCachedLanguage = () => {
  const storage = getLanguageStorage()
  if (!storage) return

  try {
    storage.removeItem(LANGUAGE_STORAGE_KEY)
  } catch (error) {
    console.warn('[i18n] Failed to forget cached language:', error)
  }
}

export const getCachedLanguage = () => {
  const storage = getLanguageStorage()
  if (!storage) return undefined

  try {
    const cached = storage.getItem(LANGUAGE_STORAGE_KEY)
    return cached ? resolveLanguage(cached) : undefined
  } catch (error) {
    console.warn('[i18n] Failed to read cached language:', error)
    return undefined
  }
}

type LocaleModule = {
  default: Record<string, unknown>
}

const STARTUP_LANGUAGE_SECTIONS = [
  'layout',
  'home',
  'shared',
  'settings',
  'profiles',
  'proxies',
  // clod:freeze — значок «режется» живёт на главной и в прокси.
  'freeze',
] as const

const localeModules = import.meta.glob<LocaleModule>('@/locales/*/*.json')

const localeLoaders = Object.entries(localeModules).reduce<
  Record<string, Record<string, () => Promise<LocaleModule>>>
>((acc, [path, loader]) => {
  const match = path.match(/[/\\]locales[/\\]([^/\\]+)[/\\]([^/\\]+)\.json$/)
  if (match) {
    const [, language, section] = match
    acc[language] ??= {}
    acc[language][section] = loader
  }
  return acc
}, {})

const loadLanguageSections = async (
  language: string,
  sections: readonly string[],
) => {
  try {
    const entries = await Promise.all(
      sections.map(async (section) => {
        const loader = localeLoaders[language]?.[section]
        if (!loader) {
          throw new Error(
            `Locale loader not found for language "${language}" section "${section}"`,
          )
        }

        const module = await loader()
        return [section, module.default] as const
      }),
    )

    return Object.fromEntries(entries)
  } catch (error) {
    if (language !== FALLBACK_LANGUAGE) {
      console.warn(
        `Failed to load language ${language}, fallback to ${FALLBACK_LANGUAGE}, ${error}`,
      )
      return loadLanguageSections(FALLBACK_LANGUAGE, sections)
    }
    throw error
  }
}

const getLoadedLanguageSections = (language: string) =>
  Object.keys(i18n.getResourceBundle(language, 'translation') ?? {})

i18n.use(initReactI18next).init({
  resources: {},
  lng: FALLBACK_LANGUAGE,
  fallbackLng: FALLBACK_LANGUAGE,
  interpolation: {
    escapeValue: false,
  },
})

export const ensureLanguageSections = async (
  sections: string | readonly string[],
  language: string = i18n.language || FALLBACK_LANGUAGE,
) => {
  const targetLanguage = resolveLanguage(language)
  const sectionList = Array.isArray(sections) ? sections : [sections]
  const loadedSections = new Set(getLoadedLanguageSections(targetLanguage))
  const missingSections = sectionList.filter(
    (section) => !loadedSections.has(section),
  )

  if (!missingSections.length) {
    return
  }

  const resources = await loadLanguageSections(targetLanguage, missingSections)
  i18n.addResourceBundle(targetLanguage, 'translation', resources, true, true)
}

export const changeLanguage = async (language: string) => {
  const targetLanguage = resolveLanguage(language)
  const loadedSections = getLoadedLanguageSections(
    i18n.language || FALLBACK_LANGUAGE,
  )

  await ensureLanguageSections(
    loadedSections.length ? loadedSections : STARTUP_LANGUAGE_SECTIONS,
    targetLanguage,
  )

  await i18n.changeLanguage(targetLanguage)
  // Диктор читает страницу голосом её языка.
  document.documentElement.lang = localeTag(targetLanguage)
  setTrafficLocale(localeTag(targetLanguage))
  cacheLanguage(targetLanguage)
}

export const initializeLanguage = async (
  initialLanguage: string = FALLBACK_LANGUAGE,
) => {
  await changeLanguage(initialLanguage)
}
