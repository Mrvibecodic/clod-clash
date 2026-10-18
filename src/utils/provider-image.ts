export type ProviderImageView = { src: string } | 'pending' | undefined

/**
 * Ссылку на картинку провайдера окно само не открывает: её скачивает
 * приложение (срок, потолок размера, только публичный https), и до готовой
 * копии на месте картинки пусто.
 */
export const providerImageView = (
  image: string | undefined,
  cached: string,
): ProviderImageView => {
  const value = image?.trim() ?? ''
  if (value.startsWith('http')) return cached ? { src: cached } : 'pending'
  if (value.startsWith('data')) return { src: value }
  if (value.startsWith('<svg')) {
    return {
      src: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(value)}`,
    }
  }
  return undefined
}
