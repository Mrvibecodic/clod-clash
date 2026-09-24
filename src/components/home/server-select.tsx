import AltRouteRoundedIcon from '@mui/icons-material/AltRouteRounded'
import BoltRoundedIcon from '@mui/icons-material/BoltRounded'
import CheckRoundedIcon from '@mui/icons-material/CheckRounded'
import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
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
import dayjs from 'dayjs'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CountryFlag } from '@/components/home/country-flag'
import { NoServersStatus } from '@/components/home/no-servers-status'
import { useDrawerCapHeight } from '@/hooks/use-drawer-cap-height'
import { favoritesFirst, useFavorites } from '@/hooks/use-favorites'
import { useNoServersStatus } from '@/hooks/use-no-servers-status'
import { useProfiles } from '@/hooks/use-profiles'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useServerDescriptions } from '@/hooks/use-server-descriptions'
import { useVerge } from '@/hooks/use-verge'
import { useVisibility } from '@/hooks/use-visibility'
import { SHAPE, TINT } from '@/pages/_theme'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import delayManager, { effectiveLatencyTimeout } from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import { nameWithoutFlag } from '@/utils/country'
import { delayBars, delayColor } from '@/utils/delay-color'
import {
  AUTO_GROUP_TYPES,
  displayLeaf,
  entryDelay,
  entryMeasuredAt,
  entryPingTarget,
  failedDelay,
  groupType,
  hasRealNodes,
  isCorePlaceholder,
  NON_NODE_TYPES,
  type ProxyNode,
  SELECTABLE_GROUP_TYPES,
  usableDelay,
  visibleGroups,
} from '@/utils/proxy-groups'
import { toUnixSeconds } from '@/utils/subscription-status'

const VIRTUALIZE_FROM = 50
const ROW_HEIGHT = 52

const PingVerdict = ({ delay }: { delay?: number }) =>
  usableDelay(delay) ? (
    <CheckRoundedIcon sx={{ fontSize: 18, flex: 'none' }} color="success" />
  ) : failedDelay(delay) ? (
    <CloseRoundedIcon sx={{ fontSize: 18, flex: 'none' }} color="error" />
  ) : (
    <Typography variant="body2" sx={{ flex: 'none' }} color="text.disabled">
      —
    </Typography>
  )

const DRAWER_REFRESH_MS = 5000

const AGE_TICK_MS = 5000

interface Props {
  open: boolean
  onClose: () => void
}

export const ServerSelect = ({ open, onClose }: Props) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()
  const { refreshProxy } = useAppRefreshers()
  const { changeProxy } = useProxySelection({
    enableConnectionCleanup: false,
    onSuccess: () => {
      refreshProxy().catch(() => {})
    },
    onError: (error) => showNotice.error(error),
  })
  const [testing, setTesting] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)
  const { verge } = useVerge()

  const records = useMemo(
    () => (proxies?.records ?? {}) as Record<string, any>,
    [proxies],
  )
  const descriptions = useServerDescriptions()
  const groups = useMemo(() => visibleGroups(proxies), [proxies])
  const [groupName, setGroupName] = useState<string>('')
  const group = useMemo(
    () => groups.find((item) => item.name === groupName) ?? groups[0],
    [groups, groupName],
  )
  const canSelect = SELECTABLE_GROUP_TYPES.has(groupType(group))

  const { current, mutateProfiles } = useProfiles()
  const { favorites, toggleFavorite } = useFavorites()

  const {
    show: noServers,
    onlySentinels,
    partiallyDropped,
  } = useNoServersStatus(current)
  const listEmpty = Boolean(proxies) && !hasRealNodes(proxies)
  const showStatus = noServers && (onlySentinels || listEmpty)

  const nodes = useMemo(
    () =>
      favoritesFirst(
        (group?.all ?? []).filter((node) => !isCorePlaceholder(node.name)),
        favorites,
      ),
    [group, favorites],
  )

  const virtualizer = useVirtualizer({
    count: nodes.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
    enabled: nodes.length > VIRTUALIZE_FROM,
  })

  const visible = useVisibility()
  useEffect(() => {
    if (!open || !visible) return
    const timer = window.setInterval(() => {
      refreshProxy().catch(() => {})
    }, DRAWER_REFRESH_MS)
    return () => window.clearInterval(timer)
  }, [open, visible, refreshProxy])

  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!open || !visible) return
    const first = window.setTimeout(() => setNow(Date.now()), 0)
    const timer = window.setInterval(() => setNow(Date.now()), AGE_TICK_MS)
    return () => {
      window.clearTimeout(first)
      window.clearInterval(timer)
    }
  }, [open, visible])

  const measuredLabel = useMemo(() => {
    if (!open || !group || nodes.length === 0) return undefined
    const newest = nodes.reduce((latest, node) => {
      const at = entryMeasuredAt(records, node.name, group.name)
      return at > latest ? at : latest
    }, 0)
    if (!newest) return undefined
    const minutes = Math.floor((now - newest) / 60_000)
    return minutes < 1
      ? t('home.components.serverSelect.measuredJustNow')
      : t('home.components.serverSelect.measuredMinutesAgo', { minutes })
  }, [open, group, nodes, records, now, t])

  const select = useLockFn(async (nodeName: string) => {
    if (!group || !canSelect) return
    await changeProxy(group.name, nodeName)
    if (groups.length < 2) onClose()
  })

  // clod: мерим узлы по одному. Групповой обработчик ядра /group/{name}/delay на
  // время замера снимает закреплённый узел url-test/fallback, а при провале не
  // возвращает; по узлам закрепление не трогается и действует общий потолок.
  const runDelayTest = useCallback(async () => {
    if (!group) return
    setTesting(true)
    try {
      await delayManager.checkListDelay(
        nodes
          .map((node) => records[node.name] as IProxyItem | undefined)
          .filter((node): node is IProxyItem => !!node),
        group.name,
        effectiveLatencyTimeout(verge?.default_latency_timeout),
      )
    } finally {
      setTesting(false)
      refreshProxy().catch(() => {})
    }
  }, [group, nodes, records, verge?.default_latency_timeout, refreshProxy])

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
    const leaf = isGroup ? displayLeaf(records, node.name) : undefined
    const description = isGroup ? undefined : descriptions[node.name]

    return (
      <ListItemButton
        key={node.name}
        selected={selected}
        disableRipple={!canSelect}
        onClick={() => void select(node.name)}
        sx={[
          {
            borderRadius: SHAPE.control,
            height: ROW_HEIGHT,
            gap: 1.25,
          },
          !canSelect &&
            (({ palette }) => ({
              cursor: 'default',
              '&:hover': { backgroundColor: 'transparent' },
              '&.Mui-selected:hover': {
                backgroundColor: alpha(
                  palette.primary.main,
                  palette.action.selectedOpacity,
                ),
              },
            })),
        ]}
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
              bgcolor: (theme) => alpha(theme.palette.primary.main, TINT.base),
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
          <Typography
            variant="caption"
            color={selected && !description ? 'success.main' : 'text.secondary'}
            noWrap
            title={description}
          >
            {description ??
              (selected
                ? isGroup && leaf
                  ? `${t('home.components.serverSelect.inUse')} · ${nameWithoutFlag(leaf)}`
                  : t('home.components.serverSelect.inUse')
                : isGroup
                  ? leaf
                    ? `${typeLabel(type)} · ${nameWithoutFlag(leaf)}`
                    : typeLabel(type)
                  : node.type)}
          </Typography>
        </Box>
        {current?.disable_ping ? (
          <PingVerdict delay={delay} />
        ) : (
          <Typography
            variant="body2"
            sx={{
              color: usableDelay(delay) ? delayColor(delay) : 'text.disabled',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            {usableDelay(delay) ? `${delay} ms` : '—'}
          </Typography>
        )}
        {isGroup ? null : (
          <IconButton
            size="small"
            aria-label={t('home.components.serverSelect.favorite')}
            sx={{ color: starred ? 'warning.main' : 'text.disabled' }}
            onMouseDown={(event) => event.stopPropagation()}
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

  const capHeight = useDrawerCapHeight(open)

  return (
    <Drawer
      anchor="bottom"
      open={open}
      onClose={onClose}
      slotProps={{
        paper: {
          sx: {
            maxHeight: capHeight,
            borderRadius: `${SHAPE.overlay} ${SHAPE.overlay} 0 0`,
            overflow: 'hidden',
          },
        },
      }}
    >
      <Stack sx={{ p: 2, gap: 1.5, minHeight: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Typography variant="h6">
            {t('home.components.serverSelect.title')}
          </Typography>

          {measuredLabel ? (
            <Typography variant="caption" color="text.secondary" noWrap>
              {measuredLabel}
            </Typography>
          ) : null}

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

          <IconButton
            size="small"
            edge="end"
            aria-label={t('shared.actions.close')}
            onClick={onClose}
          >
            <CloseRoundedIcon fontSize="small" />
          </IconButton>
        </Stack>

        {groups.length > 1 ? (
          <Stack
            direction="row"
            sx={{
              gap: 0.75,
              overflowX: 'auto',
              flex: 'none',
              pb: 0.5,
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
                        ? alpha(theme.palette.primary.main, TINT.base)
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

        {showStatus || partiallyDropped ? (
          <NoServersStatus
            profile={current}
            onRefreshed={mutateProfiles}
            quiet={!showStatus}
          />
        ) : null}

        {nodes.length === 0 ? (
          <Typography
            color="text.secondary"
            sx={{ py: 4, textAlign: 'center' }}
          >
            {t('home.components.serverSelect.empty')}
          </Typography>
        ) : nodes.length > VIRTUALIZE_FROM ? (
          <Box ref={scrollRef} sx={{ overflowY: 'auto', minHeight: 0 }}>
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
          <List sx={{ overflowY: 'auto', minHeight: 0 }}>
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

const SignalDot = ({ delay }: { delay?: number }) => (
  <Box
    sx={{
      width: 10,
      height: 10,
      flex: 'none',
      borderRadius: '50%',
      bgcolor: usableDelay(delay) ? delayColor(delay) : 'divider',
    }}
  />
)

const LatencyNumber = ({ delay }: { delay?: number }) => (
  <Typography
    sx={{ fontSize: 12, flex: 'none', fontVariantNumeric: 'tabular-nums' }}
    color={usableDelay(delay) ? delayColor(delay) : 'text.disabled'}
  >
    {usableDelay(delay) ? `${delay} ms` : '—'}
  </Typography>
)

const SignalBars = ({ delay }: { delay?: number }) => {
  const lit = usableDelay(delay) ? delayBars(delay) : 0
  const color = (index: number) => (index < lit ? delayColor(delay) : 'divider')
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

let lastAutoDelayKey = ''
const CORE_MEASURED_TYPES = new Set(['urltest', 'fallback', 'loadbalance'])

let lastKnownPing: { key: string; delay: number } | undefined

const PING_MAX_AGE_MS = 60_000
const PING_GAP_MS = 60_000
const PING_RETRY_MS = 5_000
const PING_RETRY_LIMIT = 6
let lastAutoPingAt = 0

export const ServerSelectRow = ({ onOpen }: RowProps) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()
  const { current: currentProfile } = useProfiles()
  const descriptions = useServerDescriptions()
  const visible = useVisibility()
  const { verge } = useVerge()
  const { refreshProxy } = useAppRefreshers()

  const records = (proxies?.records ?? {}) as Record<string, any>
  const group = visibleGroups(proxies)[0]
  const selection = group?.now
  const current = isCorePlaceholder(selection) ? undefined : selection
  const leaf = current ? displayLeaf(records, current) : undefined
  const flagName = leaf ?? current
  const delay = current
    ? entryDelay(records, current, group?.name ?? '')
    : undefined
  const measuredAt = current
    ? entryMeasuredAt(records, current, group?.name ?? '')
    : 0
  const pingTarget = current
    ? entryPingTarget(records, current, group?.name ?? '')
    : undefined
  const measuredAtRef = useRef(measuredAt)
  measuredAtRef.current = measuredAt
  const hasPing = usableDelay(delay)
  const pingProvider = pingTarget
    ? (records[pingTarget]?.provider as string | undefined)
    : undefined

  const pingKey = current
    ? `${group?.name ?? ''}::${current}::${leaf ?? ''}`
    : ''
  const pingFailed = failedDelay(delay)
  useEffect(() => {
    if (usableDelay(delay)) lastKnownPing = { key: pingKey, delay }
    else if (failedDelay(delay) && lastKnownPing?.key === pingKey) {
      lastKnownPing = undefined
    }
  }, [pingKey, delay])
  const remembered =
    lastKnownPing?.key === pingKey ? lastKnownPing.delay : undefined
  const shownDelay = hasPing || pingFailed ? delay : remembered

  const groupName = group?.name
  const updatedAt = currentProfile?.updated ?? 0
  const timeout = effectiveLatencyTimeout(verge?.default_latency_timeout)
  const testNodesRef = useRef<IProxyItem[]>([])
  testNodesRef.current = (group?.all ?? [])
    .filter((node) => !isCorePlaceholder(node.name))
    .map((node) => records[node.name] as IProxyItem | undefined)
    .filter((node): node is IProxyItem => !!node)
  // Состав узлов в ключе: профиль из трея сменил `updated` раньше, чем пришли
  // новые узлы, — без него автотест промерил бы узлы прежней подписки.
  const testNodesSig = testNodesRef.current.map((node) => node.name).join('\n')
  // Авто-группы ядро меряет само — при каждой загрузке конфига и раз в
  // interval; наш прогон поверх был бы вторым замером тех же узлов.
  const coreMeasures = CORE_MEASURED_TYPES.has(group?.type?.toLowerCase() ?? '')
  // clod: автотест — только пока окно на экране (в трее — ничего) и по узлам:
  // групповой обработчик ядра снимал закрепление url-test/fallback.
  useEffect(() => {
    if (!visible || !groupName || !testNodesSig || coreMeasures) return
    const key = `${groupName}|${updatedAt}|${testNodesSig}`
    if (lastAutoDelayKey === key) return
    const timer = window.setTimeout(() => {
      lastAutoDelayKey = key
      lastAutoPingAt = Date.now()
      delayManager
        .checkListDelay(testNodesRef.current, groupName, timeout)
        .finally(() => {
          refreshProxy().catch(() => {})
        })
    }, 800)
    return () => window.clearTimeout(timer)
  }, [
    visible,
    groupName,
    updatedAt,
    testNodesSig,
    coreMeasures,
    timeout,
    refreshProxy,
  ])

  useEffect(() => {
    if (!visible || !groupName || !pingTarget) return

    let attempts = 0
    let cancelled = false
    const measure = () => {
      if (cancelled) return
      const now = Date.now()
      const measuredAt = measuredAtRef.current
      if (measuredAt && now - measuredAt < PING_MAX_AGE_MS) return
      if (now - lastAutoPingAt < (hasPing ? PING_GAP_MS : PING_RETRY_MS)) return

      lastAutoPingAt = now
      attempts += 1
      // Через менеджер: замер виден сразу, а не только из истории ядра — у узлов
      // провайдеров её приносит кэш провайдеров
      delayManager
        .checkDelay(pingTarget, groupName, timeout, pingProvider)
        .finally(() => refreshProxy().catch(() => {}))
        .catch(() => {})
    }

    const timer = window.setTimeout(measure, 600)
    // Живой пинг, пока окно на экране (`visible`; в трее эффект снят, на
    // возврате — заново): раз в PING_GAP_MS сначала перечитываем прокси — у
    // url-test групп ядро мерит само, — и шлём свой запрос только если свежего
    // замера нет. Возраст берём из ref: перечитывание обновляет его мимо замыкания.
    const live = window.setInterval(() => {
      refreshProxy()
        .catch(() => {})
        .then(measure)
    }, PING_GAP_MS)
    const retry = hasPing
      ? undefined
      : window.setInterval(() => {
          if (attempts >= PING_RETRY_LIMIT) {
            window.clearInterval(retry)
            return
          }
          measure()
        }, PING_RETRY_MS)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
      window.clearInterval(live)
      if (retry !== undefined) window.clearInterval(retry)
    }
  }, [
    visible,
    groupName,
    measuredAt,
    hasPing,
    pingTarget,
    pingProvider,
    timeout,
    refreshProxy,
  ])

  const {
    reason,
    show: noServers,
    onlySentinels,
    partiallyDropped,
    droppedTotal,
  } = useNoServersStatus(currentProfile)
  const listEmpty = Boolean(proxies) && !hasRealNodes(proxies)
  const statusRow = noServers && (listEmpty || onlySentinels)
  const refillDate = currentProfile?.refill_date
    ? dayjs(toUnixSeconds(currentProfile.refill_date) * 1000).format(
        'DD.MM.YYYY',
      )
    : undefined
  const statusCaption = statusRow
    ? reason === 'traffic' && refillDate
      ? t('home.components.serverStatus.row.traffic', { date: refillDate })
      : t(
          `home.components.serverStatus.row.${reason === 'traffic' ? 'trafficNoDate' : reason}`,
        )
    : undefined
  const statusColor =
    reason === 'expired'
      ? 'error.main'
      : reason === 'traffic' || reason === 'deviceLimit'
        ? 'warning.main'
        : 'text.secondary'

  const description =
    (leaf ? descriptions[leaf] : undefined) ??
    (current ? descriptions[current] : undefined)
  const subject = description ?? (leaf ? nameWithoutFlag(leaf) : undefined)
  const caption = currentProfile?.disable_ping
    ? (subject ?? '—')
    : usableDelay(shownDelay)
      ? subject
        ? `${subject} · ${shownDelay} ${t('home.components.serverSelect.ms')}`
        : `${shownDelay} ${t('home.components.serverSelect.ms')}`
      : (subject ?? '—')

  return (
    <Stack
      direction="row"
      onClick={onOpen}
      sx={{
        alignItems: 'center',
        gap: 1.5,
        px: 1.75,
        py: 1.5,
        alignSelf: 'stretch',
        boxSizing: 'border-box',
        borderRadius: SHAPE.surface,
        cursor: 'pointer',
        bgcolor: 'background.paper',
        boxShadow: 'var(--card-shadow)',
        transition: (theme) =>
          theme.transitions.create(
            ['border-color', 'background-color', 'transform', 'box-shadow'],
            { duration: theme.transitions.duration.short },
          ),
        '@media (hover: hover)': {
          '&:hover': {
            transform: 'translateY(-2px)',
            boxShadow: 'var(--card-shadow-hover)',
          },
        },
        '@media (prefers-reduced-motion: reduce)': {
          '&:hover': { transform: 'none' },
        },
        border: (theme) =>
          `1px solid ${
            statusRow
              ? reason === 'expired'
                ? theme.palette.error.main
                : reason === 'traffic'
                  ? theme.palette.warning.main
                  : 'var(--card-line)'
              : 'var(--card-line)'
          }`,
        '&:hover': { borderColor: 'primary.main' },
      }}
    >
      {flagName ? <CountryFlag name={flagName} size={26} /> : null}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography noWrap sx={{ fontSize: 14, fontWeight: 700 }}>
          {current
            ? nameWithoutFlag(current)
            : statusRow
              ? t('home.components.serverStatus.noServers')
              : t('home.components.serverSelect.none')}
        </Typography>
        <Typography
          noWrap
          sx={{ fontSize: 12, display: 'block' }}
          color={statusRow ? statusColor : 'text.secondary'}
          title={statusRow ? undefined : description}
        >
          {statusCaption ?? caption}
        </Typography>
        {!statusRow && partiallyDropped && droppedTotal > 0 ? (
          <Typography
            noWrap
            sx={{ fontSize: 12, display: 'block', fontStyle: 'italic' }}
            color="text.secondary"
          >
            {t('home.components.serverStatus.row.partiallyDropped', {
              total: droppedTotal,
            })}
          </Typography>
        ) : null}
      </Box>

      {currentProfile?.disable_ping ? (
        <PingVerdict delay={shownDelay} />
      ) : currentProfile?.latency_style === 'dot' ? (
        <SignalDot delay={shownDelay} />
      ) : currentProfile?.latency_style === 'number' ? (
        <LatencyNumber delay={shownDelay} />
      ) : (
        <SignalBars delay={shownDelay} />
      )}

      <IconButton
        size="small"
        aria-label={t('home.components.serverSelect.title')}
      >
        <ExpandMoreRoundedIcon />
      </IconButton>
    </Stack>
  )
}
