import { OpenInNewRounded } from '@mui/icons-material'
import { Box, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { useProfiles } from '@/hooks/use-profiles'
import { openProviderLink, useProviderLinks } from '@/hooks/use-provider-links'

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
 * всех сразу. Нет ссылок — нет и группы: пустая карточка ничего не сообщает.
 */
export const SettingProviderLinks = () => {
  const { t } = useTranslation()
  const { current } = useProfiles()
  const links = useProviderLinks(current)

  if (!links.length) return null

  return (
    <SettingList title={current?.name || t('shared.providerLinks.title')}>
      <Box sx={{ px: 2, mt: -1, mb: 0.5 }}>
        <Typography variant="caption" color="text.secondary">
          {t('shared.providerLinks.subtitle')}
        </Typography>
      </Box>

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
    </SettingList>
  )
}
