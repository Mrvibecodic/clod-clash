import type {
  DraggableAttributes,
  DraggableSyntheticListeners,
} from '@dnd-kit/core'
import {
  CheckBoxOutlineBlankRounded,
  CheckBoxRounded,
  DragIndicatorRounded,
  HomeWorkRounded,
  RefreshRounded,
  ShieldRounded,
  SupportAgentRounded,
} from '@mui/icons-material'
import {
  alpha,
  Box,
  Button,
  Chip,
  CircularProgress,
  IconButton,
  keyframes,
  LinearProgress,
  Menu,
  MenuItem,
  Typography,
} from '@mui/material'
import { open } from '@tauri-apps/plugin-shell'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import {
  memo,
  useCallback,
  useEffect,
  useReducer,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog } from '@/components/base'
import { EditorViewer } from '@/components/profile/editor-viewer'
import { GroupsEditorViewer } from '@/components/profile/groups-editor-viewer'
import { RulesEditorViewer } from '@/components/profile/rules-editor-viewer'
import { useEditorDocument } from '@/hooks/use-editor-document'
import {
  getNextUpdateTime,
  readProfileFile,
  saveProfileFile,
  updateProfile,
  viewProfile,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useLoadingCache, useSetLoadingCache } from '@/services/states'
import type { TranslationKey } from '@/types/generated/i18n-keys'
import { debugLog } from '@/utils/debug'
import parseTraffic from '@/utils/parse-traffic'
import { clockSkew, toUnixSeconds } from '@/utils/subscription-status'

import { ProfileBox } from './profile-box'
import { ProxiesEditorViewer } from './proxies-editor-viewer'
import { QrViewer } from './qr-viewer'
const round = keyframes`
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
`

export interface ProfileItemProps {
  selected: boolean
  activating: boolean
  itemData: IProfileItem
  mutateProfiles: () => Promise<void>
  onSelect: (force: boolean) => void
  onEdit: () => void
  onSave?: (prev?: string, curr?: string) => void
  onDelete: () => void
  batchMode?: boolean
  isSelected?: boolean
  onSelectionChange?: () => void
  timerUpdateRevision: number
  completedUpdateRevision: number
  dragHandleRef: (node: HTMLElement | null) => void
  dragHandleAttributes: DraggableAttributes
  dragHandleListeners: DraggableSyntheticListeners
}

const ProfileItemBase = (props: ProfileItemProps) => {
  const {
    selected,
    activating,
    itemData,
    mutateProfiles,
    onSelect,
    onEdit,
    onSave,
    onDelete,
    batchMode,
    isSelected,
    onSelectionChange,
    timerUpdateRevision,
    completedUpdateRevision,
    dragHandleRef,
    dragHandleAttributes,
    dragHandleListeners,
  } = props

  const { t } = useTranslation()
  const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null)
  const [position, setPosition] = useState({ left: 0, top: 0 })
  const loadingCache = useLoadingCache()
  const setLoadingCache = useSetLoadingCache()

  // Новое состояние: показывать ли время следующего обновления
  const [showNextUpdate, setShowNextUpdate] = useState(false)
  const showNextUpdateRef = useRef(false)
  const [nextUpdateTime, setNextUpdateTime] = useState('')
  const refreshTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )
  const setLoading = useCallback(
    (loading: boolean) => {
      setLoadingCache((cache) => {
        const next = new Set(cache)
        if (loading) {
          next.add(itemData.uid)
        } else {
          next.delete(itemData.uid)
        }
        return next
      })
    },
    [itemData.uid, setLoadingCache],
  )

  const { uid, name = 'Profile', extra, updated = 0, option } = itemData
  const [mountedAt] = useState(() => Date.now())

  // Функция получения времени следующего обновления
  const fetchNextUpdateTimeCallback = useCallback(
    async (forceRefresh = false) => {
      if (
        itemData.option?.update_interval &&
        itemData.option.update_interval > 0
      ) {
        try {
          debugLog(
            `Попытка получить время следующего обновления для конфигурации ${itemData.uid}`,
          )

          // Если нужно принудительное обновление, сначала вызываем Timer.refresh()
          if (forceRefresh) {
            // Здесь можно было бы вызвать новый API для обновления, но пока
            // полагаемся на обновление внутри patch_profile
            debugLog(`Принудительное обновление задачи таймера`)
          }

          const nextUpdate = await getNextUpdateTime(itemData.uid)
          debugLog(
            `Результат получения времени следующего обновления:`,
            nextUpdate,
          )

          if (nextUpdate) {
            const nextUpdateDate = dayjs(nextUpdate * 1000)
            const now = dayjs()

            // Если время уже истекло, показываем "обновление не удалось"
            if (nextUpdateDate.isBefore(now)) {
              setNextUpdateTime(
                t('profiles.components.profileItem.status.lastUpdateFailed'),
              )
            } else {
              // Иначе показываем оставшееся время
              const diffMinutes = nextUpdateDate.diff(now, 'minute')

              if (diffMinutes < 60) {
                if (diffMinutes <= 0) {
                  setNextUpdateTime(
                    `${t('profiles.components.profileItem.status.nextUp')} <1m`,
                  )
                } else {
                  setNextUpdateTime(
                    `${t('profiles.components.profileItem.status.nextUp')} ${diffMinutes}m`,
                  )
                }
              } else {
                const hours = Math.floor(diffMinutes / 60)
                const mins = diffMinutes % 60
                setNextUpdateTime(
                  `${t('profiles.components.profileItem.status.nextUp')} ${hours}h ${mins}m`,
                )
              }
            }
          } else {
            debugLog(`Возвращено пустое время следующего обновления`)
            setNextUpdateTime(
              t('profiles.components.profileItem.status.noSchedule'),
            )
          }
        } catch (err) {
          console.error(
            `Ошибка при получении времени следующего обновления:`,
            err,
          )
          setNextUpdateTime(t('profiles.components.profileItem.status.unknown'))
        }
      } else {
        debugLog(
          `Для этой конфигурации не задан интервал обновления или он равен 0`,
        )
        setNextUpdateTime(
          t('profiles.components.profileItem.status.autoUpdateDisabled'),
        )
      }
    },
    [itemData.option?.update_interval, itemData.uid, t],
  )
  const fetchNextUpdateTime = useLockFn(fetchNextUpdateTimeCallback)

  // Функция переключения режима отображения
  const toggleUpdateTimeDisplay = (e: React.MouseEvent) => {
    e.stopPropagation()

    if (!showNextUpdate) {
      fetchNextUpdateTime()
    }

    setShowNextUpdate(!showNextUpdate)
  }

  useEffect(() => {
    showNextUpdateRef.current = showNextUpdate
  }, [showNextUpdate])

  // Обновляем время следующего обновления при загрузке компонента или изменении интервала
  useEffect(() => {
    if (showNextUpdate) {
      fetchNextUpdateTime()
    }
  }, [
    fetchNextUpdateTime,
    showNextUpdate,
    itemData.option?.update_interval,
    updated,
  ])

  // Страница подписана на общие события таймера, здесь реагируем только на
  // сигнал обновления текущего конфига
  useEffect(() => {
    if (timerUpdateRevision === 0 || !showNextUpdateRef.current) return

    if (refreshTimeoutRef.current !== undefined) {
      clearTimeout(refreshTimeoutRef.current)
    }
    refreshTimeoutRef.current = window.setTimeout(() => {
      fetchNextUpdateTime(true)
    }, 1000)

    return () => {
      if (refreshTimeoutRef.current !== undefined) {
        clearTimeout(refreshTimeoutRef.current)
      }
    }
  }, [fetchNextUpdateTime, timerUpdateRevision])

  useEffect(() => {
    if (completedUpdateRevision === 0 || !showNextUpdateRef.current) return
    fetchNextUpdateTime()
  }, [completedUpdateRevision, fetchNextUpdateTime])

  // local file mode
  // remote file mode
  // remote file mode
  const hasUrl = !!itemData.url
  const hasExtra = !!extra // only subscription url has extra info
  const hasHome = !!itemData.home // only subscription url has home page

  const { upload = 0, download = 0, total = 0 } = extra ?? {}
  const from = parseUrl(itemData.url)
  const description = itemData.desc
  // clod: Remnawave sends total=0 for an unmetered plan and expire=0 for one
  // that never ends, so both need their own label instead of "0 B" / "-".
  const unlimitedTraffic = total === 0
  const neverExpires = !extra?.expire
  // clod: срок с остатком дней — как в карточке подписки на главной. Часы
  // читаем раз на маунт: чистота рендера важнее секундной точности. Поправку
  // до часов панели берём ту же, что и там: иначе устройство с ушедшими часами
  // гасит карточку как истёкшую на сутки раньше срока.
  const expireSeconds = toUnixSeconds(extra?.expire ?? 0)
  const skew = clockSkew(itemData) ?? 0
  const daysLeft = neverExpires
    ? undefined
    : Math.max(
        0,
        Math.ceil((expireSeconds - (mountedAt / 1000 + skew)) / 86400),
      )
  const expire = neverExpires
    ? t('profiles.components.profileItem.labels.neverExpires')
    : daysLeft !== undefined
      ? t('profiles.components.profileItem.labels.expiresIn', {
          count: daysLeft,
          date: parseExpire(expireSeconds - skew),
        })
      : parseExpire(expireSeconds - skew)
  const refillDate = itemData.refill_date
    ? parseExpire(itemData.refill_date)
    : undefined
  const progress = Math.min(
    Math.round(((download + upload) * 100) / (total + 0.01)) + 1,
    100,
  )

  // clod: состояние карточки читается с одного взгляда — бейджем и цветом, а
  // не датой мелким шрифтом. Пороги те же, что у карточки подписки на главной.
  const trafficOut =
    !unlimitedTraffic && total > 0 && download + upload >= total
  const expired = daysLeft === 0 && !neverExpires
  const expiring = !expired && daysLeft !== undefined && daysLeft <= 3
  const badge = selected
    ? { key: 'active' as const, color: 'primary' as const }
    : expired
      ? { key: 'expired' as const, color: 'error' as const }
      : trafficOut
        ? { key: 'trafficOut' as const, color: 'error' as const }
        : expiring
          ? { key: 'expiring' as const, color: 'warning' as const }
          : undefined

  // clod: `hwid_state` пишется при каждом обновлении подписки; показываем
  // только состояния, из-за которых обновление не проходит.
  const hwidNotice =
    itemData.hwid_state === 'limit'
      ? ('hwidLimit' as const)
      : itemData.hwid_state === 'not_supported'
        ? ('hwidNotSupported' as const)
        : undefined

  const loading = loadingCache.has(itemData.uid)

  // interval update fromNow field
  const [, forceRefresh] = useReducer((value: number) => value + 1, 0)
  useEffect(() => {
    if (!hasUrl) return

    let timer: ReturnType<typeof setTimeout> | undefined

    const handler = () => {
      const now = Date.now()
      const lastUpdate = updated * 1000
      // Если прошло больше суток, не трогаем
      if (now - lastUpdate >= 24 * 36e5) return

      const wait = now - lastUpdate >= 36e5 ? 30e5 : 5e4

      timer = setTimeout(() => {
        forceRefresh()
        handler()
      }, wait)
    }

    handler()

    return () => {
      if (timer) {
        clearTimeout(timer)
        timer = undefined
      }
    }
  }, [forceRefresh, hasUrl, updated])

  const [fileOpen, setFileOpen] = useState(false)
  const [rulesOpen, setRulesOpen] = useState(false)
  const [proxiesOpen, setProxiesOpen] = useState(false)
  const [groupsOpen, setGroupsOpen] = useState(false)
  const [mergeOpen, setMergeOpen] = useState(false)
  const [scriptOpen, setScriptOpen] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [qrOpen, setQrOpen] = useState(false)

  const loadProfileDocument = useCallback(() => readProfileFile(uid), [uid])
  const loadMergeDocument = useCallback(
    () => readProfileFile(option?.merge ?? ''),
    [option?.merge],
  )
  const loadScriptDocument = useCallback(
    () => readProfileFile(option?.script ?? ''),
    [option?.script],
  )

  const profileDocument = useEditorDocument({
    open: fileOpen,
    load: loadProfileDocument,
  })
  const mergeDocument = useEditorDocument({
    open: mergeOpen,
    load: loadMergeDocument,
  })
  const scriptDocument = useEditorDocument({
    open: scriptOpen,
    load: loadScriptDocument,
  })

  const onOpenHome = () => {
    setAnchorEl(null)
    open(itemData.home ?? '')
  }

  const onEditInfo = () => {
    setAnchorEl(null)
    onEdit()
  }

  const onShareQrCode = () => {
    setAnchorEl(null)
    setQrOpen(true)
  }

  const onEditFile = () => {
    setAnchorEl(null)
    setFileOpen(true)
  }

  const onEditRules = () => {
    setAnchorEl(null)
    setRulesOpen(true)
  }

  const onEditProxies = () => {
    setAnchorEl(null)
    setProxiesOpen(true)
  }

  const onEditGroups = () => {
    setAnchorEl(null)
    setGroupsOpen(true)
  }

  const onEditMerge = () => {
    setAnchorEl(null)
    setMergeOpen(true)
  }

  const onEditScript = () => {
    setAnchorEl(null)
    setScriptOpen(true)
  }

  const onForceSelect = () => {
    setAnchorEl(null)
    onSelect(true)
  }

  const onOpenFile = useLockFn(async () => {
    setAnchorEl(null)
    try {
      await viewProfile(itemData.uid)
    } catch (err) {
      showNotice.error(err)
    }
  })

  /// 0 не использовать прокси
  /// 1 использовать прокси подписки
  /// 2 использовать хотя бы один прокси: по подписке, а если её нет — системный прокси по умолчанию
  const onUpdate = useLockFn(async (type: 0 | 1 | 2): Promise<void> => {
    setAnchorEl(null)
    setLoading(true)

    // Задаём начальные параметры обновления в зависимости от типа
    const option: Partial<IProfileOption> = {}
    if (type === 0) {
      option.with_proxy = false
      option.self_proxy = false
    } else if (type === 2) {
      if (itemData.option?.self_proxy) {
        option.with_proxy = false
        option.self_proxy = true
      } else {
        option.with_proxy = true
        option.self_proxy = false
      }
    }

    try {
      // Вызываем обновление на бэкенде (бэкенд сам обрабатывает откат)
      const payload = Object.keys(option).length > 0 ? option : undefined
      await updateProfile(itemData.uid, payload)

      // Обновление успешно, обновляем список
      void mutateProfiles()
    } catch {
      // Обновление полностью не удалось (включая попытку отката на бэкенде)
      // Ничего делать не нужно, бэкенд отправит ошибку через систему событий
    } finally {
      setLoading(false)
    }
  })

  type ContextMenuItem = {
    label: string
    handler: () => void
    disabled: boolean
  }

  const menuLabels: Record<string, TranslationKey> = {
    home: 'profiles.components.menu.home',
    select: 'profiles.components.menu.select',
    shareQrCode: 'profiles.components.menu.shareQrCode',
    editInfo: 'profiles.components.menu.editInfo',
    editFile: 'profiles.components.menu.editFile',
    editRules: 'profiles.components.menu.editRules',
    editProxies: 'profiles.components.menu.editProxies',
    editGroups: 'profiles.components.menu.editGroups',
    extendConfig: 'profiles.components.menu.extendConfig',
    extendScript: 'profiles.components.menu.extendScript',
    openFile: 'profiles.components.menu.openFile',
    update: 'profiles.components.menu.update',
    updateViaProxy: 'profiles.components.menu.updateViaProxy',
    delete: 'shared.actions.delete',
  } as const

  const urlModeMenu: ContextMenuItem[] = [
    ...(hasHome
      ? [
          {
            label: menuLabels.home,
            handler: onOpenHome,
            disabled: false,
          } satisfies ContextMenuItem,
        ]
      : []),
    {
      label: menuLabels.select,
      handler: onForceSelect,
      disabled: false,
    },
    {
      label: menuLabels.shareQrCode,
      handler: onShareQrCode,
      disabled: false,
    },
    {
      label: menuLabels.editInfo,
      handler: onEditInfo,
      disabled: false,
    },
    {
      label: menuLabels.editFile,
      handler: onEditFile,
      disabled: false,
    },
    {
      label: menuLabels.editRules,
      handler: onEditRules,
      disabled: !option?.rules,
    },
    {
      label: menuLabels.editProxies,
      handler: onEditProxies,
      disabled: !option?.proxies,
    },
    {
      label: menuLabels.editGroups,
      handler: onEditGroups,
      disabled: !option?.groups,
    },
    {
      label: menuLabels.extendConfig,
      handler: onEditMerge,
      disabled: !option?.merge,
    },
    {
      label: menuLabels.extendScript,
      handler: onEditScript,
      disabled: !option?.script,
    },
    {
      label: menuLabels.openFile,
      handler: onOpenFile,
      disabled: false,
    },
    {
      label: menuLabels.update,
      handler: () => onUpdate(0),
      disabled: false,
    },
    {
      label: menuLabels.updateViaProxy,
      handler: () => onUpdate(2),
      disabled: false,
    },
    {
      label: menuLabels.delete,
      handler: () => {
        setAnchorEl(null)
        if (batchMode) {
          // If in batch mode, just toggle selection instead of showing delete confirmation
          if (onSelectionChange) {
            onSelectionChange()
          }
        } else {
          setConfirmOpen(true)
        }
      },
      disabled: false,
    },
  ]
  const fileModeMenu: ContextMenuItem[] = [
    {
      label: menuLabels.select,
      handler: onForceSelect,
      disabled: false,
    },
    {
      label: menuLabels.editInfo,
      handler: onEditInfo,
      disabled: false,
    },
    {
      label: menuLabels.editFile,
      handler: onEditFile,
      disabled: false,
    },
    {
      label: menuLabels.editRules,
      handler: onEditRules,
      disabled: !option?.rules,
    },
    {
      label: menuLabels.editProxies,
      handler: onEditProxies,
      disabled: !option?.proxies,
    },
    {
      label: menuLabels.editGroups,
      handler: onEditGroups,
      disabled: !option?.groups,
    },
    {
      label: menuLabels.extendConfig,
      handler: onEditMerge,
      disabled: !option?.merge,
    },
    {
      label: menuLabels.extendScript,
      handler: onEditScript,
      disabled: !option?.script,
    },
    {
      label: menuLabels.openFile,
      handler: onOpenFile,
      disabled: false,
    },
    {
      label: menuLabels.delete,
      handler: () => {
        setAnchorEl(null)
        if (batchMode) {
          // If in batch mode, just toggle selection instead of showing delete confirmation
          if (onSelectionChange) {
            onSelectionChange()
          }
        } else {
          setConfirmOpen(true)
        }
      },
      disabled: false,
    },
  ]

  const handleSaveProfileDocument = useLockFn(async () => {
    const currentValue = profileDocument.value
    if (!(await saveProfileFile(uid, currentValue))) {
      await profileDocument.reload()
      return
    }
    onSave?.(profileDocument.savedValue, currentValue)
    profileDocument.markSaved(currentValue)
  })

  const handleSaveMergeDocument = useLockFn(async () => {
    const mergeUid = option?.merge ?? ''
    const currentValue = mergeDocument.value
    if (!(await saveProfileFile(mergeUid, currentValue))) {
      await mergeDocument.reload()
      return
    }
    onSave?.(mergeDocument.savedValue, currentValue)
    mergeDocument.markSaved(currentValue)
  })

  const handleSaveScriptDocument = useLockFn(async () => {
    const scriptUid = option?.script ?? ''
    const currentValue = scriptDocument.value
    if (!(await saveProfileFile(scriptUid, currentValue))) {
      await scriptDocument.reload()
      return
    }
    onSave?.(scriptDocument.savedValue, currentValue)
    scriptDocument.markSaved(currentValue)
  })

  return (
    <Box sx={{ position: 'relative' }}>
      <ProfileBox
        aria-selected={selected}
        dimmed={expired}
        onClick={(e) => {
          // Если уже идёт активация, блокируем повторный клик
          if (activating) {
            e.preventDefault()
            e.stopPropagation()
            return
          }
          onSelect(false)
        }}
        onContextMenu={(event) => {
          const { clientX, clientY } = event
          setPosition({ top: clientY, left: clientX })
          setAnchorEl(event.currentTarget as HTMLElement)
          event.preventDefault()
        }}
      >
        {activating && (
          <Box
            sx={{
              position: 'absolute',
              inset: 0,
              display: 'flex',
              justifyContent: 'center',
              alignItems: 'center',
              zIndex: 10,
              borderRadius: '10px',
              backdropFilter: 'blur(2px)',
              backgroundColor: (theme) =>
                alpha(theme.palette.background.paper, 0.4),
            }}
          >
            <CircularProgress color="inherit" size={22} />
          </Box>
        )}

        {/* clod:card-v2 — шапка карточки. Раньше бейдж состояния висел
            абсолютом в правом верхнем углу ПОВЕРХ кнопки обновления: у
            активной подписки (а бейдж «Активна» есть всегда) нажатие уходило
            в бейдж, и обновление молча не срабатывало. Теперь в верхнем ряду
            только имя и кнопка, а бейдж ушёл строкой ниже: втроём они делили
            одну строку и жались друг к другу на узкой карточке. */}
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            gap: 0.5,
            minWidth: 0,
            minHeight: 36,
          }}
        >
          {batchMode && (
            <IconButton
              size="small"
              sx={{ p: '2px', ml: '-6px', flexShrink: 0 }}
              onClick={(e) => {
                e.stopPropagation()
                if (onSelectionChange) {
                  onSelectionChange()
                }
              }}
            >
              {isSelected ? (
                <CheckBoxRounded color="primary" />
              ) : (
                <CheckBoxOutlineBlankRounded />
              )}
            </IconButton>
          )}
          <Box
            ref={dragHandleRef}
            sx={{ display: 'flex', flexShrink: 0, ml: '-6px' }}
            {...dragHandleAttributes}
            {...dragHandleListeners}
          >
            <DragIndicatorRounded
              sx={{ cursor: 'move', color: 'text.primary' }}
            />
          </Box>

          <Typography
            sx={{
              flex: 1,
              minWidth: 0,
              fontSize: '17px',
              fontWeight: 600,
              lineHeight: '26px',
            }}
            variant="h6"
            component="h2"
            noWrap
            title={name}
          >
            {name}
          </Typography>

          {/* clod:chan — защищённая подписка помечена значком: признак
              необратим, и видеть его надо не заходя в правку. */}
          {option?.secure && (
            <ShieldRounded
              titleAccess={t(
                'profiles.modals.profileForm.fields.secureChannel',
              )}
              sx={{ fontSize: 18, flexShrink: 0, color: 'success.main' }}
            />
          )}

          {/* only if has url can it be updated */}
          {hasUrl && (
            <IconButton
              title={t('shared.actions.refresh')}
              sx={{
                p: '4px',
                mr: '-6px',
                flexShrink: 0,
                animation: loading ? `1s linear infinite ${round}` : 'none',
              }}
              size="small"
              color="inherit"
              disabled={loading}
              onClick={(e) => {
                e.stopPropagation()
                // Если идёт активация или загрузка, блокируем обновление
                if (activating || loading) {
                  return
                }
                onUpdate(1)
              }}
            >
              <RefreshRounded color="inherit" />
            </IconButton>
          )}
        </Box>

        {/* clod:card-v2 — вторая строка: состояние и время обновления.
            Карточка узкая (в ряду их три-четыре), поэтому бейдж, адрес и
            время в одну строку не ставим: адрес уехал отдельной третьей
            строкой, а карточка выросла в высоту. */}
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            gap: 1,
            minWidth: 0,
            minHeight: 24,
          }}
        >
          {badge && (
            <Chip
              size="small"
              color={badge.color}
              variant="outlined"
              label={t(`profiles.components.profileItem.badges.${badge.key}`)}
              sx={{
                flexShrink: 0,
                height: 20,
                fontSize: 10.5,
                bgcolor: (theme) =>
                  alpha(theme.palette[badge.color].main, 0.14),
                '& .MuiChip-label': { px: 0.85 },
              }}
            />
          )}
          <Box sx={{ flex: 1, minWidth: 0 }} />
          {hasUrl && (
            <Typography
              noWrap
              component="span"
              title={
                showNextUpdate
                  ? t('profiles.components.profileItem.tooltips.showLast')
                  : `${t('shared.labels.updateTime')}: ${parseExpire(updated)}\n${t('profiles.components.profileItem.tooltips.showNext')}`
              }
              sx={{
                fontSize: 13,
                flexShrink: 0,
                cursor: 'pointer',
                borderBottom: '1px dashed transparent',
                transition: 'all 0.2s',
                '&:hover': {
                  borderBottomColor: 'primary.main',
                  color: 'primary.main',
                },
              }}
              onClick={toggleUpdateTimeDisplay}
            >
              {showNextUpdate
                ? nextUpdateTime
                : updated > 0
                  ? dayjs(updated * 1000).fromNow()
                  : ''}
            </Typography>
          )}
        </Box>

        {/* Третья строка: откуда подписка. Отдельной строкой ей хватает всей
            ширины карточки, а не остатка после бейджа и времени. */}
        <Box sx={{ minWidth: 0, minHeight: 20 }}>
          <Typography
            noWrap
            title={
              description ? description : `${t('shared.labels.from')} ${from}`
            }
            sx={{ fontSize: 13 }}
          >
            {/* clod:groups — ярлык группы идёт первым: по нему карточку
                и ищут глазами в сетке. */}
            {description
              ? description
              : hasUrl
                ? itemData.group
                  ? `${itemData.group} · ${from}`
                  : from
                : ''}
          </Typography>
        </Box>

        {/* Третья строка: трафик и срок, под ними полоса расхода. */}
        {hasExtra ? (
          <Box sx={{ mt: 1.25 }}>
            <Box
              sx={{
                display: 'flex',
                alignItems: 'baseline',
                justifyContent: 'space-between',
                gap: 1,
                fontSize: 13.5,
              }}
            >
              <Box
                component="span"
                title={t('shared.labels.usedTotal')}
                sx={{ whiteSpace: 'nowrap' }}
              >
                {parseTraffic(upload + download)} /{' '}
                {unlimitedTraffic
                  ? t('profiles.components.profileItem.labels.unlimited')
                  : parseTraffic(total)}
              </Box>
              <Box
                component="span"
                title={
                  refillDate
                    ? t('profiles.components.profileItem.tooltips.refillDate', {
                        date: refillDate,
                      })
                    : t('shared.labels.expireTime')
                }
                sx={{
                  whiteSpace: 'nowrap',
                  color: expired || expiring ? 'error.main' : undefined,
                }}
              >
                {expire}
              </Box>
            </Box>
            <LinearProgress
              variant="determinate"
              value={progress}
              color={trafficOut ? 'error' : 'primary'}
              sx={{
                mt: 0.75,
                height: 6,
                borderRadius: 3,
                opacity: total > 0 ? 1 : 0,
                bgcolor: (theme) => alpha(theme.palette.text.primary, 0.08),
                '& .MuiLinearProgress-bar': { borderRadius: 3 },
              }}
            />
          </Box>
        ) : (
          <Box
            sx={{
              mt: 1.25,
              display: 'flex',
              justifyContent: 'flex-end',
              fontSize: 12.5,
            }}
          >
            <span title={t('shared.labels.updateTime')}>
              {parseExpire(updated)}
            </span>
          </Box>
        )}
        {/* clod: состояние устройства (`x-hwid-*`) раньше жило только в
            профиле и в логах: диалог закрыли — и причина, по которой подписка
            не обновляется, пропадала. Теперь она видна на карточке.
            clod:card-v2 — без `noWrap`: строка длинная и на узкой карточке
            обрезалась ровно там, где начиналось объяснение. */}
        {hwidNotice && (
          <Typography
            title={t(`profiles.components.profileItem.status.${hwidNotice}`)}
            color="error"
            sx={{ fontSize: 12, mt: 1, lineHeight: 1.35 }}
          >
            {t(`profiles.components.profileItem.status.${hwidNotice}`)}
          </Typography>
        )}
        {/* clod: ссылки провайдера из заголовков подписки — у каждой
            подписки свои личный кабинет и поддержка */}
        {(itemData.portal_url || itemData.support_url) && (
          <Box sx={{ display: 'flex', gap: 1, mt: 1 }}>
            {itemData.portal_url && (
              <Button
                size="small"
                variant="outlined"
                startIcon={<HomeWorkRounded />}
                sx={{ flex: 1, minWidth: 0, whiteSpace: 'nowrap' }}
                onClick={(e) => {
                  e.stopPropagation()
                  open(itemData.portal_url ?? '')
                }}
              >
                {t('profiles.components.profileItem.actions.portal')}
              </Button>
            )}
            {itemData.support_url && (
              <Button
                size="small"
                variant="outlined"
                startIcon={<SupportAgentRounded />}
                sx={{ flex: 1, minWidth: 0, whiteSpace: 'nowrap' }}
                onClick={(e) => {
                  e.stopPropagation()
                  open(itemData.support_url ?? '')
                }}
              >
                {t('profiles.components.hwidDialog.support')}
              </Button>
            )}
          </Box>
        )}
      </ProfileBox>

      <Menu
        open={!!anchorEl}
        anchorEl={anchorEl}
        onClose={() => setAnchorEl(null)}
        anchorPosition={position}
        anchorReference="anchorPosition"
        transitionDuration={225}
        slotProps={{ list: { sx: { py: 0.5 } } }}
        onContextMenu={(e) => {
          setAnchorEl(null)
          e.preventDefault()
        }}
      >
        {(hasUrl ? urlModeMenu : fileModeMenu).map((item) => (
          <MenuItem
            key={item.label}
            onClick={item.handler}
            disabled={item.disabled}
            sx={[
              {
                minWidth: 120,
              },
              (theme) => {
                return {
                  color:
                    item.label === menuLabels.delete
                      ? theme.palette.error.main
                      : undefined,
                }
              },
            ]}
            dense
          >
            {t(item.label)}
          </MenuItem>
        ))}
      </Menu>
      {fileOpen && (
        <EditorViewer
          open={true}
          value={profileDocument.value}
          language="yaml"
          path={`profile:${uid}.yaml`}
          loading={profileDocument.loading}
          dirty={profileDocument.dirty}
          onChange={profileDocument.setValue}
          onSave={handleSaveProfileDocument}
          onClose={() => setFileOpen(false)}
        />
      )}
      {rulesOpen && (
        <RulesEditorViewer
          groupsUid={option?.groups ?? ''}
          mergeUid={option?.merge ?? ''}
          profileUid={uid}
          property={option?.rules ?? ''}
          open={true}
          onSave={onSave}
          onClose={() => setRulesOpen(false)}
        />
      )}
      {proxiesOpen && (
        <ProxiesEditorViewer
          profileUid={uid}
          property={option?.proxies ?? ''}
          open={true}
          onSave={onSave}
          onClose={() => setProxiesOpen(false)}
        />
      )}
      {groupsOpen && (
        <GroupsEditorViewer
          mergeUid={option?.merge ?? ''}
          proxiesUid={option?.proxies ?? ''}
          profileUid={uid}
          property={option?.groups ?? ''}
          open={true}
          onSave={onSave}
          onClose={() => {
            setGroupsOpen(false)
          }}
        />
      )}
      {mergeOpen && (
        <EditorViewer
          open={true}
          value={mergeDocument.value}
          language="yaml"
          path={`merge:${option?.merge ?? ''}.yaml`}
          loading={mergeDocument.loading}
          dirty={mergeDocument.dirty}
          onChange={mergeDocument.setValue}
          onSave={handleSaveMergeDocument}
          onClose={() => setMergeOpen(false)}
        />
      )}
      {scriptOpen && (
        <EditorViewer
          open={true}
          value={scriptDocument.value}
          language="javascript"
          path={`script:${option?.script ?? ''}.js`}
          loading={scriptDocument.loading}
          dirty={scriptDocument.dirty}
          onChange={scriptDocument.setValue}
          onSave={handleSaveScriptDocument}
          onClose={() => setScriptOpen(false)}
        />
      )}

      <BaseDialog
        title={t('profiles.modals.confirmDelete.title')}
        open={confirmOpen}
        okBtn={t('shared.actions.confirm')}
        cancelBtn={t('shared.actions.cancel')}
        contentSx={{ width: { xs: 320, sm: 420 }, userSelect: 'text' }}
        onCancel={() => setConfirmOpen(false)}
        onClose={() => setConfirmOpen(false)}
        onOk={() => {
          onDelete()
          setConfirmOpen(false)
        }}
      >
        <Typography variant="body2" sx={{ wordBreak: 'break-word' }}>
          {t('profiles.modals.confirmDelete.message')}
        </Typography>
      </BaseDialog>
      {qrOpen && itemData.url && (
        <QrViewer
          open={true}
          value={`${itemData.url}${itemData.url.includes('?') ? '&' : '?'}name=${encodeURIComponent(name)}`}
          onClose={() => setQrOpen(false)}
        />
      )}
    </Box>
  )
}

export const ProfileItem = memo(ProfileItemBase)

function parseUrl(url?: string) {
  if (!url) return ''
  const regex = /https?:\/\/(.+?)\//
  const result = url.match(regex)
  return result ? result[1] : 'local file'
}

function parseExpire(expire?: number) {
  if (!expire) return '-'
  return dayjs(expire * 1000).format('YYYY-MM-DD')
}
