import {
  delayProxyByName,
  healthcheckNodeInProvider,
} from 'tauri-plugin-mihomo-api'

import { debugLog } from '@/utils/debug'
import { delayColor } from '@/utils/delay-color'
import { isValidUrl } from '@/utils/network'

const hashKey = (name: string, group: string) => `${group ?? ''}::${name}`

export interface DelayUpdate {
  delay: number
  elapsed?: number
  updatedAt: number
  /** У метки «идёт проверка» (-2) — замер, который она заслонила. */
  before?: DelayUpdate
}

const CACHE_TTL = 30 * 60 * 1000

/**
 * How long a `-2` ("testing") cache entry may outrank a real measurement.
 *
 * A test refreshes the marker right before it calls the core, so a live one is
 * never older than the request timeout. Anything older is a leftover of a test
 * that never came back (unmounted screen, core restart) — without this cap it
 * would win the freshness comparison forever and blank the ping on every
 * screen that reads this cache.
 */
const TESTING_TTL = 60 * 1000

/**
 * clod:cache-evict — потолок числа записей о задержках и как часто убираться.
 *
 * Тысячи хватает с запасом: это все узлы нескольких крупных подписок сразу,
 * а прицел у потолка не на экономию памяти, а на то, чтобы карта не росла
 * бесконечно у человека, который месяцами не перезапускает приложение.
 */
const MAX_CACHE_ENTRIES = 1000
const EVICT_EVERY = 200

const MAX_PARALLEL_CHECKS = 10

/** Used when neither the user, the template nor the settings named a URL. */
const BUILTIN_TEST_URL = 'http://cp.cloudflare.com/generate_204'

/**
 * Тайм-аут проверки из настроек. Ядро разбирает его как int16, поэтому потолок
 * 32767. Сохранённое раньше вне границ прижимается к ближайшей, чтобы короткий
 * тайм-аут не превращался в 10 с; не заданное, нулевое и отрицательное — умолчание.
 */
export const LATENCY_TIMEOUT_MIN = 1000
export const LATENCY_TIMEOUT_MAX = 32767
const LATENCY_TIMEOUT_DEFAULT = 10000

export const effectiveLatencyTimeout = (value?: number) =>
  value !== undefined && Number.isInteger(value) && value > 0
    ? Math.min(Math.max(value, LATENCY_TIMEOUT_MIN), LATENCY_TIMEOUT_MAX)
    : LATENCY_TIMEOUT_DEFAULT

class DelayManager {
  private cache = new Map<string, DelayUpdate>()

  /** Записей с последней уборки; см. `evictStaleDelays`. */
  private writesSinceEvict = 0

  private freeCheckSlots = MAX_PARALLEL_CHECKS
  /** Сколько пакетных проверок идёт по группе: пока идёт, сортировка по задержке стоит. */
  private checkingGroups = new Map<string, number>()
  private queuedChecks = new Set<string>()
  private checkSlotWaiters: (() => void)[] = []

  /**
   * URL, выбранный пользователем для конкретной группы (страница «Прокси»).
   * Свой у каждой подписки: `setProfile` меняет карту целиком.
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

  /** Подписка, к которой относятся замеры в кэше. */
  private profile = ''

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
   * настроек, живёт в `configUrlMap`/`defaultUrl` — положенное сюда затеняло
   * бы `url:` группы и после обновления подписки.
   */
  setUrl(group: string, url: string) {
    // Ядро принимает только полный адрес со схемой: негодный даёт ошибку по
    // всем узлам сразу. Каждый источник адреса проверяется на входе — читают
    // его на каждую строку списка, и проверять на чтении было бы дорого.
    if (!isValidUrl(url)) {
      debugLog(
        `[DelayManager] URL теста отклонён, группа: ${group}, URL: ${url}`,
      )
      this.clearUrl(group)
      return
    }
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
    this.configUrlMap = new Map([...urls].filter(([, url]) => isValidUrl(url)))
  }

  /**
   * clod: подписка сменилась. Ключ кэша — «группа::узел», и у одноимённых узлов
   * новой подписки иначе полчаса висел бы пинг прошлой. Адреса, заданные человеком
   * на «Прокси», хранятся по подписке — берём её набор целиком, а не ждём, пока
   * заголовок группы попадёт на экран.
   */
  setProfile(uid: string, userUrls: Map<string, string>) {
    if (this.profile && this.profile !== uid) this.cache.clear()
    this.profile = uid
    this.urlMap = new Map([...userUrls].filter(([, url]) => isValidUrl(url)))
  }

  /** Общий запасной адрес из настроек. Пустое значение возвращает встроенный. */
  setDefaultUrl(url?: string) {
    const chosen = url?.trim()
    this.defaultUrl = chosen && isValidUrl(chosen) ? chosen : BUILTIN_TEST_URL
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
    if (delay === -2) {
      const prev = this.cache.get(key)
      update.before = prev && prev.delay >= 0 ? prev : prev?.before
    }

    this.cache.set(key, update)
    this.evictStaleDelays()

    const queue = this.pendingItemUpdates.get(key)
    if (queue) {
      queue.push(update)
    } else {
      this.pendingItemUpdates.set(key, [update])
    }
    this.scheduleItemFlush()

    return update
  }

  /**
   * clod:cache-evict — выбросить протухшее, не дожидаясь чтения.
   *
   * `getDelayUpdate` удаляет запись старше `CACHE_TTL`, но только ту, которую
   * СПРОСИЛИ. Узлы, пропавшие из конфига при смене профиля или обновлении
   * подписки, не спрашивает уже никто: их ключи оставались в карте до
   * перезагрузки окна, и у пользователя с десятком подписок по сотне серверов
   * она росла весь сеанс. Чистим на записи — там же, где карта и растёт.
   *
   * Полный проход раз в `EVICT_EVERY` записей: сама чистка дешевле, чем
   * проверять возраст всей карты на каждый замер, а «Проверить все» на большой
   * группе — это сотни вызовов подряд.
   */
  private evictStaleDelays() {
    this.writesSinceEvict += 1
    if (
      this.writesSinceEvict < EVICT_EVERY &&
      this.cache.size <= MAX_CACHE_ENTRIES
    ) {
      return
    }
    this.writesSinceEvict = 0

    const now = Date.now()
    for (const [key, entry] of this.cache) {
      if (now - entry.updatedAt > CACHE_TTL) this.cache.delete(key)
    }

    // Живых записей всё равно больше потолка (одна огромная подписка, свежий
    // прогон «Проверить все») — режем самые старые. Map хранит порядок
    // вставки, но замер обновляет значение НЕ меняя место ключа, поэтому
    // сортируем по времени замера, а не полагаемся на порядок.
    if (this.cache.size > MAX_CACHE_ENTRIES) {
      const byAge = [...this.cache.entries()].sort(
        ([, left], [, right]) => left.updatedAt - right.updatedAt,
      )
      for (const [key] of byAge.slice(0, this.cache.size - MAX_CACHE_ENTRIES)) {
        this.cache.delete(key)
      }
    }
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

  /**
   * The newest core measurement for this node: `{ delay, at }`, `at` in ms.
   *
   * clod: тест шёл по URL самой группы (`proxy-groups[].url` из шаблона
   * провайдера), и ядро складывает такие замеры не в `history`, а в
   * `extra[url]`. Без этого «Тест» показывал пинг до дефолтного адреса —
   * то есть не то, что реально происходит с YouTube-группой.
   *
   * A timestamp the core sent in a shape we cannot parse counts as "unknown"
   * (`0`) rather than dropping the measurement: the number is still real, it
   * just loses every freshness comparison to something we can date.
   */
  private newestCoreEntry(proxy: IProxyItem, group: string) {
    // A stale `extra` entry must not outrank a ping just taken against the
    // default URL, or the other way round. On an equal timestamp `extra` wins:
    // that is the address the group was actually measured with.
    let newest: { delay: number; at: number } | undefined
    for (const entry of [
      proxy.extra?.[this.getUrl(group)]?.history?.at(-1),
      proxy.history?.at(-1),
    ]) {
      if (!entry) continue
      const parsed = Date.parse(entry.time)
      const at = Number.isFinite(parsed) ? parsed : 0
      if (!newest || at > newest.at) newest = { delay: entry.delay, at }
    }
    return newest
  }

  /**
   * Our own cache entry, but only while it still means something.
   *
   * A stale "testing" marker is dropped here so it cannot outlive the test it
   * belongs to, and `-1` (never measured) carries no information at all.
   */
  private liveCacheEntry(proxy: IProxyItem, group: string) {
    const update = this.getDelayUpdate(proxy.name, group)
    if (!update) return undefined
    if (update.delay === -2) {
      const alive =
        Date.now() - update.updatedAt <= TESTING_TTL ||
        this.queuedChecks.has(hashKey(proxy.name, group))
      return alive ? update : undefined
    }
    return update.delay >= 0 ? update : undefined
  }

  /// Временный фикс сортировки задержки узлов у provider
  /// `skipTesting` — показать последний замер вместо метки «идёт проверка»:
  /// сортировка и строки Главной не должны прыгать, пока узел в очереди.
  getDelayFix(proxy: IProxyItem, group: string, skipTesting = false) {
    const live = this.liveCacheEntry(proxy, group)
    const cached = skipTesting && live?.delay === -2 ? live.before : live
    const core = this.newestCoreEntry(proxy, group)

    // Two independent sources of the same number — the newer measurement wins.
    // The cache used to be read first and unconditionally: after a config
    // reload the core already had a fresh ping while the screen kept showing a
    // half-hour-old figure (and a stuck `-2` meant a spinner that never ended).
    if (cached && (!core || cached.updatedAt >= core.at)) return cached.delay

    if (core) {
      // 0ms отображаем как error
      return core.delay || 1e6
    }
    return -1
  }

  isChecking(group: string) {
    return this.checkingGroups.has(group)
  }

  /**
   * Когда сняли тот замер, который видит пользователь: мс epoch, 0 — не знаем.
   *
   * Dates exactly the entry `getDelayFix` picked, not the freshest one that
   * exists. Taking the maximum of both sources made the age come from the
   * cache while the figure on screen came from the core's history: the
   * measurement looked fresh, the automatic re-ping never fired, and the user
   * stared at an hour-old ping.
   */
  getMeasuredAt(proxy: IProxyItem, group: string) {
    const live = this.liveCacheEntry(proxy, group)
    const cached = live?.delay === -2 ? live.before : live
    const core = this.newestCoreEntry(proxy, group)

    // `-2` is a state, not a measurement the user can see: its age says
    // nothing about how old the figure hiding behind the spinner is.
    if (cached && cached.delay >= 0 && (!core || cached.updatedAt >= core.at)) {
      return cached.updatedAt
    }

    return core?.at ?? 0
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

  /** `pad` — спиннер не короче 500 мс; пакету не нужен: там он стоит с начала очереди. */
  async checkDelay(
    name: string,
    group: string,
    timeout: number,
    providerName?: string,
    pad = true,
  ): Promise<DelayUpdate> {
    debugLog(
      `[DelayManager] Начало теста задержки, прокси: ${name}, группа: ${group}, тайм-аут: ${timeout}ms`,
    )

    // Сначала выставляем статус «тестируется»
    this.setDelay(name, group, -2)

    const startTime = Date.now()
    const profile = this.profile

    try {
      const url = this.getUrl(group)
      debugLog(
        `[DelayManager] Вызов API для теста задержки, прокси: ${name}, URL: ${url}`,
      )

      // Тайм-аут отдают ядро и плагин (delay = 0), свой таймер не нужен
      const result = await this.unifiedDelayCheck(
        name,
        url,
        timeout,
        providerName,
      )

      // Гарантируем показ анимации загрузки не менее 500мс
      const elapsedTime = Date.now() - startTime
      if (pad && elapsedTime < 500) {
        await new Promise((resolve) => setTimeout(resolve, 500 - elapsedTime))
      }

      const delay = result.delay
      const elapsed = elapsedTime
      debugLog(
        `[DelayManager] Тест задержки завершён, прокси: ${name}, результат: ${delay}ms`,
      )

      // Замер прежней подписки в кэш новой не пишем — ключ у одноимённых узлов общий
      if (profile !== this.profile) return { delay: -1, updatedAt: Date.now() }

      return this.setDelay(name, group, delay, { elapsed })
    } catch (error) {
      // Гарантируем показ анимации загрузки не менее 500мс
      if (pad) await new Promise((resolve) => setTimeout(resolve, 500))
      console.error(
        `[DelayManager] Ошибка теста задержки, прокси: ${name}`,
        error,
      )
      // Отказ вызова — это «ядро недоступно», а не приговор узлу: не мерили
      const delay = -1
      const elapsed = Date.now() - startTime

      if (profile !== this.profile) return { delay, updatedAt: Date.now() }

      return this.setDelay(name, group, delay, { elapsed })
    }
  }

  private async takeCheckSlot() {
    if (this.freeCheckSlots > 0) {
      this.freeCheckSlots -= 1
      return
    }
    await new Promise<void>((resolve) => this.checkSlotWaiters.push(resolve))
  }

  private releaseCheckSlot() {
    const next = this.checkSlotWaiters.shift()
    if (next) next()
    else this.freeCheckSlots += 1
  }

  async checkListDelay(
    proxies: IProxyItem[],
    group: string,
    timeout: number,
    concurrency = 10,
  ) {
    debugLog(
      `[DelayManager] Начало пакетного теста задержки, группа: ${group}, количество: ${proxies.length}, параллельность: ${concurrency}`,
    )
    const names = proxies.map((p) => p.name)
    // Выставляем статус «идёт тест задержки»
    names.forEach((name) => {
      this.setDelay(name, group, -2)
      this.queuedChecks.add(hashKey(name, group))
    })

    let index = 0
    const startTime = Date.now()
    const listener = this.groupListenerMap.get(group)
    const profile = this.profile

    const help = async (): Promise<void> => {
      // Подписка сменилась: у одноимённых узлов новой ключ тот же, и очередь
      // прежней ставила бы им метку «идёт проверка» без замера за ней
      if (this.profile !== profile) {
        for (const name of names.slice(index)) {
          this.queuedChecks.delete(hashKey(name, group))
        }
        return
      }
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

        await this.takeCheckSlot()
        try {
          await this.checkDelay(
            currName,
            group,
            timeout,
            currProviderName,
            false,
          )
        } finally {
          this.releaseCheckSlot()
        }
        if (listener) {
          this.queueGroupNotification(group)
        }
      } catch (error) {
        console.error(
          `[DelayManager] Ошибка теста отдельного прокси в пакете, прокси: ${currName}`,
          error,
        )
        // Как и отказ внутри checkDelay — «не мерили», а не приговор узлу
        this.setDelay(currName, group, -1)
      } finally {
        this.queuedChecks.delete(hashKey(currName, group))
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

    this.checkingGroups.set(group, (this.checkingGroups.get(group) ?? 0) + 1)
    try {
      await Promise.all(promiseList)
    } finally {
      const left = (this.checkingGroups.get(group) ?? 1) - 1
      if (left > 0) this.checkingGroups.set(group, left)
      else this.checkingGroups.delete(group)
    }
    const totalTime = Date.now() - startTime
    debugLog(
      `[DelayManager] Пакетный тест задержки завершён, группа: ${group}, общее время: ${totalTime}ms`,
    )
  }

  formatDelay(
    delay: number,
    timeout: number,
    labels: { timeout: string; error: string },
  ) {
    if (delay === -1) return '-'
    if (delay === -2) return 'testing'
    if (delay === 0 || (delay >= timeout && delay <= 1e5)) return labels.timeout
    if (delay > 1e5) return labels.error
    return `${delay}`
  }

  formatDelayColor(delay: number, timeout = 10000) {
    if (delay < 0) return ''
    if (delay >= timeout) return 'error.main'
    return delayColor(delay)
  }
}

export default new DelayManager()
