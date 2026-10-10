import {
  ArrowForwardRounded,
  CheckCircleOutlineRounded,
} from '@mui/icons-material'
import { alpha, Box, ListItemButton, styled, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { BaseLoading, CodeChip, TypeChip } from '@/components/base'
import { useProxyDelayState } from '@/hooks/use-proxy-delay-state'
import { SHAPE } from '@/pages/_theme'
import { delayColor, delayText, usableDelay } from '@/utils/delay-color'
import { featureChips, typeChips } from '@/utils/proxy-label'

import { FreezeMark } from './freeze-mark'
import { ProxyFavorite } from './proxy-favorite'
import { PingUnit } from './proxy-ping-unit'
import { PING_SX, proxyTileSx } from './proxy-tile'

interface Props {
  group: IProxyGroupItem
  proxy: IProxyItem
  selected: boolean
  showType?: boolean
  onClick?: (name: string) => void
  favorite?: boolean
  onToggleFavorite?: (name: string) => void
  pingBounds?: IProfileItem['ping_thresholds']
  /** Провайдер спрятал плашки протокола, транспорта и защиты. */
  hideBadges?: boolean
  /** clod: описание узла из панели — под типами и прячется вместе с ними. */
  description?: string
  /** clod:freeze — пометка рядом с пингом; пинг и выбор не трогает. */
  freezeMark?: Parameters<typeof FreezeMark>[0]['mark']
}

// Многоколоночная раскладка
export const ProxyItemMini = (props: Props) => {
  const {
    group,
    proxy,
    selected,
    showType = true,
    onClick,
    favorite,
    onToggleFavorite,
    pingBounds,
    hideBadges,
    description,
    freezeMark,
  } = props
  const { t } = useTranslation()

  const { delayValue, isPreset, onDelay } = useProxyDelayState(
    proxy,
    group.name,
  )

  const chip = [
    ...typeChips(proxy, hideBadges),
    ...featureChips(proxy, hideBadges),
  ].join(' · ')

  return (
    <ListItemButton
      dense
      selected={selected}
      disableRipple={!onClick}
      onClick={() => onClick?.(proxy.name)}
      sx={[
        {
          minHeight: 56,
          borderRadius: SHAPE.control,
          pl: 1.5,
          pr: 0.5,
          gap: 0.5,
          justifyContent: 'space-between',
          alignItems: 'center',
        },
        proxyTileSx(!!onClick, delayValue >= 0),
      ]}
    >
      <Box
        title={`${proxy.name}\n${proxy.now ?? ''}`}
        sx={{ overflow: 'hidden', flex: 1, minWidth: 0 }}
      >
        <Typography
          variant="body2"
          component="div"
          noWrap
          sx={{ color: 'text.primary', fontWeight: selected ? 600 : 400 }}
        >
          {proxy.name}
        </Typography>

        {showType && (
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              gap: 0.75,
              mt: '5px',
              minWidth: 0,
            }}
          >
            {proxy.all ? (
              <>
                {chip && <TypeChip>{chip}</TypeChip>}
                {proxy.now && (
                  <>
                    <ArrowForwardRounded
                      sx={{ fontSize: 14, color: 'text.disabled' }}
                    />
                    <Typography
                      variant="body2"
                      component="span"
                      noWrap
                      sx={{ color: 'text.primary', fontSize: 13, minWidth: 0 }}
                    >
                      {proxy.now}
                    </Typography>
                  </>
                )}
              </>
            ) : (
              <>
                {chip && (
                  <CodeChip title={proxy.label?.text} sx={{ minWidth: 0 }}>
                    {chip}
                  </CodeChip>
                )}
                {!!proxy.provider && (
                  <CodeChip sx={{ minWidth: 0 }}>{proxy.provider}</CodeChip>
                )}
              </>
            )}
          </Box>
        )}
        {showType && description && (
          <Typography
            variant="caption"
            component="div"
            noWrap
            title={description}
            sx={{ mt: '3px', color: 'text.secondary' }}
          >
            {description}
          </Typography>
        )}
      </Box>
      <FreezeMark mark={freezeMark} compact />
      <Box
        sx={{ ml: 0.5, color: 'primary.main', display: isPreset ? 'none' : '' }}
      >
        {delayValue === -2 && (
          <Widget>
            <BaseLoading />
          </Widget>
        )}
        {delayValue !== -2 && (
          <Widget
            className="the-check"
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              onDelay(proxy.provider)
            }}
            sx={({ palette }) => ({
              display: 'none', // показывать при hover
              ':hover': { bgcolor: alpha(palette.primary.main, 0.15) },
            })}
          >
            {t('proxies.page.actions.check')}
          </Widget>
        )}

        {delayValue >= 0 && (
          // Показываем задержку
          <Widget
            className="the-delay"
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
              onDelay(proxy.provider)
            }}
            sx={({ palette }) => ({
              ...PING_SX,
              color: delayColor(delayValue, pingBounds),
              ':hover': { bgcolor: alpha(palette.primary.main, 0.15) },
            })}
          >
            {delayText(delayValue, t('shared.labels.timeout'))}
            {usableDelay(delayValue) && <PingUnit />}
          </Widget>
        )}
        {proxy.type !== 'Direct' &&
          delayValue !== -2 &&
          delayValue < 0 &&
          selected && (
            // Показываем иконку выбранного
            <CheckCircleOutlineRounded
              className="the-icon"
              sx={{ fontSize: 16, mr: 0.5, display: 'block' }}
            />
          )}
      </Box>
      {onToggleFavorite && (
        <Box sx={{ width: 24, flex: 'none', display: 'flex' }}>
          <ProxyFavorite
            proxy={proxy}
            favorite={favorite}
            onToggle={onToggleFavorite}
          />
        </Box>
      )}
    </ListItemButton>
  )
}

const Widget = styled(Box)(({ theme: { typography } }) => ({
  padding: '2px 4px',
  fontSize: 13.5,
  fontFamily: typography.fontFamily,
  borderRadius: '4px',
  whiteSpace: 'nowrap',
}))
