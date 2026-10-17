import { Box, ListItemButton, Typography } from '@mui/material'

import { CodeChip } from '@/components/base'
import { useProxyDelayState } from '@/hooks/use-proxy-delay-state'
import { SHAPE } from '@/pages/_theme'
import { featureChips, typeChips } from '@/utils/proxy-label'

import { FreezeMark } from './freeze-mark'
import { ProxyFavorite } from './proxy-favorite'
import { GroupNow } from './proxy-group-now'
import { ProxyPing } from './proxy-ping'
import { proxyTileSx } from './proxy-tile'

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
  frozen?: boolean
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
    frozen,
  } = props

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
              <GroupNow type={chip} now={proxy.now} />
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
      <FreezeMark frozen={frozen} />
      <Box
        sx={{ ml: 0.5, color: 'primary.main', display: isPreset ? 'none' : '' }}
      >
        <ProxyPing
          dense
          delay={delayValue}
          selected={selected}
          bounds={pingBounds}
          showSelected={proxy.type !== 'Direct'}
          onCheck={() => onDelay(proxy.provider)}
        />
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
