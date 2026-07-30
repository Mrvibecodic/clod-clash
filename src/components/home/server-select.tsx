import BoltRoundedIcon from '@mui/icons-material/BoltRounded'
import CheckRoundedIcon from '@mui/icons-material/CheckRounded'
import ExpandMoreRoundedIcon from '@mui/icons-material/ExpandMoreRounded'
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
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useProxiesData } from '@/providers/app-data-context'
import delayManager from '@/services/delay'
import { showNotice } from '@/services/notice-service'
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

/** Groups the user is meant to pick from — the ones the core lets us select. */
const selectableGroups = (proxies: any): ProxyGroup[] =>
  ((proxies?.groups ?? []) as ProxyGroup[]).filter(
    (group) =>
      group.type?.toLowerCase() === 'selector' && group.name !== 'GLOBAL',
  )

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
  const { changeProxy } = useProxySelection()
  const [testing, setTesting] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)

  const groups = useMemo(() => selectableGroups(proxies), [proxies])
  const [groupName, setGroupName] = useState<string>('')
  const group = useMemo(
    () => groups.find((item) => item.name === groupName) ?? groups[0],
    [groups, groupName],
  )
  const nodes = group?.all ?? []

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
    }
  }, [group])

  const renderRow = (node: ProxyNode) => {
    const delay = delayManager.getDelayFix(node as any, group?.name ?? '')
    const selected = group?.now === node.name

    return (
      <ListItemButton
        key={node.name}
        selected={selected}
        onClick={() => void select(node.name)}
        sx={{ borderRadius: 1, height: ROW_HEIGHT, gap: 1.25 }}
      >
        <CountryFlag name={node.name} />
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography noWrap>{node.name}</Typography>
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
        {selected ? (
          <CheckRoundedIcon color="success" sx={{ ml: 1 }} fontSize="small" />
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
        gap: 1,
        px: 2,
        py: 1.25,
        width: '100%',
        borderRadius: 2,
        cursor: 'pointer',
        border: (theme) => `1px solid ${theme.palette.divider}`,
        '&:hover': { borderColor: 'primary.main' },
      }}
    >
      {current ? <CountryFlag name={current} size={26} /> : null}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="caption" color="text.secondary">
          {t('home.components.serverSelect.current')}
        </Typography>
        <Typography noWrap>
          {current ?? t('home.components.serverSelect.none')}
        </Typography>
      </Box>

      {delay !== undefined && delay > 0 ? (
        <Typography
          variant="body2"
          sx={{ color: delayColor(delay), fontVariantNumeric: 'tabular-nums' }}
        >
          {delay} ms
        </Typography>
      ) : null}

      <IconButton
        size="small"
        aria-label={t('home.components.serverSelect.title')}
      >
        <ExpandMoreRoundedIcon />
      </IconButton>
    </Stack>
  )
}
