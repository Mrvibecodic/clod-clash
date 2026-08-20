import { alpha, Box, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { openProviderLink, useProviderLinks } from '@/hooks/use-provider-links'
import { CARD_SURFACE, CARD_TITLE, SHAPE, TINT } from '@/pages/_theme'

interface Props {
  profile?: IProfileItem | null
  compact?: boolean
}

export const ProviderLinksCard = ({ profile, compact }: Props) => {
  const { t } = useTranslation()
  const links = useProviderLinks(profile)

  if (!links.length) return null

  return (
    <Box
      sx={{
        ...CARD_SURFACE,
        px: 1.5,
        py: compact ? 0.75 : 1,
      }}
    >
      <Typography
        variant="caption"
        color="text.secondary"
        noWrap
        sx={{ ...CARD_TITLE, display: 'block', mb: 0.5 }}
      >
        {profile?.name || t('shared.providerLinks.title')}
      </Typography>
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
              borderRadius: SHAPE.control,
              cursor: 'pointer',
              transition: (theme) =>
                theme.transitions.create(['background-color'], {
                  duration: theme.transitions.duration.shortest,
                }),
              '&:hover': {
                bgcolor: (theme) =>
                  alpha(theme.palette.primary.main, TINT.weak),
              },
            }}
          >
            <Box
              sx={{
                width: 30,
                height: 30,
                borderRadius: SHAPE.control,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: 'primary.main',
                bgcolor: (theme) =>
                  alpha(theme.palette.primary.main, TINT.base),
              }}
            >
              {link.icon}
            </Box>
            <Typography
              variant="caption"
              noWrap
              sx={{ maxWidth: '100%', fontSize: 12 }}
            >
              {link.label}
            </Typography>
          </Box>
        ))}
      </Box>
    </Box>
  )
}
