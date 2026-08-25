import {
  HomeWorkRounded,
  MenuBookRounded,
  MonitorHeartRounded,
  SmartToyRounded,
  SupportAgentRounded,
} from '@mui/icons-material'
import type { ReactNode } from 'react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import type { TranslationKey } from '@/types/generated/i18n-keys'

export interface ProviderLink {
  key: 'portal' | 'support' | 'bot' | 'monitor' | 'guide'
  url: string
  label: string
  icon: ReactNode
}

const DEFS = [
  {
    key: 'portal',
    field: 'portal_url',
    label: 'shared.providerLinks.portal',
    icon: <HomeWorkRounded fontSize="small" />,
  },
  {
    key: 'support',
    field: 'support_url',
    label: 'shared.providerLinks.support',
    icon: <SupportAgentRounded fontSize="small" />,
  },
  {
    key: 'bot',
    field: 'bot_url',
    label: 'shared.providerLinks.bot',
    icon: <SmartToyRounded fontSize="small" />,
  },
  {
    key: 'monitor',
    field: 'monitor_url',
    label: 'shared.providerLinks.monitor',
    icon: <MonitorHeartRounded fontSize="small" />,
  },
  {
    key: 'guide',
    field: 'guide_url',
    label: 'shared.providerLinks.guide',
    icon: <MenuBookRounded fontSize="small" />,
  },
] as const satisfies readonly {
  key: ProviderLink['key']
  field: keyof IProfileItem
  label: TranslationKey
  icon: ReactNode
}[]

export const useProviderLinks = (profile?: IProfileItem | null) => {
  const { t } = useTranslation()

  return useMemo<ProviderLink[]>(() => {
    if (!profile) return []
    return DEFS.reduce<ProviderLink[]>((acc, def) => {
      const url = profile[def.field]
      if (typeof url === 'string' && url) {
        acc.push({ key: def.key, url, label: t(def.label), icon: def.icon })
      }
      return acc
    }, [])
  }, [profile, t])
}

const ALLOWED_LINK_SCHEMES = ['https:', 'tg:', 'mailto:']

export const openProviderLink = async (url: string) => {
  let scheme = ''
  try {
    scheme = new URL(url).protocol
  } catch {}
  if (!ALLOWED_LINK_SCHEMES.includes(scheme)) {
    console.error('[provider-links] scheme is not allowed:', url)
    showNotice.error('shared.providerLinks.openError')
    return
  }
  try {
    await openWebUrl(url)
  } catch (error) {
    console.error('[provider-links] failed to open:', error)
    showNotice.error(error)
  }
}
