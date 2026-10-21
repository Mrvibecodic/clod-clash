/**
 * clod:connect-mode — что поднимает кнопка «Подключить» и что из этого запер
 * провайдер. Зеркало `connect_targets`/`allowed_targets` в бэкенде: таблица
 * примеров `connect-targets-cases.json` общая, и расхождение краснит обе стороны.
 */
export interface ConnectTargetsInput {
  connectSystemProxy?: boolean | null
  connectTunMode?: boolean | null
  connectMode?: string | null
  lockMode?: boolean | null
}

export interface ConnectTargets {
  targetSys: boolean
  targetTun: boolean
  /** Способ подключения назвал провайдер, и цель вне него не включается. */
  targetsLocked: boolean
}

/** Закрытый список: заголовок с чем-то другим бэкенд до профиля не доносит. */
const providerTargets = (
  mode?: string | null,
): { sys: boolean; tun: boolean } | undefined => {
  switch (mode) {
    case 'tun':
      return { sys: false, tun: true }
    case 'proxy':
      return { sys: true, tun: false }
    case 'both':
      return { sys: true, tun: true }
    default:
      return undefined
  }
}

/**
 * Три уровня, как и у режима интерфейса: выбор пользователя → пожелание
 * панели → умолчание приложения. При замке способа побеждает панель — иначе
 * заголовок «только TUN» не сработал бы ни у кого, кто однажды трогал тумблеры.
 * Обе цели выключены — системный прокси возвращается сам.
 */
export const connectTargets = ({
  connectSystemProxy,
  connectTunMode,
  connectMode,
  lockMode,
}: ConnectTargetsInput): ConnectTargets => {
  const provider = providerTargets(connectMode)
  const locked = lockMode ? provider : undefined
  const sys = locked?.sys ?? connectSystemProxy ?? provider?.sys ?? true
  const tun = locked?.tun ?? connectTunMode ?? provider?.tun ?? false
  return {
    targetSys: sys || !tun,
    targetTun: tun,
    targetsLocked: locked !== undefined,
  }
}
