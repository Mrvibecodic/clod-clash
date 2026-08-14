import { getVergeConfig, patchVergeConfig } from './cmds'
import {
  cacheLanguage,
  changeLanguage,
  forgetCachedLanguage,
  getCachedLanguage,
  initializeLanguage,
  resolveLanguage,
} from './i18n'

let vergeConfigCache: IVergeConfig | null | undefined

const detectSystemTheme = (): 'light' | 'dark' => {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function')
    return 'light'
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light'
}

const getThemeModeFromWindow = (): IVergeConfig['theme_mode'] | undefined => {
  if (typeof window === 'undefined') return undefined
  const mode = (
    window as typeof window & {
      __VERGE_INITIAL_THEME_MODE?: unknown
    }
  ).__VERGE_INITIAL_THEME_MODE
  if (mode === 'light' || mode === 'dark' || mode === 'system') {
    return mode
  }
  return undefined
}

export const resolveThemeMode = (
  vergeConfig?: IVergeConfig | null,
): 'light' | 'dark' => {
  const initialMode = vergeConfig?.theme_mode ?? getThemeModeFromWindow()
  if (initialMode === 'dark' || initialMode === 'light') {
    return initialMode
  }
  return detectSystemTheme()
}

export const setPreloadConfig = (config: IVergeConfig | null) => {
  vergeConfigCache = config
}

export const getPreloadConfig = () => vergeConfigCache

const preloadConfig = async () => {
  try {
    const config = await getVergeConfig()
    setPreloadConfig(config)
    return config
  } catch (error) {
    console.warn('[preload.ts] Failed to read Verge config:', error)
    setPreloadConfig(null)
    return null
  }
}

const preloadLanguage = async (
  vergeConfig?: IVergeConfig | null,
  loadConfig: () => Promise<IVergeConfig | null> = preloadConfig,
) => {
  const cachedLanguage = getCachedLanguage()
  if (cachedLanguage) {
    return cachedLanguage
  }

  let resolvedConfig = vergeConfig

  if (resolvedConfig === undefined) {
    try {
      resolvedConfig = await loadConfig()
    } catch (error) {
      console.warn(
        '[preload.ts] Failed to read language from Verge config:',
        error,
      )
      resolvedConfig = null
    }
  }

  const languageFromConfig = resolvedConfig?.language
  if (languageFromConfig) {
    const resolved = resolveLanguage(languageFromConfig)
    cacheLanguage(resolved)
    return resolved
  }

  // clod:language — системная локаль здесь НЕ запоминается.
  //
  // Раньше её клали в кэш браузера наравне с выбором пользователя, а кэш при
  // следующем запуске главнее конфига. Достаточно было одного запуска, на
  // котором конфиг не успел прочитаться (на Linux это заметили после
  // обновления, когда хранилище вебвью уезжает вместе со сборкой), — и язык
  // системы закреплялся навсегда, перебивая выбранный. Теперь это лишь
  // временный ответ до того, как ответит конфиг.
  return resolveLanguage(
    typeof navigator !== 'undefined' ? navigator.language : undefined,
  )
}

/**
 * clod:language — конфиг главнее кэша, а первый запуск закрепляет выбор.
 *
 * Кэш в хранилище вебвью нужен ровно для первой отрисовки: спрашивать бэкенд
 * до неё — это белый экран. Но право решать у него забрано: как только конфиг
 * прочитан, язык берётся из него. Если же языка в конфиге нет вовсе (первая
 * установка), туда записывается тот, с которым приложение открылось, — дальше
 * он не зависит ни от системной локали, ни от хранилища вебвью.
 */
const reconcileLanguage = async (
  config: IVergeConfig | null,
  applied: string,
) => {
  // Конфиг не прочитался — ничего не решаем и СТИРАЕМ запомненное: иначе
  // догадка этого запуска пережила бы перезапуск и стала бы главнее конфига.
  if (!config) {
    forgetCachedLanguage()
    return
  }

  const fromConfig = config.language ? resolveLanguage(config.language) : ''
  if (fromConfig) {
    if (fromConfig === applied) {
      cacheLanguage(fromConfig)
      return
    }
    try {
      await changeLanguage(fromConfig)
    } catch (error) {
      console.warn('[preload.ts] Failed to apply language from config:', error)
    }
    return
  }

  try {
    await patchVergeConfig({ language: applied })
    cacheLanguage(applied)
  } catch (error) {
    console.warn('[preload.ts] Failed to store the initial language:', error)
  }
}

export const preloadAppData = async () => {
  const configPromise = preloadConfig()
  const initialLanguage = await preloadLanguage(undefined, () => configPromise)
  const [config] = await Promise.all([
    configPromise,
    initializeLanguage(initialLanguage),
  ])
  await reconcileLanguage(config, initialLanguage)
  const initialThemeMode = resolveThemeMode(config)
  return { initialThemeMode }
}
