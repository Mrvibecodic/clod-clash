import type { SvgIconComponent } from '@mui/icons-material'
import ForkRightRoundedIcon from '@mui/icons-material/ForkRightRounded'
import LanguageRoundedIcon from '@mui/icons-material/LanguageRounded'
import SubjectRoundedIcon from '@mui/icons-material/SubjectRounded'
import WifiRoundedIcon from '@mui/icons-material/WifiRounded'
import { useCallback, useMemo } from 'react'

import { useVerge } from '@/hooks/use-verge'
import type { TranslationKey } from '@/types/generated/i18n-keys'

/**
 * clod:tool-shortcuts — единый список технических экранов.
 *
 * Он один на два места: раздел «Инструменты» в настройках (там же и включение)
 * и ряд ярлыков на главной. Порядок ряда — порядок этого массива, а не порядок
 * нажатий: человек запоминает МЕСТО плитки, и переезд плитки от того, что её
 * включили последней, был бы хуже любого «умного» порядка.
 */
export const TOOL_SHORTCUTS = [
  {
    key: 'proxies',
    path: '/proxies',
    label: 'layout.components.navigation.tabs.proxies',
    hint: 'home.pages.advanced.tiles.proxiesHint',
    Icon: WifiRoundedIcon,
  },
  {
    key: 'rules',
    path: '/rules',
    label: 'layout.components.navigation.tabs.rules',
    hint: 'home.pages.advanced.tiles.rulesHint',
    Icon: ForkRightRoundedIcon,
  },
  {
    key: 'connections',
    path: '/connections',
    label: 'layout.components.navigation.tabs.connections',
    hint: 'home.pages.advanced.tiles.connectionsHint',
    Icon: LanguageRoundedIcon,
  },
  {
    key: 'logs',
    path: '/logs',
    label: 'layout.components.navigation.tabs.logs',
    hint: 'home.pages.advanced.tiles.logsHint',
    Icon: SubjectRoundedIcon,
  },
] as const satisfies readonly {
  /** Ключ хранится в конфиге: не переводится и не меняет порядок. */
  key: string
  path: string
  label: TranslationKey
  hint: TranslationKey
  Icon: SvgIconComponent
}[]

/**
 * Какие инструменты вынесены ярлыками на главную и как это менять.
 *
 * Отсутствие поля в конфиге — это «все четыре», а не «ни одного»: инструменты
 * должны быть под рукой сразу после обновления, а лишние снимаются тумблером.
 * Пустой массив — законное состояние «ни одного», поэтому `undefined` и `[]`
 * здесь значат разное.
 */
export const useToolShortcuts = () => {
  const { verge, patchVerge } = useVerge()
  const stored = verge?.home_tool_shortcuts

  const enabledKeys = useMemo(
    () => new Set<string>(stored ?? TOOL_SHORTCUTS.map((tool) => tool.key)),
    [stored],
  )

  const shortcuts = useMemo(
    () => TOOL_SHORTCUTS.filter((tool) => enabledKeys.has(tool.key)),
    [enabledKeys],
  )

  const isEnabled = useCallback(
    (key: string) => enabledKeys.has(key),
    [enabledKeys],
  )

  const setEnabled = useCallback(
    async (key: string, next: boolean) => {
      const keys = new Set(enabledKeys)
      if (next) {
        keys.add(key)
      } else {
        keys.delete(key)
      }
      // Пишем в порядке TOOL_SHORTCUTS и только известные ключи: конфиг правят
      // руками, и мусор оттуда не должен доживать до главной.
      await patchVerge({
        home_tool_shortcuts: TOOL_SHORTCUTS.filter((tool) =>
          keys.has(tool.key),
        ).map((tool) => tool.key),
      })
    },
    [enabledKeys, patchVerge],
  )

  return { shortcuts, isEnabled, setEnabled }
}
