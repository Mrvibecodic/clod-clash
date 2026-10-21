import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from '@dnd-kit/core'
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import {
  ArrowDownward,
  Delete as DeleteIcon,
  DragIndicator,
  Link,
  LinkOff,
  WarningRounded,
} from '@mui/icons-material'
import {
  Alert,
  Box,
  Button,
  Chip,
  IconButton,
  Paper,
  Typography,
  useTheme,
} from '@mui/material'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { BaseEmpty, TooltipIcon } from '@/components/base'
import { useProfiles } from '@/hooks/use-profiles'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import {
  connectProxyChain,
  disconnectProxyChain,
  dismantleProxyChain,
} from '@/services/cmds'
import delayManager from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import {
  chainIsConnected,
  clearProxyChain,
  type ConnectedChain,
  type ProxyChainNode as ProxyChainItem,
} from '@/services/proxy-chain-store'
import { revalidateQueriesByPrefix } from '@/services/query-client'
import { debugLog } from '@/utils/debug'
import { delayText, delayTone, failedDelay } from '@/utils/delay-color'
import { errorReason } from '@/utils/error-explanation'
import { nodeName, typeText } from '@/utils/proxy-label'

interface ProxyChainProps {
  proxyChain: ProxyChainItem[]
  onUpdateChain: (chain: ProxyChainItem[]) => void
  chain?: ConnectedChain | null
  mode?: string
  selectedGroup?: string | null
}

interface SortableItemProps {
  proxy: ProxyChainItem
  /**
   * Плашка протокола: подпись из подписки или тип от ядра; пусто — плашки
   * нет (провайдер спрятал).
   */
  label: string
  delay?: number
  index: number
  isFirst: boolean
  isLast: boolean
  pingBounds?: IProfileItem['ping_thresholds']
  onRemove: (id: string) => void
}

const toChainItems = (nodes: readonly string[]): ProxyChainItem[] => {
  const timestamp = Date.now()

  return nodes.map((name, index) => ({
    id: `${name}_${timestamp}_${index}`,
    name,
  }))
}

// Цифра строки — из тех же данных, что у всего экрана: свежее из своего замера
// и истории ядра, по адресу, которым меряет режим цепочки. В самой цепочке её
// не храним — копия отставала бы от ядра.
const chainDelay = (record?: IProxyItem) => {
  if (!record) return undefined
  const delay = delayManager.getDelayFix(record, 'chain-mode', true)
  return delay < 0 ? undefined : delay
}

const SortableItem = ({
  proxy,
  label,
  delay,
  index,
  isFirst,
  isLast,
  pingBounds,
  onRemove,
}: SortableItemProps) => {
  const theme = useTheme()
  const { t } = useTranslation()
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: proxy.id })

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  }

  const roleLabel = isFirst
    ? t('proxies.page.chain.entryNode')
    : isLast
      ? t('proxies.page.chain.exitNode')
      : undefined

  const roleColor = isFirst
    ? theme.palette.success.main
    : isLast
      ? theme.palette.warning.main
      : undefined

  return (
    <Box
      ref={setNodeRef}
      style={style}
      sx={{
        mb: 0,
        display: 'flex',
        alignItems: 'center',
        p: 1,
        backgroundColor: isDragging
          ? theme.palette.action.selected
          : theme.palette.background.default,
        borderRadius: 1,
        border: roleColor
          ? `1.5px solid ${roleColor}`
          : `1px solid ${theme.palette.divider}`,
        // clod:design-v3 — в покое строка плоская, тень появляется только
        // пока её тащат: так видно, что элемент оторван от списка.
        boxShadow: isDragging ? theme.shadows[8] : 'none',
        transition: theme.transitions.create(
          ['box-shadow', 'background-color', 'border-color'],
          { duration: theme.transitions.duration.short },
        ),
      }}
    >
      <Box
        {...attributes}
        {...listeners}
        sx={{
          display: 'flex',
          alignItems: 'center',
          mr: 1,
          color: theme.palette.text.secondary,
          cursor: 'grab',
          '&:active': {
            cursor: 'grabbing',
          },
        }}
      >
        <DragIndicator />
      </Box>

      {roleLabel ? (
        <Chip
          label={roleLabel}
          size="small"
          sx={{
            mr: 1,
            fontWeight: 700,
            color: '#fff',
            backgroundColor: roleColor,
          }}
        />
      ) : (
        <Chip
          label={`${index + 1}`}
          size="small"
          color="primary"
          sx={{ mr: 1, minWidth: 32 }}
        />
      )}

      <Typography
        variant="body2"
        sx={{
          flex: 1,
          fontWeight: 500,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {nodeName(proxy.name, t('shared.labels.direct'))}
      </Typography>

      {label && (
        <Chip label={label} size="small" variant="outlined" sx={{ mr: 1 }} />
      )}

      {delay !== undefined && (
        <Chip
          label={delayText(delay, t('shared.units.ms'))}
          title={failedDelay(delay) ? t('shared.labels.timeout') : undefined}
          size="small"
          color={delayTone(delay, pingBounds) ?? 'default'}
          sx={{ mr: 1, fontSize: '0.7rem', minWidth: 50 }}
        />
      )}

      <IconButton
        size="small"
        aria-label={t('proxies.page.actions.removeNode')}
        title={t('proxies.page.actions.removeNode')}
        onClick={() => onRemove(proxy.id)}
        sx={{
          color: theme.palette.error.main,
          '&:hover': {
            backgroundColor: theme.palette.error.light + '20',
          },
        }}
      >
        <DeleteIcon fontSize="small" />
      </IconButton>
    </Box>
  )
}

export const ProxyChain = ({
  proxyChain,
  onUpdateChain,
  chain,
  mode,
  selectedGroup,
}: ProxyChainProps) => {
  const theme = useTheme()
  const { t } = useTranslation()
  const chainWarning = t('proxies.page.chain.warning')
  const { proxies } = useProxiesData()
  const { current } = useProfiles()
  const profileUid = current?.uid
  const { refreshProxy } = useAppRefreshers()
  const [isConnecting, setIsConnecting] = useState(false)

  const targetGroup = mode === 'global' ? 'GLOBAL' : selectedGroup
  const isConnected = useMemo(
    () =>
      chainIsConnected(
        chain,
        proxyChain.map((item) => item.name),
        targetGroup,
      ),
    [proxyChain, targetGroup, chain],
  )

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  )

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event

      if (active.id !== over?.id) {
        const oldIndex = proxyChain.findIndex((item) => item.id === active.id)
        const newIndex = proxyChain.findIndex((item) => item.id === over?.id)

        onUpdateChain(arrayMove(proxyChain, oldIndex, newIndex))
      }
    },
    [proxyChain, onUpdateChain],
  )

  const handleRemoveProxy = useCallback(
    (id: string) => {
      const newChain = proxyChain.filter((item) => item.id !== id)
      // Список опустел — значит цепочки больше нет.
      if (newChain.length === 0) dismantleProxyChain(profileUid)
      onUpdateChain(newChain)
    },
    [proxyChain, onUpdateChain, profileUid],
  )

  const handleConnect = useCallback(async () => {
    if (isConnected) {
      setIsConnecting(true)
      try {
        await disconnectProxyChain()
        clearProxyChain(profileUid)
        await revalidateQueriesByPrefix(['proxyChain'])

        await closeAllConnections()
        await refreshProxy()

        onUpdateChain([])
      } catch (error) {
        console.error('Failed to disconnect from proxy chain:', error)
        showNotice.error('proxies.page.chain.disconnectFailed')
      } finally {
        setIsConnecting(false)
      }
      return
    }

    if (proxyChain.length < 2) {
      showNotice.error('proxies.page.chain.minimumNodes')
      return
    }

    setIsConnecting(true)
    try {
      if (!targetGroup) {
        throw new Error('Необходимо выбрать группу прокси в режиме правил')
      }

      const chainProxies = proxyChain.map((node) => node.name)
      debugLog('Connecting the chain:', targetGroup, chainProxies)
      await connectProxyChain(targetGroup, chainProxies)
      await revalidateQueriesByPrefix(['proxyChain'])

      // Обновляем данные прокси, чтобы обновить статус подключения
      refreshProxy()
      debugLog('Successfully connected to proxy chain')
    } catch (error) {
      console.error('Failed to connect to proxy chain:', error)
      showNotice.error('proxies.page.chain.connectFailed', {
        reason: errorReason(error, t),
      })
    } finally {
      setIsConnecting(false)
    }
  }, [
    proxyChain,
    isConnected,
    refreshProxy,
    targetGroup,
    onUpdateChain,
    profileUid,
    t,
  ])

  useEffect(() => {
    if (chain && chain.nodes.length > 0) {
      onUpdateChain(toChainItems(chain.nodes))
    }
  }, [chain, onUpdateChain])

  return (
    <Paper
      elevation={0}
      sx={{
        height: '100%',
        p: 2,
        display: 'flex',
        flexDirection: 'column',
      }}
    >
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          mb: 2,
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75 }}>
          <Typography variant="h6">{t('proxies.page.chain.header')}</Typography>
          <TooltipIcon
            title={chainWarning}
            icon={WarningRounded}
            color="warning"
            sx={{ p: 0.25 }}
          />
        </Box>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          {proxyChain.length > 0 && (
            <IconButton
              size="small"
              onClick={() => {
                dismantleProxyChain(profileUid)
                onUpdateChain([])
              }}
              sx={{
                color: theme.palette.error.main,
                '&:hover': {
                  backgroundColor: theme.palette.error.light + '20',
                },
              }}
              title={t('proxies.page.actions.clearChainConfig')}
            >
              <DeleteIcon fontSize="small" />
            </IconButton>
          )}
          <Button
            size="small"
            variant="contained"
            startIcon={isConnected ? <LinkOff /> : <Link />}
            onClick={handleConnect}
            disabled={
              isConnecting ||
              proxyChain.length < 2 ||
              (mode !== 'global' && !selectedGroup)
            }
            color={isConnected ? 'error' : 'success'}
            sx={{
              minWidth: 90,
            }}
            title={
              proxyChain.length < 2
                ? t('proxies.page.chain.minimumNodes')
                : undefined
            }
          >
            {isConnecting
              ? t('proxies.page.actions.connecting')
              : isConnected
                ? t('proxies.page.actions.disconnect')
                : t('proxies.page.actions.connect')}
          </Button>
        </Box>
      </Box>

      <Alert
        severity={proxyChain.length === 1 ? 'warning' : 'info'}
        sx={{ mb: 2 }}
      >
        {proxyChain.length === 1
          ? t('proxies.page.chain.minimumNodesHint')
          : t('proxies.page.chain.instruction')}
      </Alert>

      <Box sx={{ flex: 1, overflow: 'auto' }}>
        {proxyChain.length === 0 ? (
          <BaseEmpty />
        ) : (
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <SortableContext
              items={proxyChain.map((proxy) => proxy.id)}
              strategy={verticalListSortingStrategy}
            >
              <Box
                sx={{
                  borderRadius: 1,
                  minHeight: 60,
                  p: 1,
                }}
              >
                {proxyChain.map((proxy, index) => (
                  <Box key={proxy.id}>
                    <SortableItem
                      proxy={proxy}
                      label={typeText(
                        {
                          type:
                            proxy.type ?? proxies?.records?.[proxy.name]?.type,
                          label: proxies?.records?.[proxy.name]?.label,
                          all: proxies?.records?.[proxy.name]?.all,
                        },
                        current?.hide_badges,
                      )}
                      delay={chainDelay(proxies?.records?.[proxy.name])}
                      index={index}
                      isFirst={index === 0}
                      isLast={
                        index === proxyChain.length - 1 && proxyChain.length > 1
                      }
                      pingBounds={current?.ping_thresholds}
                      onRemove={handleRemoveProxy}
                    />
                    {index < proxyChain.length - 1 && (
                      <Box
                        sx={{
                          display: 'flex',
                          justifyContent: 'center',
                          py: 0.25,
                        }}
                      >
                        <ArrowDownward
                          sx={{
                            fontSize: 20,
                            color: theme.palette.primary.main,
                            opacity: 0.7,
                          }}
                        />
                      </Box>
                    )}
                  </Box>
                ))}
              </Box>
            </SortableContext>
          </DndContext>
        )}
      </Box>
    </Paper>
  )
}
