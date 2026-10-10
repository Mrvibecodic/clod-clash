import {
  ArrowForwardRounded,
  ExpandMoreRounded,
  InboxRounded,
} from '@mui/icons-material'
import { Box, ListItemButton, Typography, styled } from '@mui/material'
import { memo, type ReactNode, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { TypeChip } from '@/components/base'
import { useIconCache } from '@/hooks/use-icon-cache'
import { useVerge } from '@/hooks/use-verge'
import { groupType, SELECTABLE_GROUP_TYPES } from '@/utils/proxy-groups'

import { ProxyGroupTools } from './proxy-group-tools'
import { ProxyHead } from './proxy-head'
import { ProxyItem } from './proxy-item'
import { ProxyItemMini } from './proxy-item-mini'
import type { HeadState } from './use-head-state'
import type { IRenderItem } from './use-render-list'

interface RenderProps {
  item: IRenderItem
  stickyed?: boolean
  isChainMode?: boolean
  onLocation: (group: IRenderItem['group']) => void
  onCheckAll: (groupName: string) => void
  onHeadState: (groupName: string, patch: Partial<HeadState>) => void
  onChangeProxy: (
    group: IRenderItem['group'],
    proxy: IRenderItem['proxy'] & { name: string },
  ) => void
  onGroupToggle?: (group: IRenderItem['group']) => void
  favorites?: Set<string>
  onToggleFavorite?: (name: string) => void
  pingBounds?: IProfileItem['ping_thresholds']
  /** Провайдер спрятал плашки протокола, транспорта и защиты. */
  hideBadges?: boolean
  /**
   * Описания узлов и пометки 16–20 читает список, а не строка: строки
   * виртуального списка монтируются заново при прокрутке, и запрос в каждой
   * перечитывал бы их на ходу.
   */
  descriptions?: Record<string, string>
  freezeMarks?: Record<string, FreezeMark>
}

export const ProxyRender = memo(function ProxyRender(props: RenderProps) {
  const { t } = useTranslation()
  const {
    item,
    stickyed = false,
    onLocation,
    onCheckAll,
    onHeadState,
    onChangeProxy,
    onGroupToggle,
    favorites,
    onToggleFavorite,
    isChainMode = false,
    pingBounds,
    hideBadges,
    descriptions,
    freezeMarks,
  } = props
  const { type, group, headState, proxy, proxyCol } = item
  const selectable = isChainMode || SELECTABLE_GROUP_TYPES.has(groupType(group))
  const { verge } = useVerge()
  const enable_group_icon = verge?.enable_group_icon ?? true
  const iconCachePath = useIconCache({
    icon: group.icon,
    cacheKey: group.name.replaceAll(' ', ''),
    enabled: enable_group_icon,
  })

  const showType = headState?.showType
  const proxyColItemsMemo = useMemo(() => {
    if (type !== 4 || !proxyCol) {
      return null
    }

    return proxyCol.map((proxyItem) => (
      <ProxyItemMini
        key={`${item.key}-${proxyItem?.name ?? 'unknown'}`}
        group={group}
        proxy={proxyItem}
        selected={group.now === proxyItem?.name}
        showType={showType}
        onClick={selectable ? () => onChangeProxy(group, proxyItem) : undefined}
        favorite={favorites?.has(proxyItem?.name)}
        onToggleFavorite={onToggleFavorite}
        pingBounds={pingBounds}
        hideBadges={hideBadges}
        description={descriptions?.[proxyItem?.name]}
        freezeMark={freezeMarks?.[proxyItem?.name]}
      />
    ))
  }, [
    type,
    proxyCol,
    item.key,
    group,
    showType,
    selectable,
    onChangeProxy,
    favorites,
    onToggleFavorite,
    pingBounds,
    hideBadges,
    descriptions,
    freezeMarks,
  ])

  if (type === 0) {
    const open = !!headState?.open
    return (
      <Box
        sx={{
          p: open ? '5px 8px 0' : '5px 8px',
          bgcolor: stickyed ? 'background.default' : undefined,
        }}
      >
        <ListItemButton
          dense
          sx={({ palette, shadows }) => ({
            height: '100%',
            pl: 2,
            pr: 1.25,
            py: 1.25,
            bgcolor: 'background.paper',
            border: `1px solid ${palette.divider}`,
            borderBottomColor: open ? 'transparent' : palette.divider,
            borderRadius: open ? '12px 12px 0 0' : '12px',
            // clod:design-v3 — липкий заголовок группы отрывается от списка
            // общей лестницей теней, без ручного !important.
            boxShadow: stickyed && open ? shadows[6] : 'none',
            '&:hover, &.Mui-focusVisible': { bgcolor: 'background.paper' },
          })}
          onClick={() => {
            if (headState?.open) {
              onGroupToggle?.(group)
            }
            onHeadState?.(group.name, { open: !headState?.open })
          }}
        >
          <Box sx={{ display: 'flex', alignItems: 'center', width: '100%' }}>
            {enable_group_icon && group.icon?.trim().startsWith('http') && (
              <img
                src={iconCachePath === '' ? group.icon : iconCachePath}
                alt="group icon"
                width="32px"
                style={{ marginRight: '12px', borderRadius: '6px' }}
              />
            )}
            {enable_group_icon && group.icon?.trim().startsWith('data') && (
              <img
                src={group.icon}
                alt="group icon"
                width="32px"
                style={{ marginRight: '12px', borderRadius: '6px' }}
              />
            )}
            {enable_group_icon && group.icon?.trim().startsWith('<svg') && (
              <img
                src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(group.icon)}`}
                alt="group icon"
                width="32px"
              />
            )}
            <Box sx={{ flex: '0 1 auto', minWidth: 0 }}>
              <StyledPrimary>{group.name}</StyledPrimary>
              <Box
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 0.75,
                  mt: '3px',
                  minWidth: 0,
                }}
              >
                <TypeChip>{group.type}</TypeChip>
                {group.now && (
                  <>
                    <ArrowForwardRounded
                      sx={{ fontSize: 14, color: 'text.disabled' }}
                    />
                    <Typography
                      component="span"
                      noWrap
                      title={group.now}
                      sx={{ fontSize: 13, fontWeight: 600, minWidth: 0 }}
                    >
                      {group.now}
                    </Typography>
                  </>
                )}
              </Box>
            </Box>
            <Box
              sx={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'end',
                flex: '1 1 auto',
                minWidth: 0,
              }}
            >
              <ProxyGroupTools
                groupName={group.name}
                headState={headState!}
                onLocation={() => onLocation(group)}
                onCheckDelay={() => onCheckAll(group.name)}
                onHeadState={(p) => onHeadState(group.name, p)}
              />
              <Typography
                component="span"
                title={t('proxies.page.labels.proxyCount')}
                sx={{
                  ml: 1,
                  mr: 0.5,
                  fontSize: 12.5,
                  fontWeight: 600,
                  color: 'text.secondary',
                  whiteSpace: 'nowrap',
                }}
              >
                {t('proxies.page.chain.nodeCount', {
                  count: group.all.length,
                })}
              </Typography>
              <ExpandMoreRounded
                sx={{
                  color: 'text.secondary',
                  transform: open ? 'rotate(180deg)' : 'none',
                  transition: 'transform 200ms cubic-bezier(0.2, 0, 0, 1)',
                  '@media (prefers-reduced-motion: reduce)': {
                    transition: 'none',
                  },
                }}
              />
            </Box>
          </Box>
        </ListItemButton>
      </Box>
    )
  }

  const framed = !isChainMode
  const first = type === 1
  const frame = (content: ReactNode) =>
    framed ? (
      <Box sx={{ px: 1, pt: first ? '5px' : 0, pb: item.last ? '5px' : 0 }}>
        <Box
          sx={({ palette }) => ({
            bgcolor: 'background.paper',
            border: `1px solid ${palette.divider}`,
            borderTopWidth: first ? 1 : 0,
            borderBottomWidth: item.last ? 1 : 0,
            borderRadius: `${first ? '12px 12px' : '0 0'} ${item.last ? '12px 12px' : '0 0'}`,
            pb: item.last ? 1 : 0,
          })}
        >
          {content}
        </Box>
      </Box>
    ) : (
      content
    )

  if (type === 1) {
    return frame(
      <ProxyHead
        sx={{ pl: 2, pr: 3, pt: 1, mb: 1 }}
        groupName={group.name}
        headState={headState!}
        onLocation={() => onLocation(group)}
        onCheckDelay={() => onCheckAll(group.name)}
        onHeadState={(p) => onHeadState(group.name, p)}
      />,
    )
  }

  if (type === 2) {
    return frame(
      <ProxyItem
        group={group}
        proxy={proxy!}
        selected={group.now === proxy?.name}
        showType={headState?.showType}
        sx={{ py: 0, px: framed ? 1.5 : 2 }}
        onClick={selectable ? () => onChangeProxy(group, proxy!) : undefined}
        favorite={favorites?.has(proxy!.name)}
        onToggleFavorite={onToggleFavorite}
        pingBounds={pingBounds}
        hideBadges={hideBadges}
        description={descriptions?.[proxy!.name]}
        freezeMark={freezeMarks?.[proxy!.name]}
      />,
    )
  }

  if (type === 3) {
    return frame(
      <Box
        sx={{
          py: 2,
          pl: 0,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'text.secondary',
        }}
      >
        <InboxRounded sx={{ fontSize: '2.5em', color: 'inherit' }} />
        <Typography sx={{ color: 'inherit' }}>
          {t('proxies.page.labels.noProxies')}
        </Typography>
      </Box>,
    )
  }

  if (type === 4) {
    return frame(
      <Box
        sx={{
          minHeight: 56,
          display: 'grid',
          py: 0.5,
          gap: 1,
          px: framed ? 1.5 : 2,
          gridTemplateColumns: `repeat(${item.col! || 2}, minmax(0, 1fr))`,
        }}
      >
        {proxyColItemsMemo}
      </Box>,
    )
  }

  return null
})

const StyledPrimary = styled('div')`
  font-size: 16px;
  font-weight: 700;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`
