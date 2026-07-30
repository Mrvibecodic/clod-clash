import BoltRoundedIcon from '@mui/icons-material/BoltRounded'
import CheckRoundedIcon from '@mui/icons-material/CheckRounded'
import ExpandMoreRoundedIcon from '@mui/icons-material/ExpandMoreRounded'
import StarBorderRoundedIcon from '@mui/icons-material/StarBorderRounded'
import StarRoundedIcon from '@mui/icons-material/StarRounded'
import {
  Box,
  Button,
  CircularProgress,
  Drawer,
  IconButton,
  List,
  ListItemButton,
  MenuItem,
  Select,
  Stack,
  Typography,
} from '@mui/material'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLockFn } from 'ahooks'
import { useCallback, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { delayGroup } from 'tauri-plugin-mihomo-api'

import { CountryFlag } from '@/components/home/country-flag'
import { useProfiles } from '@/hooks/use-profiles'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import delayManager from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import { nameWithoutFlag } from '@/utils/country'
import { delayColor } from '@/utils/delay-color'

interface ProxyNode {
  name: string
  type?: string
}

interface ProxyGroup {
  name: string
  type?: string
  now?: string
  all?: ProxyNode[]
}

const VIRTUALIZE_FROM = 50
const ROW_HEIGHT = 52

/** Entry types that are groups or built-ins, not actual servers. */
const NON_NODE_TYPES = new Set([
  'selector',
  'urltest',
  'fallback',
  'loadbalance',
  'relay',
  'direct',
  'reject',
  'rejectdrop',
  'pass',
  'compatible',
])

/**
 * Groups the user is meant to pick from — the ones the core lets us select.
 *
 * Panels often strip the template down to a flat `proxies:` list with no
 * custom groups at all (mode: global does the routing). The built-in GLOBAL
 * selector is the only way to pick a server there, so it serves as the
 * fallback — with the group entries filtered out, leaving actual servers.
 */
const selectableGroups = (proxies: any): ProxyGroup[] => {
  const groups = ((proxies?.groups ?? []) as ProxyGroup[]).filter(
    (group) =>
      group.type?.toLowerCase() === 'selector' && group.name !== 'GLOBAL',
  )
  if (groups.length > 0) return groups

  const global = proxies?.global as ProxyGroup | undefined
  const records = proxies?.records ?? {}
  const nodes = (global?.all ?? []).filter((node) => {
    const type = (records[node.name]?.type ?? node.type ?? '').toLowerCase()
    return !NON_NODE_TYPES.has(type)
  })
  return nodes.length > 0 ? [{ ...global, name: 'GLOBAL', all: nodes }] : []
}

interface Props {
  open: boolean
  onClose: () => void
}

/**
 * Full server list, opened over the home screen rather than as its own page:
 * picking a server is a detour from connecting, not a destination.
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

  const groups = useMemo(() => selectableGroups(proxies), [proxies])
  const [groupName, setGroupName] = useState<string>('')
  const group = useMemo(
    () => groups.find((item) => item.name === groupName) ?? groups[0],
    [groups, groupName],
  )

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
    if (!group) return
    await changeProxy(group.name, nodeName)
    onClose()
  })

  const runDelayTest = useCallback(async () => {
    if (!group) return
    setTesting(true)
    try {
      await delayGroup(group.name, delayManager.getUrl(group.name), 10000)
    } catch (error) {
      showNotice.error(error)
    } finally {
      setTesting(false)
      // The core has fresh delay history now; re-read the proxies so the
      // rows actually show it (the test itself emits no frontend event).
      refreshProxy().catch(() => {})
    }
  }, [group, refreshProxy])

  const renderRow = (node: ProxyNode) => {
    const delay = delayManager.getDelayFix(node as any, group?.name ?? '')
    const selected = group?.now === node.name
    const starred = favorites.has(node.name)

    return (
      <ListItemButton
        key={node.name}
        selected={selected}
        onClick={() => void select(node.name)}
        sx={{ borderRadius: 1, height: ROW_HEIGHT, gap: 1.25 }}
      >
        <CountryFlag name={node.name} />
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography noWrap>{nameWithoutFlag(node.name)}</Typography>
          {node.type ? (
            <Typography variant="caption" color="text.secondary">
              {node.type}
            </Typography>
          ) : null}
        </Box>
        <Typography
          variant="body2"
          sx={{ color: delayColor(delay), fontVariantNumeric: 'tabular-nums' }}
        >
          {delay > 0 ? `${delay} ms` : '—'}
        </Typography>
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
          {groups.length > 1 ? (
            <Select
              size="small"
              value={group?.name ?? ''}
              onChange={(event) => setGroupName(event.target.value)}
              IconComponent={ExpandMoreRoundedIcon}
              sx={{ minWidth: 160 }}
            >
              {groups.map((item) => (
                <MenuItem key={item.name} value={item.name}>
                  {item.name}
                </MenuItem>
              ))}
            </Select>
          ) : (
            <Typography variant="h6">
              {group?.name ?? t('home.components.serverSelect.title')}
            </Typography>
          )}

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
  const lit =
    delay === undefined || delay <= 0
      ? 0
      : delay < 120
        ? 4
        : delay < 250
          ? 3
          : delay < 500
            ? 2
            : 1
  const color = (index: number) =>
    index < lit
      ? lit >= 3
        ? 'success.main'
        : 'warning.main'
      : 'divider'
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

/** The compact row on the home screen: current server, latency, one tap. */
export const ServerSelectRow = ({ onOpen }: RowProps) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()

  const group = selectableGroups(proxies)[0]
  const current = group?.now
  const delay = current
    ? delayManager.getDelayFix({ name: current } as any, group?.name ?? '')
    : undefined

  return (
    <Stack
      direction="row"
      onClick={onOpen}
      sx={{
        alignItems: 'center',
        gap: 1.5,
        px: 1.75,
        py: 1.5,
        width: '100%',
        borderRadius: '14px',
        cursor: 'pointer',
        bgcolor: 'background.paper',
        border: (theme) => `1px solid ${theme.palette.divider}`,
        '&:hover': { borderColor: 'primary.main' },
      }}
    >
      {current ? <CountryFlag name={current} size={26} /> : null}
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
          {delay !== undefined && delay > 0
            ? `${delay} ${t('home.components.serverSelect.ms')}`
            : t('home.components.serverSelect.current')}
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
