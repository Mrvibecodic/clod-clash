import delayManager from '@/services/delay'
import { failedDelay, usableDelay } from '@/utils/delay-color'
import { compileStringMatcher } from '@/utils/search-matcher'

// default | delay | alphabet
export type ProxySortType = 0 | 1 | 2

export type ProxySearchState = {
  matchCase?: boolean
  matchWholeWord?: boolean
  useRegularExpression?: boolean
}

export function filterSort(
  proxies: IProxyItem[],
  groupName: string,
  filterText: string,
  sortType: ProxySortType,
  searchState?: ProxySearchState,
) {
  const fp = filterProxies(proxies, groupName, filterText, searchState)
  const sp = sortProxies(fp, groupName, sortType)
  return sp
}

/**
 * Можно фильтровать по значению задержки / типу узла
 */
const regex1 = /delay([=<>])(\d+|timeout|error)/i
const regex2 = /type=(.*)/i

/**
 * filter the proxy
 * according to the regular conditions
 */
function filterProxies(
  proxies: IProxyItem[],
  groupName: string,
  filterText: string,
  searchState?: ProxySearchState,
) {
  const query = filterText.trim()
  if (!query) return proxies

  const res1 = regex1.exec(query)
  if (res1) {
    const symbol = res1[1]
    const symbol2 = res1[2].toLowerCase()
    // «timeout» и «error» — одно и то же: узел не ответил
    const failed = symbol2 === 'timeout' || symbol2 === 'error'
    const value = +symbol2

    return proxies.filter((p) => {
      // Как и в сортировке: узел в очереди проверки не выпадает из фильтра
      const delay = delayManager.getDelayFix(p, groupName, true)

      if (failed) return symbol === '=' && failedDelay(delay)
      if (!usableDelay(delay)) return false
      if (symbol === '=') return delay === value
      if (symbol === '<') return delay <= value
      return delay >= value
    })
  }

  const res2 = regex2.exec(query)
  if (res2) {
    const type = res2[1].toLowerCase()
    return proxies.filter((p) => p.type.toLowerCase().includes(type))
  }

  const {
    matchCase = false,
    matchWholeWord = false,
    useRegularExpression = false,
  } = searchState ?? {}
  const compiled = compileStringMatcher(query, {
    matchCase,
    matchWholeWord,
    useRegularExpression,
  })

  if (!compiled.isValid) return []
  return proxies.filter((p) => compiled.matcher(p.name))
}

/**
 * sort the proxy
 */
function sortProxies(
  proxies: IProxyItem[],
  groupName: string,
  sortType: ProxySortType,
) {
  if (!proxies) return []
  if (sortType === 0) return proxies

  if (sortType === 1) {
    // Замеры по возрастанию, за ними не ответившие, в конце не мерянные
    const categorizeDelay = (delay: number): [number, number] =>
      usableDelay(delay) ? [0, delay] : failedDelay(delay) ? [1, 0] : [2, 0]

    const ranked = proxies.map((proxy) => ({
      proxy,
      // Метка «идёт проверка» не двигает строку: иначе очередь уезжала вниз на каждом опросе
      rank: categorizeDelay(delayManager.getDelayFix(proxy, groupName, true)),
    }))

    ranked.sort((a, b) => a.rank[0] - b.rank[0] || a.rank[1] - b.rank[1])

    return ranked.map((entry) => entry.proxy)
  }

  return proxies.slice().sort((a, b) => a.name.localeCompare(b.name))
}
