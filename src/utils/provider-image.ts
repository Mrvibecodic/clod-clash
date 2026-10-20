export type ProviderImageView = { src: string } | 'pending' | undefined

/**
 * Ответ приложения о копии картинки: готова, ещё нет или не будет вовсе
 * (адрес не публичный https, не картинка, сверх потолка) — до смены адреса.
 */
export type ProviderPicture = { ready: string } | 'pending' | 'never'

/**
 * Ссылку на картинку провайдера окно само не открывает: её скачивает
 * приложение (срок, потолок размера, только публичный https), и до готовой
 * копии на месте картинки пусто. Копии не будет — нет и места под неё.
 */
export const providerImageView = (
  image: string | undefined,
  cached: ProviderPicture | undefined,
): ProviderImageView => {
  const value = image?.trim() ?? ''
  if (value.startsWith('http')) {
    if (cached === 'never') return undefined
    return typeof cached === 'object' ? { src: cached.ready } : 'pending'
  }
  if (value.startsWith('data')) return { src: value }
  if (value.startsWith('<svg')) {
    return {
      src: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(value)}`,
    }
  }
  return undefined
}

/** Спрашивать копию снова стоит, только пока она «ещё качается». */
export const awaitsCopy = (cached: ProviderPicture | undefined) =>
  cached === undefined || cached === 'pending'
