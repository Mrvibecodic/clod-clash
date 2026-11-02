import {
  OpenInNewRounded,
  SettingsBackupRestoreRounded,
} from '@mui/icons-material'
import { Box, Typography } from '@mui/material'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'

import type { DialogRef } from '@/components/base'
import { useProfiles } from '@/hooks/use-profiles'
import { openProviderLink, useProviderLinks } from '@/hooks/use-provider-links'
import { CARD_SURFACE } from '@/pages/_theme'
import { profileDisplayName } from '@/utils/profile-name'

import { ProviderValuesViewer } from './mods/provider-values-viewer'
import { SettingItem, SettingList } from './mods/setting-comp'

/**
 * clod:provider-links — те же ссылки провайдера, но пунктами настроек.
 *
 * На главной они кнопками, и этого хватает ровно до того момента, пока человек
 * не пошёл искать их там, где лежит всё остальное. Дублируются они не «до
 * кучи»: в настройки заходят с вопросом «где посмотреть», и ответ должен
 * находиться здесь же.
 *
 * Группа принадлежит ТЕКУЩЕЙ подписке и названа её именем: у разных
 * провайдеров свои кабинеты и свои боты, и перепутать их нельзя. Ссылки
 * соседних профилей сюда не попадают — список строится из активного, а не из
 * всех сразу. У подписки по адресу группа есть и без ссылок — в ней пункт
 * «Вернуть значения провайдера»; у локального файла без ссылок группы нет.
 */
export const SettingProviderLinks = () => {
  const { t } = useTranslation()
  const { current } = useProfiles()
  const links = useProviderLinks(current)
  const remote = current?.type === 'remote'
  const viewerRef = useRef<DialogRef>(null)

  if (!links.length && !remote) return null

  return (
    <Box sx={CARD_SURFACE}>
      <SettingList
        title={profileDisplayName(current) || t('shared.providerLinks.title')}
      >
        {links.length ? (
          <Box sx={{ px: 2, mt: -1, mb: 0.5 }}>
            <Typography variant="caption" color="text.secondary">
              {t('shared.providerLinks.subtitle')}
            </Typography>
          </Box>
        ) : null}

        {links.map((link) => (
          <SettingItem
            key={link.key}
            label={
              <Box
                component="span"
                sx={{ display: 'flex', alignItems: 'center', gap: 1 }}
              >
                <Box
                  component="span"
                  sx={{ display: 'flex', color: 'primary.main' }}
                >
                  {link.icon}
                </Box>
                {link.label}
              </Box>
            }
            extra={
              <OpenInNewRounded
                sx={{ fontSize: 15, ml: 1, color: 'text.secondary' }}
              />
            }
            onClick={() => openProviderLink(link.url)}
          />
        ))}

        {remote ? (
          <SettingItem
            label={
              <Box
                component="span"
                sx={{ display: 'flex', alignItems: 'center', gap: 1 }}
              >
                <Box
                  component="span"
                  sx={{ display: 'flex', color: 'primary.main' }}
                >
                  <SettingsBackupRestoreRounded fontSize="small" />
                </Box>
                {t('shared.providerValues.title')}
              </Box>
            }
            onClick={() => viewerRef.current?.open()}
          />
        ) : null}
      </SettingList>
      <ProviderValuesViewer ref={viewerRef} />
    </Box>
  )
}
