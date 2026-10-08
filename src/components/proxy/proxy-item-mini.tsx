import { CheckCircleOutlineRounded } from '@mui/icons-material'
import { alpha, Box, ListItemButton, styled, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { BaseLoading } from '@/components/base'
import { useProxyDelayState } from '@/hooks/use-proxy-delay-state'
import { SHAPE } from '@/pages/_theme'
import { delayColor, delayText } from '@/utils/delay-color'
import { typeChips } from '@/utils/proxy-label'

import { FreezeMark } from './freeze-mark'
import { ProxyFavorite } from './proxy-favorite'

interface Props {
  group: IProxyGroupItem
  proxy: IProxyItem
  selected: boolean
  showType?: boolean
  onClick?: (name: string) => void
  favorite?: boolean
  onToggleFavorite?: (name: string) => void
  pingBounds?: IProfileItem['ping_thresholds']
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
    description,
    freezeMark,
  } = props
  const { t } = useTranslation()

  const { delayValue, isPreset, onDelay } = useProxyDelayState(
    proxy,
    group.name,
  )

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
          pr: 1,
          justifyContent: 'space-between',
          alignItems: 'center',
        },
        ({ palette: { mode, primary, background } }) => {
          const bgcolor = background.paper
          const selectedBg =
            mode === 'light'
              ? alpha(primary.main, 0.15)
              : alpha(primary.main, 0.35)
          const showDelay = delayValue >= 0
          const selectColor = mode === 'light' ? primary.main : primary.light

          return {
            '&:hover .the-check': { display: !showDelay ? 'block' : 'none' },
            '&:hover .the-delay': { display: showDelay ? 'block' : 'none' },
            '&:hover .the-icon': { display: 'none' },
            '&.Mui-selected': {
              width: `calc(100% + 3px)`,
              marginLeft: `-3px`,
              borderLeft: `3px solid ${selectColor}`,
              bgcolor: selectedBg,
            },
            ...(!onClick && {
              cursor: 'default',
              '&:hover': { backgroundColor: bgcolor },
              '&.Mui-selected:hover': { bgcolor: selectedBg },
            }),
            backgroundColor: bgcolor,
          }
        },
      ]}
    >
      <Box
        title={`${proxy.name}\n${proxy.now ?? ''}`}
        sx={{ overflow: 'hidden', flex: 1, minWidth: 0 }}
      >
        <Typography
          variant="body2"
          component="div"
          color="text.primary"
          sx={{
            display: 'block',
            textOverflow: 'ellipsis',
            wordBreak: 'break-all',
            overflow: 'hidden',
            whiteSpace: 'nowrap',
          }}
        >
          {proxy.name}
        </Typography>

        {showType && (
          <Box
            sx={{
              display: 'flex',
              flexWrap: 'nowrap',
              flex: 'none',
              marginTop: '4px',
            }}
          >
            {proxy.now && (
              <Typography
                variant="body2"
                component="div"
                color="text.secondary"
                sx={{
                  display: 'block',
                  textOverflow: 'ellipsis',
                  wordBreak: 'break-all',
                  overflow: 'hidden',
                  whiteSpace: 'nowrap',
                  marginRight: '8px',
                }}
              >
                {proxy.now}
              </Typography>
            )}
            {!!proxy.provider && (
              <TypeBox color="text.secondary" component="span">
                {proxy.provider}
              </TypeBox>
            )}
            {/* Протокол держит ширину, транспорт и защита сжимаются первыми. */}
            {typeChips(proxy).map((chip, index) => (
              <TypeBox
                key={chip}
                color="text.secondary"
                component="span"
                title={proxy.label?.text}
                sx={
                  index === 0
                    ? { flexShrink: 0 }
                    : {
                        minWidth: 0,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }
                }
              >
                {chip}
              </TypeBox>
            ))}
            {proxy.tfo && (
              <TypeBox color="text.secondary" component="span">
                TFO
              </TypeBox>
            )}
            {proxy.mptcp && (
              <TypeBox color="text.secondary" component="span">
                MPTCP
              </TypeBox>
            )}
            {proxy.smux && (
              <TypeBox color="text.secondary" component="span">
                SMUX
              </TypeBox>
            )}
          </Box>
        )}
        {showType && description && (
          <Typography
            variant="caption"
            component="div"
            color="text.secondary"
            noWrap
            title={description}
            sx={{ mt: '2px' }}
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
              color: delayColor(delayValue, pingBounds),
              ':hover': { bgcolor: alpha(palette.primary.main, 0.15) },
            })}
          >
            {delayText(delayValue, t('shared.labels.timeout'))}
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
      <ProxyFavorite
        proxy={proxy}
        favorite={favorite}
        onToggle={onToggleFavorite}
      />
    </ListItemButton>
  )
}

const Widget = styled(Box)(({ theme: { typography } }) => ({
  padding: '2px 4px',
  fontSize: 14,
  fontFamily: typography.fontFamily,
  borderRadius: '4px',
}))

const TypeBox = styled(Box, {
  shouldForwardProp: (prop) => prop !== 'component',
})<{ component?: React.ElementType }>(({ theme: { typography } }) => ({
  display: 'inline-block',
  border: '1px solid #ccc',
  borderColor: 'text.secondary',
  color: 'text.secondary',
  borderRadius: 4,
  fontSize: 10,
  fontFamily: typography.fontFamily,
  marginRight: '4px',
  marginTop: 'auto',
  padding: '0 4px',
  lineHeight: 1.5,
}))
