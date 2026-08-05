import {
  delayProxyByName,
  healthcheckNodeInProvider,
  type ProxyDelay,
} from 'tauri-plugin-mihomo-api'

import { debugLog } from '@/utils/debug'

const hashKey = (name: string, group: string) => `${group ?? ''}::${name}`

export interface DelayUpdate {
  delay: number
  elapsed?: number
  updatedAt: number
}

const CACHE_TTL = 30 * 60 * 1000

/** Used when neither the user, the template nor the settings named a URL. */
const BUILTIN_TEST_URL = 'http://cp.cloudflare.com/generate_204'

class DelayManager {
  private cache = new Map<string, DelayUpdate>()

  /**
   * URL, выбранный пользователем для конкретной группы (страница «Прокси»).
   * Живёт до тех пор, пока пользователь его не сменит.
   */
  private urlMap = new Map<string, string>()

  /**
   * clod: URL групп, вычитанные из работающего конфига (`proxy-groups[].url`).
   *
   * Отдельная карта, а не общая с пользовательской: конфиг перечитывается
   * целиком при каждой смене профиля, и замена этой карты не должна стирать
   * выбор, который пользователь сделал руками.
   */
  private configUrlMap = new Map<string, string>()

  /** `verge.default_latency_test` — общий запасной адрес. */
  private defaultUrl = BUILTIN_TEST_URL

  // Слушатели для каждого узла
  private listenerMap = new Map<string, (update: DelayUpdate) => void>()

  // Слушатели для каждой группы
  private groupListenerMap = new Map<string, () => void>()

  private pendingItemUpdates = new Map<string, DelayUpdate[]>()
  private pendingGroupUpdates = new Set<string>()
  private itemFlushScheduled = false
  private groupFlushScheduled = false

  private scheduleOnNextFrame(run: () => void): void {
    if (typeof window !== 'undefined') {
      if (typeof window.requestAnimationFrame === 'function') {
        window.requestAnimationFrame(run)
        return
      }
      if (typeof window.setTimeout === 'function') {
        window.setTimeout(run, 0)
        return
      }
    }

    Promise.resolve().then(run)
  }

  private scheduleItemFlush() {
    if (this.itemFlushScheduled) return
    this.itemFlushScheduled = true

    this.scheduleOnNextFrame(() => {
      this.itemFlushScheduled = false
      const updates = this.pendingItemUpdates
      this.pendingItemUpdates = new Map()

      updates.forEach((queue, key) => {
        const listener = this.listenerMap.get(key)
        if (!listener) return

        queue.forEach((update) => {
          try {
            listener(update)
          } catch (error) {
            console.error(
              `[DelayManager] Не удалось уведомить слушатель задержки узла: ${key}`,
              error,
            )
          }
        })
      })
    })
  }

  private scheduleGroupFlush() {
    if (this.groupFlushScheduled) return
    this.groupFlushScheduled = true

    this.scheduleOnNextFrame(() => {
      this.groupFlushScheduled = false
      const groups = this.pendingGroupUpdates
      this.pendingGroupUpdates = new Set()

      groups.forEach((group) => {
        const listener = this.groupListenerMap.get(group)
        if (!listener) return
        try {
          listener()
        } catch (error) {
          console.error(
            `[DelayManager] Не удалось уведомить слушатель задержки группы: ${group}`,
            error,
          )
        }
      })
    })
  }

  private queueGroupNotification(group: string) {
    this.pendingGroupUpdates.add(group)
    this.scheduleGroupFlush()
  }

  /**
   * clod: только РЕАЛЬНЫЙ ввод пользователя. Всё, что пришло из конфига или
   * настроек, живёт в `configUrlMap`/`defaultUrl` — положенное сюда переживает
   * смену профиля и затеняет `url:` группы нового конфига.
   */
  setUrl(group: string, url: string) {
    debugLog(
      `[DelayManager] Установлен URL теста, группа: ${group}, URL: ${url}`,
    )
    this.urlMap.set(group, url)
  }

  /** Пользователь очистил своё поле — группа возвращается к `url:` из конфига. */
  clearUrl(group: string) {
    this.urlMap.delete(group)
  }

  /**
   * clod: заменить URL, вычитанные из конфига, целиком.
   *
   * Именно заменить, а не дополнить: при смене профиля группа с тем же именем,
   * но без своего `url:`, не должна унаследовать адрес из прошлого профиля —
   * иначе и тест, и чтение истории пойдут не туда. Пользовательских URL это
   * не касается, они в другой карте.
   */
  replaceConfigUrls(urls: Map<string, string>) {
    this.configUrlMap = new Map(urls)
  }

  /** Общий запасной адрес из настроек. Пустое значение возвращает встроенный. */
  setDefaultUrl(url?: string) {
    this.defaultUrl = url?.trim() || BUILTIN_TEST_URL
  }

  /**
   * Адрес, по которому эту группу и надо проверять.
   *
   * Порядок: выбор пользователя для этой группы → `url:` группы из конфига →
   * общий адрес из настроек → встроенный. Одно место на всё приложение: если
   * тест и чтение истории разойдутся в адресе, пинг покажется как «—».
   */
  getUrl(group: string) {
    // Горячий путь: `getDelayFix` зовёт нас из компаратора сортировки и на
    // каждую строку списка — никакой интерполяции строк здесь быть не должно.
    return (
      this.urlMap.get(group) ?? this.configUrlMap.get(group) ?? this.defaultUrl
    )
  }

  setListener(
    name: string,
    group: string,
    listener: (update: DelayUpdate) => void,
  ) {
    const key = hashKey(name, group)
    this.listenerMap.set(key, listener)
  }

  removeListener(name: string, group: string) {
    const key = hashKey(name, group)
    this.listenerMap.delete(key)
  }

  setGroupListener(group: string, listener: () => void) {
    this.groupListenerMap.set(group, listener)
  }

  removeGroupListener(group: string) {
    this.groupListenerMap.delete(group)
  }

  setDelay(
    name: string,
    group: string,
    delay: number,
    meta?: { elapsed?: number },
  ): DelayUpdate {
    const key = hashKey(name, group)
    debugLog(
      `[DelayManager] Установлена задержка, прокси: ${name}, группа: ${group}, задержка: ${delay}`,
    )
    const update: DelayUpdate = {
      delay,
      elapsed: meta?.elapsed,
      updatedAt: Date.now(),
    }

    this.cache.set(key, update)

    const queue = this.pendingItemUpdates.get(key)
    if (queue) {
      queue.push(update)
    } else {
      this.pendingItemUpdates.set(key, [update])
    }
    this.scheduleItemFlush()

    return update
  }

  getDelayUpdate(name: string, group: string) {
    const key = hashKey(name, group)
    const entry = this.cache.get(key)
    if (!entry) return undefined

    if (Date.now() - entry.updatedAt > CACHE_TTL) {
      this.cache.delete(key)
      return undefined
    }

    return { ...entry }
  }

  getDelay(name: string, group: string) {
    const update = this.getDelayUpdate(name, group)
    return update ? update.delay : -1
  }

  /// Временный фикс сортировки задержки узлов у provider
  getDelayFix(proxy: IProxyItem, group: string) {
    if (!proxy.provider) {
      const update = this.getDelayUpdate(proxy.name, group)
      if (update && (update.delay >= 0 || update.delay === -2)) {
        return update.delay
      }
    }

    // clod: тест шёл по URL самой группы (`proxy-groups[].url` из шаблона
    // провайдера), и ядро складывает такие замеры не в `history`, а в
    // `extra[url]`. Без этого «Тест» показывал пинг до дефолтного адреса —
    // то есть не то, что реально происходит с YouTube-группой.
    const extraEntry = proxy.extra?.[this.getUrl(group)]?.history?.at(-1)
    const historyEntry = proxy.history?.at(-1)

    // Берём более свежий замер: старая запись в `extra` не должна перебивать
    // только что снятый пинг по дефолтному адресу (и наоборот).
    const newest =
      extraEntry && historyEntry
        ? Date.parse(extraEntry.time) >= Date.parse(historyEntry.time)
          ? extraEntry
          : historyEntry
        : (extraEntry ?? historyEntry)

    if (newest) {
      // 0ms отображаем как error
      return newest.delay || 1e6
    }
    return -1
  }

  /**
   * Когда сняли тот замер, который видит пользователь: мс epoch, 0 — не знаем.
   *
   * Смотрит туда же, куда и `getDelayFix` (история по адресу группы и обычная
   * история), и берёт более свежую запись. Нужен, чтобы отличить «пинг просто
   * старый» от «пинга нет»: цифра часовой давности выглядит на экране ровно
   * так же, как свежая, и молча врёт.
   */
  getMeasuredAt(proxy: IProxyItem, group: string) {
    const times = [
      proxy.extra?.[this.getUrl(group)]?.history?.at(-1)?.time,
      proxy.history?.at(-1)?.time,
    ]
      .map((time) => (time ? Date.parse(time) : Number.NaN))
      .filter((time) => Number.isFinite(time))

    // Свой кэш — там же, где его читает `getDelayFix`, и читает ПЕРВЫМ:
    // после ручного теста на экране висит цифра отсюда, и её возраст обязан
    // считаться отсюда же, иначе свежий замер выглядит протухшим.
    if (!proxy.provider) {
      const update = this.getDelayUpdate(proxy.name, group)
      if (update && update.delay >= 0) times.push(update.updatedAt)
    }

    return times.length > 0 ? Math.max(...times) : 0
  }

  // Единая проверка задержки
  async unifiedDelayCheck(
    name: string,
    url: string,
    timeout: number,
    providerName?: string,
  ) {
    if (providerName)
      return healthcheckNodeInProvider(providerName, name, url, timeout)
    return delayProxyByName(name, url, timeout)
  }

  async checkDelay(
    name: string,
    group: string,
    timeout: number,
    providerName?: string,
  ): Promise<DelayUpdate> {
    debugLog(
      `[DelayManager] Начало теста задержки, прокси: ${name}, группа: ${group}, тайм-аут: ${timeout}ms`,
    )

    // Сначала выставляем статус «тестируется»
    this.setDelay(name, group, -2)

    const startTime = Date.now()

    try {
      const url = this.getUrl(group)
      debugLog(
        `[DelayManager] Вызов API для теста задержки, прокси: ${name}, URL: ${url}`,
      )

      // Обрабатываем таймаут, delay = 0 означает таймаут
      const timeoutPromise = new Promise<ProxyDelay>((resolve) => {
        setTimeout(() => resolve({ delay: 0 }), timeout)
      })

      // Используем Promise.race для контроля таймаута
      const result = await Promise.race([
        this.unifiedDelayCheck(name, url, timeout, providerName),
        timeoutPromise,
      ])

      // Гарантируем показ анимации загрузки не менее 500мс
      const elapsedTime = Date.now() - startTime
      if (elapsedTime < 500) {
        await new Promise((resolve) => setTimeout(resolve, 500 - elapsedTime))
      }

      const delay = result.delay
      const elapsed = elapsedTime
      debugLog(
        `[DelayManager] Тест задержки завершён, прокси: ${name}, результат: ${delay}ms`,
      )

      return this.setDelay(name, group, delay, { elapsed })
    } catch (error) {
      // Гарантируем показ анимации загрузки не менее 500мс
      await new Promise((resolve) => setTimeout(resolve, 500))
      console.error(
        `[DelayManager] Ошибка теста задержки, прокси: ${name}`,
        error,
      )
      const delay = 1e6 // error
      const elapsed = Date.now() - startTime

      return this.setDelay(name, group, delay, { elapsed })
    }
  }

  async checkListDelay(
    proxies: IProxyItem[],
    group: string,
    timeout: number,
    concurrency = 36,
  ) {
    debugLog(
      `[DelayManager] Начало пакетного теста задержки, группа: ${group}, количество: ${proxies.length}, параллельность: ${concurrency}`,
    )
    const names = proxies.map((p) => p.name)
    // Выставляем статус «идёт тест задержки»
    names.forEach((name) => {
      this.setDelay(name, group, -2)
    })

    let index = 0
    const startTime = Date.now()
    const listener = this.groupListenerMap.get(group)

    const help = async (): Promise<void> => {
      const currProxy = proxies[index++]
      if (!currProxy) return
      const currName = currProxy.name
      const currProviderName = currProxy.provider

      try {
        // Убеждаемся, что перед вызовом API статус «тестируется»
        this.setDelay(currName, group, -2)

        // Добавляем случайную задержку, чтобы запросы не уходили и не возвращались одновременно
        if (index > 1) {
          // Первый запрос без задержки — для отзывчивости
          await new Promise((resolve) =>
            setTimeout(resolve, Math.random() * 200),
          )
        }

        await this.checkDelay(currName, group, timeout, currProviderName)
        if (listener) {
          this.queueGroupNotification(group)
        }
      } catch (error) {
        console.error(
          `[DelayManager] Ошибка теста отдельного прокси в пакете, прокси: ${currName}`,
          error,
        )
        // Выставляем статус ошибки
        this.setDelay(currName, group, 1e6)
      }

      return help()
    }

    // Ограничиваем число одновременных запросов
    const actualConcurrency = Math.min(concurrency, names.length, 10)
    debugLog(`[DelayManager] Фактическая параллельность: ${actualConcurrency}`)

    const promiseList: Promise<void>[] = []
    for (let i = 0; i < actualConcurrency; i++) {
      promiseList.push(help())
    }

    await Promise.all(promiseList)
    const totalTime = Date.now() - startTime
    debugLog(
      `[DelayManager] Пакетный тест задержки завершён, группа: ${group}, общее время: ${totalTime}ms`,
    )
  }

  formatDelay(delay: number, timeout = 10000) {
    if (delay === -1) return '-'
    if (delay === -2) return 'testing'
    if (delay === 0 || (delay >= timeout && delay <= 1e5)) return 'Timeout'
    if (delay > 1e5) return 'Error'
    return `${delay}`
  }

  formatDelayColor(delay: number, timeout = 10000) {
    if (delay < 0) return ''
    if (delay === 0 || delay >= timeout) return 'error.main'
    if (delay >= 10000) return 'error.main'
    if (delay >= 400) return 'warning.main'
    if (delay >= 250) return 'primary.main'
    return 'success.main'
  }
}

export default new DelayManager()
