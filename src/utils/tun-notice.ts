import type { TranslationKey } from '@/types/generated/i18n-keys'

const SETUP_MARKERS: Record<string, TranslationKey> = {
  'tun::setup_pending':
    'settings.sections.system.notifications.tunMode.setupPending',
  'tun::setup_busy': 'settings.sections.system.notifications.tunMode.setupBusy',
}

const FAILURE_REASONS: Record<string, TranslationKey> = {
  startFailed: 'home.components.tunStatus.reasons.startFailed',
  adapterBusy: 'home.components.tunStatus.reasons.adapterBusy',
  noRights: 'home.components.tunStatus.reasons.noRights',
  noTraffic: 'home.components.tunStatus.reasons.noTraffic',
  setupFailed: 'home.components.tunStatus.reasons.setupFailed',
  rightsDeclined: 'home.components.tunStatus.reasons.rightsDeclined',
  serviceSilent: 'home.components.tunStatus.reasons.serviceSilent',
}

const markerOf = (error: unknown): string | undefined => {
  if (typeof error === 'string') return error.trim()
  if (error instanceof Error) return error.message.trim()
  if (typeof error === 'object' && error !== null) {
    const { message } = error as { message?: unknown }
    if (typeof message === 'string') return message.trim()
  }
  return undefined
}

export const tunSetupNotice = (error: unknown): unknown => {
  const marker = markerOf(error)
  return (marker && SETUP_MARKERS[marker]) || error
}

export const tunFailureKey = (
  failure?: string | null,
): TranslationKey | undefined =>
  failure ? FAILURE_REASONS[failure] : undefined
