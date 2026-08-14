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
import yaml from 'js-yaml'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  closeAllConnections,
  selectNodeForGroup,
} from 'tauri-plugin-mihomo-api'

import { TooltipIcon } from '@/components/base'
import { useVisibility } from '@/hooks/use-visibility'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import {
  patchSelectedNode,
  updateProxyChainConfigInRuntime,
} from '@/services/cmds'
import { debugLog } from '@/utils/debug'

interface ProxyChainItem {
  id: string
  name: string
  type?: string
  delay?: number
}

interface ParsedChainConfig {
  proxies?: Array<{
    name: string
    type: string
    [key: string]: any
  }>
}

interface ProxyChainProps {
  proxyChain: ProxyChainItem[]
  onUpdateChain: (chain: ProxyChainItem[]) => void
  chainConfigData?: string | null
  onMarkUnsavedChanges?: () => void
  mode?: string
  selectedGroup?: string | null
}

interface SortableItemProps {
  proxy: ProxyChainItem
  index: number
  isFirst: boolean
  isLast: boolean
  onRemove: (id: string) => void
}

const toChainItems = (
  parsedConfig: ParsedChainConfig | null | undefined,
): ProxyChainItem[] => {
  const timestamp = Date.now()

  return (
    parsedConfig?.proxies?.map((proxy, index) => ({
      id: `${proxy.name}_${timestamp}_${index}`,
      name: proxy.name,
      type: proxy.type,
      delay: undefined,
    })) || []
  )
}

const SortableItem = ({
  proxy,
  index,
  isFirst,
  isLast,
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
        {proxy.name}
      </Typography>

      {proxy.type && (
        <Chip
          label={proxy.type}
          size="small"
          variant="outlined"
          sx={{ mr: 1 }}
        />
      )}

      {proxy.delay !== undefined && (
        <Chip
          label={
            proxy.delay > 0 ? `${proxy.delay}ms` : t('shared.labels.timeout')
          }
          size="small"
          color={
            proxy.delay > 0 && proxy.delay < 200
              ? 'success'
              : proxy.delay > 0 && proxy.delay < 800
                ? 'warning'
                : 'error'
          }
          sx={{ mr: 1, fontSize: '0.7rem', minWidth: 50 }}
        />
      )}

      <IconButton
        size="small"
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
  chainConfigData,
  onMarkUnsavedChanges,
  mode,
  selectedGroup,
}: ProxyChainProps) => {
  const theme = useTheme()
  const { t } = useTranslation()
  const chainWarning = t('proxies.page.chain.warning')
  const { proxies } = useProxiesData()
  const { refreshProxy } = useAppRefreshers()
  const pageVisible = useVisibility()
  const [isConnecting, setIsConnecting] = useState(false)
  const markUnsavedChanges = useCallback(() => {
    onMarkUnsavedChanges?.()
  }, [onMarkUnsavedChanges])

  const isConnected = useMemo(() => {
    if (!proxies || proxyChain.length < 2) {
      return false
    }

    const lastNode = proxyChain[proxyChain.length - 1]

    if (mode === 'global') {
      return proxies.global?.now === lastNode.name
    }

    if (!selectedGroup || !Array.isArray(proxies.groups)) {
      return false
    }

    const proxyChainGroup = proxies.groups.find(
      (group: { name: string }) => group.name === selectedGroup,
    )

    return proxyChainGroup?.now === lastNode.name
  }, [proxies, proxyChain, mode, selectedGroup])

  // Отслеживаем изменения цепочки, но исключаем случай загрузки из конфига
  const chainLengthRef = useRef(proxyChain.length)
  useEffect(() => {
    // Помечаем как несохранённое, только если длина цепочки изменилась и это не начальная загрузка
    if (
      chainLengthRef.current !== proxyChain.length &&
      chainLengthRef.current !== 0
    ) {
      markUnsavedChanges()
    }
    chainLengthRef.current = proxyChain.length
  }, [proxyChain.length, markUnsavedChanges])

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
        markUnsavedChanges()
      }
    },
    [proxyChain, onUpdateChain, markUnsavedChanges],
  )

  const handleRemoveProxy = useCallback(
    (id: string) => {
      const newChain = proxyChain.filter((item) => item.id !== id)
      onUpdateChain(newChain)
      markUnsavedChanges()
    },
    [proxyChain, onUpdateChain, markUnsavedChanges],
  )

  const handleConnect = useCallback(async () => {
    if (isConnected) {
      setIsConnecting(true)
      try {
        await updateProxyChainConfigInRuntime(null)

        const targetGroup =
          mode === 'global'
            ? 'GLOBAL'
            : selectedGroup || localStorage.getItem('proxy-chain-group')

        if (targetGroup) {
          // clod: то же и при разрыве цепочки — иначе подписка помнила бы
          // выходной узел, которого уже нет, и возвращала его после рестарта.
          const persist = (node: string) =>
            patchSelectedNode(targetGroup, node).catch((error) => {
              console.error('Failed to persist proxy chain selection:', error)
            })
          try {
            await selectNodeForGroup(targetGroup, 'DIRECT')
            await persist('DIRECT')
          } catch {
            if (proxyChain.length >= 1) {
              try {
                await selectNodeForGroup(targetGroup, proxyChain[0].name)
                await persist(proxyChain[0].name)
              } catch {
                // ignore
              }
            }
          }
        }

        localStorage.removeItem('proxy-chain-group')
        localStorage.removeItem('proxy-chain-exit-node')
        localStorage.removeItem('proxy-chain-items')

        await closeAllConnections()
        await refreshProxy()

        onUpdateChain([])
      } catch (error) {
        console.error('Failed to disconnect from proxy chain:', error)
        alert(t('proxies.page.chain.disconnectFailed'))
      } finally {
        setIsConnecting(false)
      }
      return
    }

    if (proxyChain.length < 2) {
      alert(t('proxies.page.chain.minimumNodes'))
      return
    }

    setIsConnecting(true)
    try {
      // Шаг 1: сохраняем конфиг цепочки прокси
      const chainProxies = proxyChain.map((node) => node.name)
      debugLog('Saving chain config:', chainProxies)
      await updateProxyChainConfigInRuntime(chainProxies)
      debugLog('Chain configuration saved successfully')

      // Шаг 2: подключаемся к последнему узлу цепочки прокси
      const lastNode = proxyChain[proxyChain.length - 1]
      debugLog(`Connecting to proxy chain, last node: ${lastNode.name}`)

      // Определяем имя группы прокси в зависимости от режима
      if (mode !== 'global' && !selectedGroup) {
        throw new Error('Необходимо выбрать группу прокси в режиме правил')
      }

      const targetGroup = mode === 'global' ? 'GLOBAL' : selectedGroup

      await selectNodeForGroup(targetGroup || 'GLOBAL', lastNode.name)
      // clod: выбор, записанный только в ядро, откатывался при его
      // перезапуске — цепочка «отваливалась» сама собой. Пишем и в подписку.
      await patchSelectedNode(targetGroup || 'GLOBAL', lastNode.name).catch(
        (error) => {
          console.error('Failed to persist proxy chain selection:', error)
        },
      )
      localStorage.setItem('proxy-chain-group', targetGroup || 'GLOBAL')
      localStorage.setItem('proxy-chain-exit-node', lastNode.name)

      // Обновляем данные прокси, чтобы обновить статус подключения
      refreshProxy()
      debugLog('Successfully connected to proxy chain')
    } catch (error) {
      console.error('Failed to connect to proxy chain:', error)
      alert(t('proxies.page.chain.connectFailed'))
    } finally {
      setIsConnecting(false)
    }
  }, [
    proxyChain,
    isConnected,
    t,
    refreshProxy,
    mode,
    selectedGroup,
    onUpdateChain,
  ])

  const proxyChainRef = useRef(proxyChain)
  const onUpdateChainRef = useRef(onUpdateChain)

  useEffect(() => {
    proxyChainRef.current = proxyChain
    onUpdateChainRef.current = onUpdateChain
  }, [proxyChain, onUpdateChain])

  // Обрабатываем данные конфига цепочки прокси
  useEffect(() => {
    if (chainConfigData) {
      try {
        // JSON is valid YAML, so one parser covers both persisted formats.
        const parsedConfig = yaml.load(chainConfigData) as ParsedChainConfig
        const chainItems = toChainItems(parsedConfig)

        if (chainItems.length > 0) {
          onUpdateChain(chainItems)
        }
      } catch (error) {
        console.error('Failed to process chain config data:', error)
      }
    }
  }, [chainConfigData, onUpdateChain])

  // Периодически обновляем данные о задержке
  useEffect(() => {
    if (!proxies?.records) return

    const updateDelays = () => {
      const currentChain = proxyChainRef.current
      if (currentChain.length === 0) return

      const updatedChain = currentChain.map((item) => {
        const proxyRecord = proxies.records[item.name]
        if (
          proxyRecord &&
          proxyRecord.history &&
          proxyRecord.history.length > 0
        ) {
          const latestDelay =
            proxyRecord.history[proxyRecord.history.length - 1].delay
          return { ...item, delay: latestDelay }
        }
        return item
      })

      // Обновляем, только если данные о задержке действительно изменились
      const hasChanged = updatedChain.some(
        (item, index) => item.delay !== currentChain[index]?.delay,
      )

      if (hasChanged) {
        onUpdateChainRef.current(updatedChain)
      }
    }

    // Сразу обновляем задержку один раз
    updateDelays()

    // clod: за окном в трее задержки перебирать некому — таймер там не нужен.
    // Показали окно снова — сработает проход выше, и цифры сойдутся сразу.
    if (!pageVisible) return

    // Устанавливаем таймер, обновляем задержку раз в 5 секунд
    const interval = setInterval(updateDelays, 5000)

    return () => clearInterval(interval)
  }, [proxies?.records, pageVisible]) // Зависим только от proxies.records

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
                updateProxyChainConfigInRuntime(null)
                localStorage.removeItem('proxy-chain-group')
                localStorage.removeItem('proxy-chain-exit-node')
                localStorage.removeItem('proxy-chain-items')
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
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              height: '100%',
              color: theme.palette.text.secondary,
            }}
          >
            <Typography>{t('proxies.page.chain.empty')}</Typography>
          </Box>
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
                      index={index}
                      isFirst={index === 0}
                      isLast={
                        index === proxyChain.length - 1 && proxyChain.length > 1
                      }
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
