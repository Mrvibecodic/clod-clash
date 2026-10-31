/**
 * Что стоит в очереди пакетных проверок. Запись принадлежит пакету, который её
 * поставил: закончив, пакет снимает только свою, а не поставленную позже другим.
 * Узел, до которого пакет ещё не дошёл, можно забрать — тогда пакет его пропустит.
 */
export class BatchQueue {
  private owners = new Map<string, number>()
  private measuring = new Map<string, number>()
  private batches = 0

  enqueue(keys: readonly string[]) {
    const batch = ++this.batches
    for (const key of keys) this.owners.set(key, batch)
    return batch
  }

  /** Узел ещё ждёт пакета: его не забрали и не померил другой пакет. */
  isWaiting(key: string) {
    return this.owners.has(key)
  }

  /** Пакет начинает замер узла. `false` — узел забрали, мерить не нужно. */
  start(key: string) {
    if (!this.owners.has(key)) return false
    this.measuring.set(key, (this.measuring.get(key) ?? 0) + 1)
    return true
  }

  /** Замер, начатый `start`, закончен. */
  finish(key: string, batch: number) {
    const left = (this.measuring.get(key) ?? 1) - 1
    if (left > 0) this.measuring.set(key, left)
    else this.measuring.delete(key)
    this.dequeue(key, batch)
  }

  dequeue(key: string, batch: number) {
    if (this.owners.get(key) === batch) this.owners.delete(key)
  }

  /** Узел ждёт пакета или пакет его уже меряет. */
  has(key: string) {
    return this.owners.has(key) || this.measuring.has(key)
  }

  isMeasuring(key: string) {
    return this.measuring.has(key)
  }

  /** Забрать узел, до которого пакет ещё не дошёл. `false` — его уже меряют или его нет в очереди. */
  takeOver(key: string) {
    if (this.measuring.has(key)) return false
    return this.owners.delete(key)
  }

  clear() {
    this.owners.clear()
    this.measuring.clear()
  }
}
