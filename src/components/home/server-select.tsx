import AltRouteRoundedIcon from '@mui/icons-material/AltRouteRounded'
import BoltRoundedIcon from '@mui/icons-material/BoltRounded'
import CheckRoundedIcon from '@mui/icons-material/CheckRounded'
import ExpandMoreRoundedIcon from '@mui/icons-material/ExpandMoreRounded'
import StarBorderRoundedIcon from '@mui/icons-material/StarBorderRounded'
import StarRoundedIcon from '@mui/icons-material/StarRounded'
import {
  alpha,
  Box,
  Button,
  ButtonBase,
  CircularProgress,
  Drawer,
  IconButton,
  List,
  ListItemButton,
  Stack,
  Typography,
} from '@mui/material'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CountryFlag } from '@/components/home/country-flag'
import { useGroupDelayTest } from '@/hooks/use-group-delay-test'
import { useProfiles } from '@/hooks/use-profiles'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import { showNotice } from '@/services/notice-service'
import { nameWithoutFlag } from '@/utils/country'
import { delayColor } from '@/utils/delay-color'
import {
  AUTO_GROUP_TYPES,
  displayLeaf,
  entryDelay,
  groupType,
  NON_NODE_TYPES,
  type ProxyNode,
  SELECTABLE_GROUP_TYPES,
  usableDelay,
  visibleGroups,
} from '@/utils/proxy-groups'

const VIRTUALIZE_FROM = 50
const ROW_HEIGHT = 52

interface Props {
  open: boolean
  onClose: () => void
}

/**
 * Full server list, opened over the home screen rather than as its own page:
 * picking a server is a detour from connecting, not a destination.
 *
 * Groups sit in a scrollable row of chips above the list — templates in the
 * wild (Davoyan, legiz) ship half a dozen per-service groups plus balancers,
 * and a dropdown hid all of that.
 */
export const ServerSelect = ({ open, onClose }: Props) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()
  const { refreshProxy } = useAppRefreshers()
  // `selectNodeForGroup` talks straight to the core, so nothing tells the
  // frontend to re-read the proxies — without the explicit refresh the tick
  // and the "current server" row keep showing the previous node.
  const { changeProxy } = useProxySelection({
    onSuccess: () => {
      refreshProxy().catch(() => {})
    },
    onError: (error) => showNotice.error(error),
  })
  const [testing, setTesting] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)

  const records = (proxies?.records ?? {}) as Record<string, any>
  const groups = useMemo(() => visibleGroups(proxies), [proxies])
  const [groupName, setGroupName] = useState<string>('')
  const group = useMemo(
    () => groups.find((item) => item.name === groupName) ?? groups[0],
    [groups, groupName],
  )
  const canSelect = SELECTABLE_GROUP_TYPES.has(groupType(group))

  // Starred servers float to the top of the list; the stars live on the
  // profile, so they survive subscription updates and app restarts.
  const { current, patchCurrent } = useProfiles()
  const favorites = useMemo(
    () => new Set(current?.favorites ?? []),
    [current?.favorites],
  )

  const toggleFavorite = useLockFn(async (nodeName: string) => {
    if (!current?.uid) return
    const stored = current.favorites ?? []
    const next = favorites.has(nodeName)
      ? stored.filter((name) => name !== nodeName)
      : [...stored, nodeName]
    try {
      await patchCurrent({ favorites: next })
    } catch (error) {
      showNotice.error(error)
    }
  })

  const nodes = useMemo(() => {
    const all = group?.all ?? []
    if (favorites.size === 0) return all
    return [
      ...all.filter((node) => favorites.has(node.name)),
      ...all.filter((node) => !favorites.has(node.name)),
    ]
  }, [group, favorites])

  const virtualizer = useVirtualizer({
    count: nodes.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
    enabled: nodes.length > VIRTUALIZE_FROM,
  })

  const select = useLockFn(async (nodeName: string) => {
    if (!group || !canSelect) return
    await changeProxy(group.name, nodeName)
    onClose()
  })

  // clod: тест с keepFixed + восстановлением сохранённого выбора — иначе
  // mihomo сбрасывал закреплённый узел url-test групп при каждом тесте.
  const runGroupDelayTest = useGroupDelayTest()
  const runDelayTest = useCallback(async () => {
    if (!group) return
    setTesting(true)
    try {
      await runGroupDelayTest(group.name)
    } catch (error) {
      showNotice.error(error)
    } finally {
      setTesting(false)
    }
  }, [group, runGroupDelayTest])

  const typeLabel = (type: string) => {
    switch (type) {
      case 'urltest':
        return t('home.components.serverSelect.types.urltest')
      case 'fallback':
        return t('home.components.serverSelect.types.fallback')
      case 'loadbalance':
        return t('home.components.serverSelect.types.loadbalance')
      case 'smart':
        return t('home.components.serverSelect.types.smart')
      case 'selector':
        return t('home.components.serverSelect.types.selector')
      default:
        return type
    }
  }

  const renderRow = (node: ProxyNode) => {
    const record = records[node.name]
    const type = groupType(record ?? node)
    const isGroup = Boolean(record?.all) || NON_NODE_TYPES.has(type)
    const delay = entryDelay(records, node.name, group?.name ?? '')
    const selected = group?.now === node.name
    const starred = favorites.has(node.name)
    // clod: служебные имена ядра (COMPATIBLE и т.п.) в подписи не показываем
    const leaf = isGroup ? displayLeaf(records, node.name) : undefined

    return (
      <ListItemButton
        key={node.name}
        selected={selected}
        onClick={() => void select(node.name)}
        sx={{
          borderRadius: 1,
          height: ROW_HEIGHT,
          gap: 1.25,
          cursor: canSelect ? 'pointer' : 'default',
        }}
      >
        {isGroup ? (
          <Box
            sx={{
              width: 26,
              height: 26,
              borderRadius: '50%',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flex: 'none',
              color: 'primary.main',
              bgcolor: (theme) => alpha(theme.palette.primary.main, 0.13),
            }}
          >
            {AUTO_GROUP_TYPES.has(type) ? (
              <BoltRoundedIcon sx={{ fontSize: 17 }} />
            ) : (
              <AltRouteRoundedIcon sx={{ fontSize: 16 }} />
            )}
          </Box>
        ) : (
          <CountryFlag name={node.name} />
        )}
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography noWrap>{nameWithoutFlag(node.name)}</Typography>
          {/* clod: у активного сервера подпись «Используется», не тип узла */}
          <Typography
            variant="caption"
            color={selected ? 'success.main' : 'text.secondary'}
            noWrap
          >
            {selected
              ? isGroup && leaf
                ? `${t('home.components.serverSelect.inUse')} · ${nameWithoutFlag(leaf)}`
                : t('home.components.serverSelect.inUse')
              : isGroup
                ? leaf
                  ? `${typeLabel(type)} · ${nameWithoutFlag(leaf)}`
                  : typeLabel(type)
                : node.type}
          </Typography>
        </Box>
        {/* clod: маркер ошибки (1e6) — это не пинг, показываем прочерк */}
        <Typography
          variant="body2"
          sx={{
            color: usableDelay(delay) ? delayColor(delay) : 'text.disabled',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {usableDelay(delay) ? `${delay} ms` : '—'}
        </Typography>
        {isGroup ? null : (
          <IconButton
            size="small"
            aria-label={t('home.components.serverSelect.favorite')}
            sx={{ color: starred ? 'warning.main' : 'text.disabled' }}
            onClick={(event) => {
              event.stopPropagation()
              void toggleFavorite(node.name)
            }}
          >
            {starred ? (
              <StarRoundedIcon fontSize="small" />
            ) : (
              <StarBorderRoundedIcon fontSize="small" />
            )}
          </IconButton>
        )}
        {selected ? (
          <CheckRoundedIcon color="success" fontSize="small" />
        ) : null}
      </ListItemButton>
    )
  }

  return (
    <Drawer
      anchor="bottom"
      open={open}
      onClose={onClose}
      slotProps={{
        paper: { sx: { maxHeight: '80vh', borderRadius: '12px 12px 0 0' } },
      }}
    >
      <Stack sx={{ p: 2, gap: 1.5, minHeight: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Typography variant="h6">
            {t('home.components.serverSelect.title')}
          </Typography>

          <Box sx={{ flex: 1 }} />

          <Button
            size="small"
            startIcon={
              testing ? <CircularProgress size={16} /> : <BoltRoundedIcon />
            }
            disabled={testing || !group}
            onClick={() => void runDelayTest()}
          >
            {t('home.components.serverSelect.test')}
          </Button>
        </Stack>

        {/* Group chips in a row: every visible group of the template, side
            by side — never a dropdown. Scrolls when they don't fit. */}
        {groups.length > 1 ? (
          <Stack
            direction="row"
            sx={{
              gap: 0.75,
              overflowX: 'auto',
              flex: 'none',
              pb: 0.5,
              // a slim scrollbar so a long chip row stays discoverable
              '&::-webkit-scrollbar': { height: 4 },
              '&::-webkit-scrollbar-thumb': {
                bgcolor: 'divider',
                borderRadius: 2,
              },
            }}
          >
            {groups.map((item) => {
              const active = item.name === (group?.name ?? '')
              const auto = AUTO_GROUP_TYPES.has(groupType(item))
              // clod: the resolved node right on the chip, so checking what
              // each group runs on does not require opening every group
              const leaf = displayLeaf(records, item.name)
              return (
                <ButtonBase
                  key={item.name}
                  onClick={() => setGroupName(item.name)}
                  sx={{
                    px: 1.5,
                    py: 0.75,
                    borderRadius: '10px',
                    whiteSpace: 'nowrap',
                    flex: 'none',
                    flexDirection: 'column',
                    alignItems: 'flex-start',
                    justifyContent: 'center',
                    gap: 0.25,
                    color: active ? 'primary.main' : 'text.primary',
                    bgcolor: (theme) =>
                      active
                        ? alpha(theme.palette.primary.main, 0.13)
                        : theme.palette.action.hover,
                    border: (theme) =>
                      `1px solid ${
                        active ? theme.palette.primary.main : 'transparent'
                      }`,
                  }}
                >
                  <Box
                    sx={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 0.5,
                      fontSize: 13,
                      fontWeight: 600,
                    }}
                  >
                    {auto ? <BoltRoundedIcon sx={{ fontSize: 15 }} /> : null}
                    {nameWithoutFlag(item.name)}
                  </Box>
                  {leaf ? (
                    <Box
                      sx={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 0.5,
                        minWidth: 0,
                      }}
                    >
                      {/* clod: тот же флаг, что и в строках списка */}
                      <CountryFlag name={leaf} size={13} />
                      <Typography
                        variant="caption"
                        noWrap
                        sx={{
                          maxWidth: 180,
                          color: active ? 'primary.main' : 'text.secondary',
                          opacity: active ? 0.8 : 1,
                          lineHeight: 1.2,
                        }}
                      >
                        {nameWithoutFlag(leaf)}
                      </Typography>
                    </Box>
                  ) : null}
                </ButtonBase>
              )
            })}
          </Stack>
        ) : null}

        {!canSelect && group ? (
          <Typography variant="caption" color="text.secondary">
            {t('home.components.serverSelect.autoHint')}
          </Typography>
        ) : null}

        {nodes.length === 0 ? (
          <Typography
            color="text.secondary"
            sx={{ py: 4, textAlign: 'center' }}
          >
            {t('home.components.serverSelect.empty')}
          </Typography>
        ) : nodes.length > VIRTUALIZE_FROM ? (
          <Box ref={scrollRef} sx={{ overflowY: 'auto', maxHeight: '60vh' }}>
            <Box
              sx={{
                height: virtualizer.getTotalSize(),
                position: 'relative',
                width: '100%',
              }}
            >
              {virtualizer.getVirtualItems().map((row) => (
                <Box
                  key={row.key}
                  sx={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${row.start}px)`,
                  }}
                >
                  {renderRow(nodes[row.index])}
                </Box>
              ))}
            </Box>
          </Box>
        ) : (
          <List sx={{ overflowY: 'auto', maxHeight: '60vh' }}>
            {nodes.map(renderRow)}
          </List>
        )}
      </Stack>
    </Drawer>
  )
}

interface RowProps {
  onOpen: () => void
}

/** Mockup-style four-bar signal indicator, coloured by latency. */
const SignalBars = ({ delay }: { delay?: number }) => {
  const lit = !usableDelay(delay)
    ? 0
    : delay < 120
      ? 4
      : delay < 250
        ? 3
        : delay < 500
          ? 2
          : 1
  const color = (index: number) =>
    index < lit ? (lit >= 3 ? 'success.main' : 'warning.main') : 'divider'
  return (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'flex-end',
        gap: '2px',
        height: 16,
        flex: 'none',
      }}
    >
      {[5, 8, 11, 15].map((height, index) => (
        <Box
          key={height}
          sx={{
            width: '3.5px',
            height,
            borderRadius: '2px',
            bgcolor: color(index),
          }}
        />
      ))}
    </Box>
  )
}

// clod: подписка обновилась → ядро перечитало конфиг → история задержек в
// ядре обнулилась и пинги на главной «пропадали». Автотест гоняем один раз
// на каждую пару (группа, момент обновления подписки) — module-scope, чтобы
// ремоунт экрана не пинговал повторно.
let lastAutoDelayKey = ''

/** The compact row on the home screen: current server, latency, one tap. */
export const ServerSelectRow = ({ onOpen }: RowProps) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()
  const { current: currentProfile } = useProfiles()
  const runGroupDelayTest = useGroupDelayTest()

  const records = (proxies?.records ?? {}) as Record<string, any>
  const group = visibleGroups(proxies)[0]
  const current = group?.now
  // The selection may be a balancer; the ping (and the flag) belong to the
  // node the chain actually lands on. Core placeholders (COMPATIBLE…) are
  // hidden — the flag then falls back to the selection's own name.
  const leaf = current ? displayLeaf(records, current) : undefined
  const flagName = leaf ?? current
  const delay = current
    ? entryDelay(records, current, group?.name ?? '')
    : undefined

  const groupName = group?.name
  const updatedAt = currentProfile?.updated ?? 0
  useEffect(() => {
    if (!groupName) return
    const key = `${groupName}|${updatedAt}`
    if (lastAutoDelayKey === key) return
    // подождать, пока ядро дожуёт свежий конфиг, и перепинговать сразу.
    // Тест идёт через useGroupDelayTest: keepFixed + восстановление выбора,
    // так что автоперепинговка после обновления подписки НЕ сбрасывает
    // выбранный сервер (и умеет уйти на избранный, если выбранный умер).
    const timer = window.setTimeout(() => {
      lastAutoDelayKey = key
      runGroupDelayTest(groupName).catch(() => {})
    }, 800)
    return () => window.clearTimeout(timer)
  }, [groupName, updatedAt, runGroupDelayTest])

  const caption = usableDelay(delay)
    ? leaf
      ? `${nameWithoutFlag(leaf)} · ${delay} ${t('home.components.serverSelect.ms')}`
      : `${delay} ${t('home.components.serverSelect.ms')}`
    : leaf
      ? nameWithoutFlag(leaf)
      : t('home.components.serverSelect.current')

  return (
    <Stack
      direction="row"
      onClick={onOpen}
      sx={{
        alignItems: 'center',
        gap: 1.5,
        px: 1.75,
        py: 1.5,
        // stretch to the column like every other card; an explicit
        // width:100% + padding overflowed it by the padding width
        alignSelf: 'stretch',
        boxSizing: 'border-box',
        borderRadius: '14px',
        cursor: 'pointer',
        bgcolor: 'background.paper',
        border: (theme) => `1px solid ${theme.palette.divider}`,
        '&:hover': { borderColor: 'primary.main' },
      }}
    >
      {flagName ? <CountryFlag name={flagName} size={26} /> : null}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography noWrap sx={{ fontSize: 14, fontWeight: 700 }}>
          {current
            ? nameWithoutFlag(current)
            : t('home.components.serverSelect.none')}
        </Typography>
        <Typography
          noWrap
          sx={{ fontSize: 12, display: 'block' }}
          color="text.secondary"
        >
          {caption}
        </Typography>
      </Box>

      <SignalBars delay={delay} />

      <IconButton
        size="small"
        aria-label={t('home.components.serverSelect.title')}
      >
        <ExpandMoreRoundedIcon />
      </IconButton>
    </Stack>
  )
}
