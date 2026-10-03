import AcUnitRoundedIcon from '@mui/icons-material/AcUnitRounded'
import BlockRoundedIcon from '@mui/icons-material/BlockRounded'
import { alpha, Box, Link, Tooltip, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { useProfiles } from '@/hooks/use-profiles'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

interface Props {
  mark?: FreezeMark
  /** Узкие места (мини-плитки): только значок, слово — в подсказке. */
  compact?: boolean
}

const ICONS = {
  frozen: AcUnitRoundedIcon,
  dead: BlockRoundedIcon,
} as const

const COLORS = { frozen: 'warning', dead: 'error' } as const

/**
 * clod:freeze — пометка «режется» / «не отвечает» рядом с пингом.
 *
 * Форма + слово, цвет только помогает. Подсказка объясняет пометку простыми
 * словами и ведёт в поддержку подписки, если панель прислала адрес. Сама
 * пометка ничего не делает с узлом: пинг рисуется как раньше, выбор — тоже.
 */
export const FreezeMark = ({ mark, compact }: Props) => {
  const { t } = useTranslation()
  const { current } = useProfiles()

  if (!mark) return null

  const Icon = ICONS[mark]
  const color = COLORS[mark]
  const supportUrl = current?.support_url

  const title = (
    <Box sx={{ maxWidth: 320 }}>
      <Typography
        variant="subtitle2"
        sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mb: 0.5 }}
      >
        <Icon sx={{ fontSize: 16 }} />
        {t(`freeze.title.${mark}`)}
      </Typography>
      <Typography variant="body2" sx={{ mb: 1 }}>
        {t(`freeze.explain.${mark}`)}
      </Typography>
      <Box
        sx={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: 1,
          borderTop: '1px solid',
          borderColor: 'divider',
          pt: 0.75,
        }}
      >
        <Typography variant="caption" color="text.secondary">
          {t('freeze.checkedHere')}
        </Typography>
        {supportUrl ? (
          <Link
            component="button"
            type="button"
            variant="caption"
            underline="hover"
            sx={{ fontWeight: 600 }}
            onClick={(event) => {
              event.preventDefault()
              event.stopPropagation()
              openWebUrl(supportUrl).catch((error) => showNotice.error(error))
            }}
          >
            {t('freeze.support')}
          </Link>
        ) : null}
      </Box>
    </Box>
  )

  return (
    <Tooltip
      title={title}
      placement="top"
      arrow
      enterDelay={300}
      slotProps={{
        tooltip: {
          sx: {
            bgcolor: 'background.paper',
            color: 'text.primary',
            boxShadow: 'var(--card-shadow-hover)',
            border: '1px solid',
            borderColor: 'divider',
            p: 1.5,
          },
        },
        arrow: { sx: { color: 'background.paper' } },
      }}
    >
      <Box
        component="span"
        aria-label={t(`freeze.title.${mark}`)}
        sx={({ palette }) => ({
          display: 'inline-flex',
          alignItems: 'center',
          gap: '4px',
          flex: 'none',
          borderRadius: 999,
          px: compact ? '5px' : '8px',
          py: '2px',
          fontSize: 12,
          fontWeight: 600,
          lineHeight: 1.3,
          whiteSpace: 'nowrap',
          color: palette[color].main,
          bgcolor: alpha(palette[color].main, 0.12),
          border: `1px solid ${alpha(palette[color].main, 0.35)}`,
          cursor: 'default',
        })}
      >
        <Icon sx={{ fontSize: 14 }} />
        {compact ? null : t(`freeze.mark.${mark}`)}
      </Box>
    </Tooltip>
  )
}
