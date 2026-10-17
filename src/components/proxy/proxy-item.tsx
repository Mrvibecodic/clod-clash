import { ArrowForwardRounded } from '@mui/icons-material'
import {
  Box,
  ListItem,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  type SxProps,
  type Theme,
} from '@mui/material'

import { CodeChip, TypeChip } from '@/components/base'
import { useProxyDelayState } from '@/hooks/use-proxy-delay-state'
import { featureChips, typeChips } from '@/utils/proxy-label'

import { FreezeMark } from './freeze-mark'
import { ProxyFavorite } from './proxy-favorite'
import { ProxyPing } from './proxy-ping'
import { proxyTileSx } from './proxy-tile'

interface Props {
  group: IProxyGroupItem
  proxy: IProxyItem
  selected: boolean
  showType?: boolean
  sx?: SxProps<Theme>
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

export const ProxyItem = (props: Props) => {
  const {
    group,
    proxy,
    selected,
    showType = true,
    sx,
    onClick,
    favorite,
    onToggleFavorite,
    pingBounds,
    hideBadges,
    description,
    freezeMark,
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
    <ListItem sx={sx}>
      <ListItemButton
        dense
        selected={selected}
        disableRipple={!onClick}
        onClick={() => onClick?.(proxy.name)}
        sx={[
          {
            borderRadius: '10px',
            gap: 0.5,
            marginBottom: '8px',
            minHeight: '40px',
          },
          proxyTileSx(!!onClick, delayValue >= 0),
        ]}
      >
        <ListItemText
          title={proxy.name}
          secondary={
            <>
              <Box
                sx={{
                  display: 'inline-block',
                  marginRight: '8px',
                  fontSize: '14px',
                  fontWeight: selected ? 600 : 400,
                  color: 'text.primary',
                }}
              >
                {proxy.name}
              </Box>
              {showType && proxy.all ? (
                <>
                  {chip && <TypeChip sx={{ mr: '6px' }}>{chip}</TypeChip>}
                  {proxy.now && (
                    <>
                      <ArrowForwardRounded
                        sx={{
                          fontSize: 14,
                          mr: '6px',
                          verticalAlign: 'middle',
                          color: 'text.disabled',
                        }}
                      />
                      <Box
                        component="span"
                        sx={{ fontSize: 13, color: 'text.primary' }}
                      >
                        {proxy.now}
                      </Box>
                    </>
                  )}
                </>
              ) : (
                showType &&
                chip && (
                  <CodeChip title={proxy.label?.text} sx={{ mr: '6px' }}>
                    {chip}
                  </CodeChip>
                )
              )}
              {showType && !!proxy.provider && (
                <CodeChip sx={{ mr: '6px' }}>{proxy.provider}</CodeChip>
              )}
              {showType && description && (
                <Box
                  component="span"
                  title={description}
                  sx={{
                    display: 'block',
                    fontSize: 12,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {description}
                </Box>
              )}
            </>
          }
        />

        <FreezeMark mark={freezeMark} />

        <ListItemIcon
          sx={{
            justifyContent: 'flex-end',
            color: 'primary.main',
            display: isPreset ? 'none' : '',
          }}
        >
          <ProxyPing
            delay={delayValue}
            selected={selected}
            bounds={pingBounds}
            onCheck={() => onDelay(proxy.provider)}
          />
        </ListItemIcon>

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
    </ListItem>
  )
}
