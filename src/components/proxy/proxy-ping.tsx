import { CheckCircleOutlineRounded } from '@mui/icons-material'
import { alpha, Box, styled } from '@mui/material'
import type { MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseLoading } from '@/components/base'
import {
  delayColor,
  delayText,
  failedDelay,
  usableDelay,
} from '@/utils/delay-color'

import { PingUnit } from './proxy-ping-unit'
import { PING_SX } from './proxy-tile'

const Widget = styled(Box, {
  shouldForwardProp: (prop) => prop !== 'dense',
})<{ dense?: boolean }>(({ theme: { typography }, dense }) => ({
  padding: dense ? '2px 4px' : '3px 6px',
  fontSize: 13.5,
  fontFamily: typography.fontFamily,
  borderRadius: '4px',
  whiteSpace: 'nowrap',
}))

interface Props {
  delay: number
  selected: boolean
  bounds?: IProfileItem['ping_thresholds']
  onCheck: () => void
  showSelected?: boolean
  dense?: boolean
}

export const ProxyPing = ({
  delay,
  selected,
  bounds,
  onCheck,
  showSelected = true,
  dense,
}: Props) => {
  const { t } = useTranslation()

  if (delay === -2) {
    return (
      <Widget dense={dense}>
        <BaseLoading />
      </Widget>
    )
  }

  const check = (e: MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    onCheck()
  }
  return (
    <>
      <Widget
        dense={dense}
        className="the-check"
        onClick={check}
        sx={({ palette }) => ({
          display: 'none',
          ':hover': { bgcolor: alpha(palette.primary.main, 0.15) },
        })}
      >
        {t('proxies.page.actions.check')}
      </Widget>

      {delay < 0 && selected && showSelected ? (
        <CheckCircleOutlineRounded
          className="the-icon"
          sx={{ fontSize: 16, ...(dense && { mr: 0.5, display: 'block' }) }}
        />
      ) : (
        <Widget
          dense={dense}
          className="the-delay"
          title={failedDelay(delay) ? t('shared.labels.timeout') : undefined}
          onClick={check}
          sx={({ palette }) => ({
            ...PING_SX,
            color: delayColor(delay, bounds),
            ':hover': { bgcolor: alpha(palette.primary.main, 0.15) },
          })}
        >
          {delayText(delay)}
          {usableDelay(delay) && <PingUnit />}
        </Widget>
      )}
    </>
  )
}
