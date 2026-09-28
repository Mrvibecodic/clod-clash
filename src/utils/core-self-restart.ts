// Ядро после обновления (/upgrade) перезапускает себя само. На Windows exec
// нет, и mihomo запускал новое ядро дочерним процессом, а сам выходил; служба
// видела выход и поднимала с того же файла второе ядро. Копия держала порт до
// перезагрузки компьютера. Clod Core начиная с core-v1.19.31-clod.8 только
// выходит, и поднимает его служба; стоковый Mihomo и прежние сборки Clod Core
// — нет.
const FIRST_CLEAN_CLOD_CORE = [1, 19, 31, 8]

/** Перезапустится ли ядро этой версии после обновления без второй копии. */
export function coreRestartsCleanly(version?: string): boolean {
  const found = /^v?(\d+)\.(\d+)\.(\d+)-clod\.(\d+)$/.exec(
    version?.trim() ?? '',
  )
  if (!found) return false
  const parts = found.slice(1).map(Number)
  for (const [index, part] of parts.entries()) {
    const first = FIRST_CLEAN_CLOD_CORE[index]
    if (part !== first) return part > first
  }
  return true
}
