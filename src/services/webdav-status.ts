export type WebdavStatus = 'unknown' | 'ready' | 'failed'

interface WebdavStatusCache {
  signature: string
  status: WebdavStatus
}

const WEBDAV_STATUS_KEY = 'webdav_status'

// Прежняя запись хранила в подписи сам пароль открытым текстом.
const LEGACY_WEBDAV_STATUS_KEY = 'webdav_status_cache'

// Пароля в подписи нет: она лежит на диске в профиле окна. Новый пароль
// приходит только через сохранение в диалоге, а оно сбрасывает статус.
export const buildWebdavSignature = (
  verge?: Pick<IVergeConfig, 'webdav_url' | 'webdav_username'> | null,
) => {
  const url = verge?.webdav_url?.trim() ?? ''
  const username = verge?.webdav_username?.trim() ?? ''

  if (!url && !username) return ''

  return JSON.stringify([url, username])
}

const canUseStorage = () => typeof localStorage !== 'undefined'

if (canUseStorage()) localStorage.removeItem(LEGACY_WEBDAV_STATUS_KEY)

export const getWebdavStatus = (signature: string): WebdavStatus => {
  if (!signature || !canUseStorage()) return 'unknown'

  const raw = localStorage.getItem(WEBDAV_STATUS_KEY)
  if (!raw) return 'unknown'

  try {
    const data = JSON.parse(raw) as Partial<WebdavStatusCache>
    if (!data || data.signature !== signature) return 'unknown'
    return data.status === 'ready' || data.status === 'failed'
      ? data.status
      : 'unknown'
  } catch {
    return 'unknown'
  }
}

export const setWebdavStatus = (signature: string, status: WebdavStatus) => {
  if (!signature || !canUseStorage()) return

  const payload: WebdavStatusCache = {
    signature,
    status,
  }

  localStorage.setItem(WEBDAV_STATUS_KEY, JSON.stringify(payload))
}
