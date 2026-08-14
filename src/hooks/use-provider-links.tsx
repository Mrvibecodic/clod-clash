import {
  HomeWorkRounded,
  MenuBookRounded,
  MonitorHeartRounded,
  SmartToyRounded,
  SupportAgentRounded,
} from '@mui/icons-material'
import { open } from '@tauri-apps/plugin-shell'
import type { ReactNode } from 'react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import type { TranslationKey } from '@/types/generated/i18n-keys'

/**
 * clod:provider-links — ссылки провайдера живут одной строкой.
 *
 * Их пять — кабинет, поддержка, бот, мониторинг, инструкция, — и все они
 * приходят заголовками подписки. Сделать из каждой плитку значило бы получить
 * восемь одинаковых прямоугольников, в которых ссылки провайдера уже не
 * отличить от кнопок самого приложения. Поэтому у провайдера своя карточка,
 * подписанная его именем, а плитками остаются действия приложения.
 *
 * Ссылок может не быть вовсе — тогда нет и карточки: пустая рамка на главной
 * выглядит поломкой.
 */
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

/** Ссылки текущей подписки — ровно те, что прислал провайдер, в одном порядке. */
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

export const openProviderLink = async (url: string) => {
  try {
    await open(url)
  } catch (error) {
    console.error('[provider-links] failed to open:', error)
  }
}
