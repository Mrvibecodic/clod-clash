import AcUnitRoundedIcon from '@mui/icons-material/AcUnitRounded'
import { alpha, Box, Link, Popover, Typography } from '@mui/material'
import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useProfiles } from '@/hooks/use-profiles'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

interface Props {
  frozen?: boolean
}

/**
 * clod:freeze — значок-снежинка рядом с пингом: одна иконка, чтобы не
 * занимать место. Нажатие открывает объяснение и ведёт в поддержку подписки,
 * если панель прислала адрес. Значок стоит внутри нажимаемых строк, поэтому
 * его нажатия (и нажатия в объяснении — оно в портале, но события идут по
 * дереву React) до строки не доходят: иначе вместе с объяснением выбирался бы
 * узел. Сам значок ничего не делает с узлом: пинг и выбор как раньше.
 */
export const FreezeMark = ({ frozen }: Props) => (frozen ? <Snowflake /> : null)

/** Значок с объяснением: живёт, пока есть пометка, — открытое объяснение
 * пропавшего значка с ним и закрывается, а не всплывает при его возврате. */
const Snowflake = () => {
  const { t } = useTranslation()
  const { current } = useProfiles()
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const titleId = useId()

  const supportUrl = current?.support_url
  const keep = (event: React.SyntheticEvent) => event.stopPropagation()

  return (
    <Box
      component="span"
      onClick={keep}
      onMouseDown={keep}
      sx={{ display: 'inline-flex', flex: 'none' }}
    >
      <Box
        component="button"
        type="button"
        aria-label={t('freeze.title')}
        aria-haspopup="dialog"
        aria-expanded={Boolean(anchor)}
        onClick={(event: React.MouseEvent<HTMLElement>) =>
          setAnchor(event.currentTarget)
        }
        sx={({ palette }) => ({
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          p: '3px',
          m: 0,
          border: `1px solid ${alpha(palette.warning.main, 0.35)}`,
          borderRadius: 999,
          color: palette.warning.main,
          bgcolor: alpha(palette.warning.main, 0.12),
          cursor: 'pointer',
          font: 'inherit',
        })}
      >
        <AcUnitRoundedIcon sx={{ fontSize: 14 }} />
      </Box>
      <Popover
        open={Boolean(anchor)}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'center' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'center' }}
        slotProps={{
          paper: {
            role: 'dialog',
            'aria-labelledby': titleId,
            sx: {
              maxWidth: 340,
              p: 1.5,
              mt: -0.75,
              border: '1px solid',
              borderColor: 'divider',
              boxShadow: 'var(--card-shadow-hover)',
            },
          },
        }}
      >
        <Typography
          id={titleId}
          variant="subtitle2"
          sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mb: 0.5 }}
        >
          <AcUnitRoundedIcon sx={{ fontSize: 16, color: 'warning.main' }} />
          {t('freeze.title')}
        </Typography>
        <Typography variant="body2" sx={{ mb: 1 }}>
          {t('freeze.explain')}
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
                openWebUrl(supportUrl).catch((error) => showNotice.error(error))
              }}
            >
              {t('freeze.support')}
            </Link>
          ) : null}
        </Box>
      </Popover>
    </Box>
  )
}
