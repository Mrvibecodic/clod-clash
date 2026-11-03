import { ensureTunReady } from '@/services/cmds'
import { prepareTunWith } from '@/utils/prepare-tun'

import { useSystemState } from './use-system-state'
import { useTunState } from './use-tun-state'
import { useVerge } from './use-verge'

/**
 * clod:tun-ready — включение TUN, одно на тумблеры и кнопку «Подключить».
 * Ошибки уходят вызывающему как есть: слова для них и признак занятости — у
 * него.
 */
export const useTunSwitch = () => {
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { mutateSystemState } = useSystemState()
  const { mutateTunState } = useTunState()

  /**
   * Довести систему до TUN: службу бэкенд ставит, запускает или чинит сам
   * (один запрос прав). `false` — человек отказал. Отказ бэкенда («идёт
   * выход», «уже спрашиваем права») приходит исключением с меткой: его надо
   * показать словами, а не звать ставить уже стоящую службу. Состояние
   * службы и TUN перечитывается при любом исходе (`prepareTunWith`).
   */
  const prepareTun = () =>
    prepareTunWith(ensureTunReady, () =>
      Promise.all([mutateSystemState(), mutateTunState()]),
    )

  /**
   * Тумблер TUN руками. Он же выбор способа для кнопки «Подключить»
   * (`connect_tun_mode`) — пишется той же записью настроек.
   */
  const switchTun = async (value: boolean) => {
    mutateVerge({ ...verge, enable_tun_mode: value })
    try {
      await patchVerge({ enable_tun_mode: value, connect_tun_mode: value })
    } catch (err) {
      // Ошибка не обязательно значит откат: бэкенд мог сохранить настройку и
      // всё равно сообщить об отказе следующего шага. Перечитываем конфиг —
      // в кэше не должно остаться оптимистичное значение, факт узнаём у
      // бэкенда.
      mutateVerge()
      throw err
    } finally {
      // Тумблер показывает факт — спрашиваем его у бэкенда после патча. В
      // `finally`, а не в `try`: провал этого запроса не делает патч неудачным
      // и не должен откатывать переключатель.
      await mutateTunState().catch(() => undefined)
    }
  }

  /**
   * Починить TUN, который человек уже включил (`wanted` — по состоянию
   * бэкенда, а не по кэшу окна): служба, затем повторная подача
   * той же настройки — ядро пересобирается с туннелем, подавление снимается.
   * Способ подключения не трогаем: это не тумблер. После отказа в правах
   * настройку не подаём — ядро пошло бы на заведомо провальную попытку.
   */
  const repairTun = async (wanted: boolean) => {
    const ready = await prepareTun()
    if (!ready || !wanted) return ready
    try {
      await patchVerge({ enable_tun_mode: true })
    } finally {
      await mutateTunState().catch(() => undefined)
    }
    return ready
  }

  return { prepareTun, switchTun, repairTun }
}
