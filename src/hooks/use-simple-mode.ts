import { useCallback } from 'react'

import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import { resolveSimpleMode } from '@/utils/simple-mode'

export const useSimpleMode = () => {
  const { verge, patchVerge } = useVerge()
  const { current } = useProfiles()

  const userChoice = verge?.simple_mode
  const providerChoice = current?.simple_mode

  const simpleMode = resolveSimpleMode(userChoice, providerChoice)

  const setSimpleMode = useCallback(
    async (enabled: boolean) => {
      // clod:mode-window — геометрию окна здесь больше не трогаем. Каждый
      // режим по-прежнему помнит свой размер, но применять его по одной этой
      // кнопке было мало: режим меняет ещё и заголовок панели, и тогда окно
      // оставалось чужого размера. Сохранение покидаемого режима и применение
      // нового переехали в `useModeWindowSize` — он смотрит на РЕЗУЛЬТАТ, а не
      // на способ смены.
      await patchVerge({ simple_mode: enabled })
    },
    [patchVerge],
  )

  return {
    /** The mode actually in effect. */
    simpleMode,
    setSimpleMode,
  }
}
