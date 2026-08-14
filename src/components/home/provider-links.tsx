import { alpha, Box, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { openProviderLink, useProviderLinks } from '@/hooks/use-provider-links'

interface Props {
  profile?: IProfileItem | null
  /** Плотная вёрстка главной, когда окно ужимается по высоте. */
  compact?: boolean
}

export const ProviderLinksCard = ({ profile, compact }: Props) => {
  const { t } = useTranslation()
  const links = useProviderLinks(profile)

  if (!links.length) return null

  return (
    <Box
      sx={{
        border: (theme) => `1px solid ${theme.palette.divider}`,
        borderRadius: '12px',
        px: 1.5,
        py: compact ? 0.75 : 1,
      }}
    >
      <Typography
        variant="caption"
        color="text.secondary"
        noWrap
        sx={{ display: 'block', mb: 0.5 }}
      >
        {profile?.name || t('shared.providerLinks.title')}
      </Typography>
      {/* Ссылки делят ширину карточки поровну: две штуки, прижатые к левому
          краю, читаются как обрезанный список, а не как «их всего две». */}
      <Box sx={{ display: 'flex', gap: 0.5 }}>
        {links.map((link) => (
          <Box
            key={link.key}
            role="button"
            tabIndex={0}
            title={link.label}
            onClick={() => void openProviderLink(link.url)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault()
                void openProviderLink(link.url)
              }
            }}
            sx={{
              flex: 1,
              minWidth: 0,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              gap: 0.5,
              py: compact ? 0.5 : 0.75,
              borderRadius: '10px',
              cursor: 'pointer',
              transition: 'background-color 0.15s',
              '&:hover': {
                bgcolor: (theme) => alpha(theme.palette.primary.main, 0.08),
              },
            }}
          >
            <Box
              sx={{
                width: 30,
                height: 30,
                borderRadius: '9px',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: 'primary.main',
                bgcolor: (theme) => alpha(theme.palette.primary.main, 0.12),
              }}
            >
              {link.icon}
            </Box>
            <Typography
              variant="caption"
              noWrap
              sx={{ maxWidth: '100%', fontSize: 11.5 }}
            >
              {link.label}
            </Typography>
          </Box>
        ))}
      </Box>
    </Box>
  )
}
