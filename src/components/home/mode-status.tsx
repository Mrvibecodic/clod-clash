import AltRouteRoundedIcon from '@mui/icons-material/AltRouteRounded'
import PublicRoundedIcon from '@mui/icons-material/PublicRounded'
import { Box, ButtonBase, Tooltip } from '@mui/material'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { useRuntimeConfig } from '@/hooks/use-clash'
import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useClashConfigData } from '@/providers/app-data-context'

interface Props {
  locked?: boolean
  permanent?: boolean
  showTargets?: boolean
}

const PILL_SX = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 0.75,
  px: 1.5,
  py: 0.625,
  borderRadius: 999,
  bgcolor: 'background.paper',
  border: '1px solid var(--card-line)',
  boxShadow: 'var(--card-shadow)',
  color: 'text.secondary',
  fontSize: 12.5,
  fontWeight: 600,
  whiteSpace: 'nowrap',
  '& svg': { fontSize: 14, color: 'primary.main' },
} as const

export const ModeStatus = ({
  locked,
  permanent,
  showTargets = true,
}: Props) => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { clashConfig } = useClashConfigData()
  const { targetSys, targetTun } = useConnectTargets()

  const { data: runtime } = useRuntimeConfig(!clashConfig)

  const clashMode = (clashConfig ?? runtime)?.mode?.toLowerCase()
  const modeLabel =
    clashMode === 'global' || clashMode === 'direct'
      ? t(`home.components.clashMode.labels.${clashMode}`)
      : t('home.components.clashMode.labels.rule')

  const activeTargets = [
    targetSys ? t('settings.components.verge.basic.options.sysproxy') : null,
    targetTun ? t('settings.components.verge.basic.options.tun') : null,
  ]
    .filter(Boolean)
    .join(' + ')

  const content = (
    <>
      {showTargets && activeTargets ? (
        <>
          <PublicRoundedIcon />
          <span>{activeTargets}</span>
          <Box component="span" sx={{ opacity: 0.4 }}>
            ·
          </Box>
        </>
      ) : null}
      <AltRouteRoundedIcon />
      <span>{modeLabel}</span>
    </>
  )

  if (locked) {
    return (
      <Tooltip
        title={
          permanent
            ? t('home.components.modeStatus.lockedHintPermanent')
            : t('home.components.modeStatus.lockedHint')
        }
      >
        <Box sx={{ ...PILL_SX, cursor: 'help' }}>{content}</Box>
      </Tooltip>
    )
  }

  return (
    <ButtonBase
      sx={{
        ...PILL_SX,
        transition: (theme) =>
          theme.transitions.create(['border-color', 'background-color'], {
            duration: theme.transitions.duration.short,
          }),
        '&:hover': { borderColor: 'primary.main' },
      }}
      onClick={() => void navigate('/settings')}
    >
      {content}
    </ButtonBase>
  )
}
