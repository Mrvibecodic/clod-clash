/**
 * clod:chan — отпечаток ключа прослойки в том виде, в каком его показывает
 * админка провайдера: первые 6 знаков base64url от SHA-256 ключа. По нему
 * человек сверяет, что подписке отвечает та самая прослойка.
 *
 * `pin` — ключ, как он записан в подписке (base64url без выравнивания).
 * Ключ не разобрался — `undefined`.
 */
export async function chanFingerprint(
  pin: string,
): Promise<string | undefined> {
  const base64 = pin.replace(/-/g, '+').replace(/_/g, '/')
  let key: Uint8Array<ArrayBuffer>
  try {
    key = Uint8Array.from(
      atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, '=')),
      (char) => char.charCodeAt(0),
    )
  } catch {
    return undefined
  }
  if (key.length !== 32) return undefined

  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', key))
  return btoa(String.fromCharCode(...digest))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .slice(0, 6)
}
