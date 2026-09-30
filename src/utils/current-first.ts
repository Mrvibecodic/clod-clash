// Узел, которым группа пользуется сейчас, меряется первым: пакетный тест идёт
// по десять узлов за раз, и в длинном списке он иначе ждал бы до конца прогона
export function currentFirst<T extends { name: string }>(
  proxies: T[],
  current?: string,
): T[] {
  const index = current ? proxies.findIndex((p) => p.name === current) : -1
  if (index <= 0) return proxies
  return [
    proxies[index],
    ...proxies.slice(0, index),
    ...proxies.slice(index + 1),
  ]
}
