/**
 * Что стоит в очереди пакетных проверок. Запись принадлежит пакету, который её
 * поставил: закончив, пакет снимает только свою, а не поставленную позже другим.
 */
export class BatchQueue {
  private owners = new Map<string, number>()
  private batches = 0

  enqueue(keys: readonly string[]) {
    const batch = ++this.batches
    for (const key of keys) this.owners.set(key, batch)
    return batch
  }

  dequeue(key: string, batch: number) {
    if (this.owners.get(key) === batch) this.owners.delete(key)
  }

  has(key: string) {
    return this.owners.has(key)
  }

  clear() {
    this.owners.clear()
  }
}
