import type {
  DraggableAttributes,
  DraggableSyntheticListeners,
} from '@dnd-kit/core'
import {
  CheckBoxOutlineBlankRounded,
  CheckBoxRounded,
  DragIndicatorRounded,
  ExpandLessRounded,
  ExpandMoreRounded,
  HomeWorkRounded,
  OpenInNewRounded,
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
  Divider,
  IconButton,
  keyframes,
  LinearProgress,
  Menu,
  MenuItem,
  type PopoverActions,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { memo, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog } from '@/components/base'
import { EditorViewer } from '@/components/profile/editor-viewer'
import { GroupsEditorViewer } from '@/components/profile/groups-editor-viewer'
import { RulesEditorViewer } from '@/components/profile/rules-editor-viewer'
import { useEditorDocument } from '@/hooks/use-editor-document'
import { openProviderLink } from '@/hooks/use-provider-links'
import {
  updateSubscriptionNow,
  useSubscriptionPaused,
  useSubscriptionUpdating,
} from '@/hooks/use-subscription-update'
import { useVisibility } from '@/hooks/use-visibility'
import {
  getNextUpdateTime,
  readProfileFile,
  saveProfileFile,
  viewProfile,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import type { TranslationKey } from '@/types/generated/i18n-keys'
import { profileDate } from '@/utils/date-text'
import { debugLog } from '@/utils/debug'
import parseTraffic from '@/utils/parse-traffic'
import { pressable } from '@/utils/pressable'
import { profileDisplayName } from '@/utils/profile-name'
import { clockSkew, missedUpdates } from '@/utils/subscription-status'

import { ProfileBox } from './profile-box'
import { ProxiesEditorViewer } from './proxies-editor-viewer'
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
  onDelete: () => void
  batchMode?: boolean
  isSelected?: boolean
  onSelectionChange?: () => void
  timerUpdateRevision: number
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
    onDelete,
    batchMode,
    isSelected,
    onSelectionChange,
    timerUpdateRevision,
    dragHandleRef,
    dragHandleAttributes,
    dragHandleListeners,
  } = props

  const { t } = useTranslation()
  const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null)
  const [position, setPosition] = useState({ left: 0, top: 0 })
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const menuActionsRef = useRef<PopoverActions>(null)

  const [showNextUpdate, setShowNextUpdate] = useState(false)
  const showNextUpdateRef = useRef(false)
  // Момент следующего обновления со слов бэкенда: null — задачи нет,
  // 'error' — не узнали; строка собирается при отрисовке от `now`.
  const [nextUpdateAt, setNextUpdateAt] = useState<number | null | 'error'>()
  const refreshTimeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )

  const { uid, extra, updated = 0, option } = itemData
  const name =
    profileDisplayName(itemData) ??
    t('profiles.components.profileItem.untitled')
  const autoUpdateOff = option?.allow_auto_update === false

  // «Сейчас» экрана: дни до конца, «обновлено … назад» и следующее обновление
  // не застывают, пока экран открыт.
  const [now, setNow] = useState(() => Date.now())
  const pageVisible = useVisibility()
  useEffect(() => {
    if (!pageVisible) return
    const tick = () => setNow(Date.now())
    const returned = setTimeout(tick, 0)
    const timer = setInterval(tick, 60_000)
    return () => {
      clearTimeout(returned)
      clearInterval(timer)
    }
  }, [pageVisible])

  const fetchNextUpdateTimeCallback = useCallback(async () => {
    try {
      const nextUpdate = await getNextUpdateTime(itemData.uid)
      debugLog(`Следующее обновление ${itemData.uid}:`, nextUpdate)
      setNextUpdateAt(nextUpdate ?? null)
    } catch (err) {
      console.error(`Ошибка при получении времени следующего обновления:`, err)
      setNextUpdateAt('error')
    }
  }, [itemData.uid])
  const fetchNextUpdateTime = useLockFn(fetchNextUpdateTimeCallback)

  const nextUpdateText = (() => {
    if (autoUpdateOff) {
      return t('profiles.components.profileItem.status.autoUpdateDisabled')
    }
    if (nextUpdateAt === undefined) return ''
    if (nextUpdateAt === 'error') {
      return t('profiles.components.profileItem.status.unknown')
    }
    if (nextUpdateAt === null) {
      return t('profiles.components.profileItem.status.noSchedule')
    }
    const minutes = Math.floor((nextUpdateAt * 1000 - now) / 60_000)
    if (minutes < 1)
      return t('profiles.components.profileItem.status.nextUpSoon')
    return minutes < 60
      ? t('profiles.components.profileItem.status.nextUpMinutes', { minutes })
      : t('profiles.components.profileItem.status.nextUpHours', {
          hours: Math.floor(minutes / 60),
          minutes: minutes % 60,
        })
  })()

  const toggleUpdateTimeDisplay = () => {
    if (!showNextUpdate) {
      fetchNextUpdateTime()
    }

    setShowNextUpdate(!showNextUpdate)
  }

  useEffect(() => {
    showNextUpdateRef.current = showNextUpdate
  }, [showNextUpdate])

  useEffect(() => {
    if (showNextUpdate) {
      fetchNextUpdateTime()
    }
  }, [
    fetchNextUpdateTime,
    showNextUpdate,
    option?.update_interval,
    option?.allow_auto_update,
    updated,
  ])

  useEffect(() => {
    if (timerUpdateRevision === 0 || !showNextUpdateRef.current) return

    if (refreshTimeoutRef.current !== undefined) {
      clearTimeout(refreshTimeoutRef.current)
    }
    refreshTimeoutRef.current = window.setTimeout(() => {
      fetchNextUpdateTime()
    }, 1000)

    return () => {
      if (refreshTimeoutRef.current !== undefined) {
        clearTimeout(refreshTimeoutRef.current)
      }
    }
  }, [fetchNextUpdateTime, timerUpdateRevision])

  const hasUrl = !!itemData.url
  const hasExtra = !!extra
  const hasHome = !!itemData.home

  const { upload = 0, download = 0, total = 0 } = extra ?? {}
  const from =
    parseUrl(itemData.url) ?? t('profiles.components.profileItem.localFile')
  const description = itemData.desc
  const unlimitedTraffic = total === 0
  const neverExpires = !extra?.expire
  const expireSeconds = extra?.expire ?? 0
  const skew = clockSkew(itemData) ?? 0
  const daysLeft = neverExpires
    ? undefined
    : Math.max(0, Math.ceil((expireSeconds - (now / 1000 + skew)) / 86400))
  const expire = neverExpires
    ? t('profiles.components.profileItem.labels.neverExpires')
    : daysLeft !== undefined
      ? t('profiles.components.profileItem.labels.expiresIn', {
          count: daysLeft,
          date: profileDate(expireSeconds - skew),
        })
      : profileDate(expireSeconds - skew)
  const refillDate = itemData.refill_date
    ? profileDate(itemData.refill_date)
    : undefined
  const progress = Math.min(
    Math.round(((download + upload) * 100) / (total + 0.01)) + 1,
    100,
  )

  const trafficOut =
    !unlimitedTraffic && total > 0 && download + upload >= total
  const expired = daysLeft === 0 && !neverExpires
  const expiring = !expired && daysLeft !== undefined && daysLeft <= 3
  // Выбранность показывает рамка карточки, плашка — только состояние подписки.
  const badge = expired
    ? { key: 'expired' as const, color: 'error' as const }
    : trafficOut
      ? { key: 'trafficOut' as const, color: 'error' as const }
      : expiring
        ? { key: 'expiring' as const, color: 'warning' as const }
        : undefined

  const hwidNotice =
    itemData.hwid_state === 'limit'
      ? ('hwidLimit' as const)
      : itemData.hwid_state === 'not_supported'
        ? ('hwidNotSupported' as const)
        : undefined

  const loading = useSubscriptionUpdating(itemData.uid)
  // Пауза после ручного обновления — та же, что у кнопок Главной
  const paused = useSubscriptionPaused(itemData.uid)
  const missed = missedUpdates(itemData)
  const fetchedAgo = updated > 0 ? dayjs(updated * 1000).from(now) : ''

  const [fileOpen, setFileOpen] = useState(false)
  const [rulesOpen, setRulesOpen] = useState(false)
  const [proxiesOpen, setProxiesOpen] = useState(false)
  const [groupsOpen, setGroupsOpen] = useState(false)
  const [mergeOpen, setMergeOpen] = useState(false)
  const [scriptOpen, setScriptOpen] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)

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
    void openProviderLink(itemData.home ?? '')
  }

  const onEditInfo = () => {
    setAnchorEl(null)
    onEdit()
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

  // «Через прокси» — первым маршрутом своё ядро; без него — маршрут из
  // настроек подписки, как у кнопки на карточке.
  const onUpdate = useLockFn(async (viaProxy: boolean): Promise<void> => {
    setAnchorEl(null)
    try {
      const option = viaProxy
        ? { with_proxy: false, self_proxy: true }
        : undefined
      if (await updateSubscriptionNow(itemData.uid, option)) {
        void mutateProfiles()
      }
    } catch (error) {
      // clod:Э10-01 — раньше ошибка глоталась молча: человек жал «Обновить»,
      // спиннер гас, и он не узнавал ни о провале, ни о причине. Бэкенд на ручном
      // вызове уведомление не шлёт намеренно — показываем здесь, чтобы не было двух.
      showNotice.error(error)
      void mutateProfiles()
    }
  })

  type ContextMenuItem = {
    label: TranslationKey
    handler: () => void
    disabled: boolean
  }

  const menuLabels = {
    subscriptionPage: 'profiles.components.menu.subscriptionPage',
    select: 'profiles.components.menu.select',
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
    advanced: 'profiles.components.menu.advanced',
    delete: 'shared.actions.delete',
  } as const satisfies Record<string, TranslationKey>

  const selectItem: ContextMenuItem = {
    label: menuLabels.select,
    handler: onForceSelect,
    disabled: false,
  }

  const mainMenu: ContextMenuItem[] = hasUrl
    ? [
        ...(hasHome
          ? [
              {
                label: menuLabels.subscriptionPage,
                handler: onOpenHome,
                disabled: false,
              } satisfies ContextMenuItem,
            ]
          : []),
        selectItem,
        {
          label: menuLabels.update,
          handler: () => onUpdate(false),
          disabled: loading || paused,
        },
        {
          label: menuLabels.updateViaProxy,
          handler: () => onUpdate(true),
          disabled: loading || paused,
        },
      ]
    : [selectItem]

  const advancedMenu: ContextMenuItem[] = [
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
        setConfirmOpen(true)
      },
      disabled: false,
    },
  ]

  useEffect(() => {
    menuActionsRef.current?.updatePosition()
  }, [advancedOpen])

  const pressCard = () => {
    if (batchMode) onSelectionChange?.()
    else onSelect(false)
  }

  const renderMenuItem = (item: ContextMenuItem, nested = false) => (
    <MenuItem
      key={item.label}
      onClick={item.handler}
      disabled={item.disabled}
      sx={(theme) => ({
        minWidth: 120,
        gap: 1,
        pl: nested ? 3.5 : 2,
        color:
          item.label === menuLabels.delete
            ? theme.palette.error.main
            : undefined,
      })}
      dense
    >
      {t(item.label)}
      {item.label === menuLabels.subscriptionPage && (
        <OpenInNewRounded sx={{ fontSize: 16, color: 'text.secondary' }} />
      )}
    </MenuItem>
  )

  const handleSaveProfileDocument = useLockFn(async () => {
    const currentValue = profileDocument.value
    if (!(await saveProfileFile(uid, currentValue))) return false
    profileDocument.markSaved(currentValue)
  })

  const handleSaveMergeDocument = useLockFn(async () => {
    const mergeUid = option?.merge ?? ''
    const currentValue = mergeDocument.value
    if (!(await saveProfileFile(mergeUid, currentValue))) return false
    mergeDocument.markSaved(currentValue)
  })

  const handleSaveScriptDocument = useLockFn(async () => {
    const scriptUid = option?.script ?? ''
    const currentValue = scriptDocument.value
    if (!(await saveProfileFile(scriptUid, currentValue))) return false
    scriptDocument.markSaved(currentValue)
  })

  return (
    <Box sx={{ position: 'relative' }}>
      <ProfileBox
        aria-selected={selected}
        dimmed={expired}
        // clod: два автообновления подряд не прошли — янтарная рамка
        sx={
          missed >= 2
            ? { borderColor: 'warning.main', borderWidth: '2px' }
            : undefined
        }
        onClick={(e) => {
          if (activating) {
            e.preventDefault()
            e.stopPropagation()
            return
          }
          pressCard()
        }}
        {...pressable(() => {
          if (!activating) pressCard()
        })}
        onContextMenu={(event) => {
          const { clientX, clientY } = event
          setPosition({ top: clientY, left: clientX })
          setAdvancedOpen(false)
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

          {option?.secure && (
            <ShieldRounded
              titleAccess={t(
                'profiles.modals.profileForm.fields.secureChannel',
              )}
              sx={{ fontSize: 18, flexShrink: 0, color: 'success.main' }}
            />
          )}

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
              disabled={loading || paused}
              onClick={(e) => {
                e.stopPropagation()
                if (activating || loading || paused) {
                  return
                }
                onUpdate(false)
              }}
            >
              <RefreshRounded color="inherit" />
            </IconButton>
          )}
        </Box>

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
          {/* clod:Э10-08 — при отказе ядра файл на диске остаётся прежним, а
              отметка времени и данные панели обновляются, поэтому дата рядом
              говорила бы «обновлено только что» над прежним конфигом. Пометка
              снимается при удачном применении. */}
          {itemData.not_applied && (
            <Typography
              noWrap
              component="span"
              title={t('profiles.components.profileItem.tooltips.notApplied')}
              sx={{
                fontSize: 13,
                minWidth: 0,
                mr: 0.75,
                color: 'warning.main',
                cursor: 'default',
              }}
            >
              {t('profiles.components.profileItem.labels.notApplied')}
            </Typography>
          )}
          {itemData.update_failed && !itemData.not_applied && missed === 0 && (
            <Typography
              noWrap
              component="span"
              title={t(
                'profiles.components.profileItem.status.lastUpdateFailed',
              )}
              sx={{
                fontSize: 13,
                minWidth: 0,
                mr: 0.75,
                color: 'error.main',
                cursor: 'default',
              }}
            >
              {t('profiles.components.profileItem.status.lastUpdateFailed')}
            </Typography>
          )}
          {hasUrl && (
            <Typography
              noWrap
              component="span"
              title={
                showNextUpdate
                  ? t('profiles.components.profileItem.tooltips.showLast')
                  : `${t('shared.labels.updateTime')}: ${profileDate(updated)}\n${t('profiles.components.profileItem.tooltips.showNext')}`
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
              onClick={(event) => {
                event.stopPropagation()
                toggleUpdateTimeDisplay()
              }}
              {...pressable(toggleUpdateTimeDisplay)}
            >
              {showNextUpdate ? nextUpdateText : fetchedAgo}
            </Typography>
          )}
        </Box>

        <Box sx={{ minWidth: 0, minHeight: 20 }}>
          <Typography
            noWrap
            title={
              description ? description : `${t('shared.labels.from')} ${from}`
            }
            sx={{ fontSize: 13 }}
          >
            {description
              ? description
              : hasUrl
                ? itemData.group
                  ? `${itemData.group} · ${from}`
                  : from
                : ''}
          </Typography>
        </Box>

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
              {profileDate(updated)}
            </span>
          </Box>
        )}
        {missed === 1 && (
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mt: 1 }}>
            <Box
              sx={{
                width: 6,
                height: 6,
                borderRadius: '50%',
                flexShrink: 0,
                bgcolor: 'warning.main',
              }}
            />
            <Typography noWrap sx={{ fontSize: 12, color: 'warning.main' }}>
              {t('profiles.components.profileItem.status.stale', {
                time: fetchedAgo,
              })}
            </Typography>
          </Box>
        )}
        {missed >= 2 && (
          <>
            <Divider
              sx={{
                my: 1.25,
                borderColor: (theme) => alpha(theme.palette.warning.main, 0.3),
              }}
            />
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
              <Box sx={{ flex: 1, minWidth: 0 }}>
                <Typography
                  sx={{ fontSize: 14, fontWeight: 600, color: 'warning.main' }}
                >
                  {t('profiles.components.profileItem.status.staleTitle')}
                </Typography>
                <Typography sx={{ fontSize: 12 }}>
                  {t('profiles.components.profileItem.status.staleDetail', {
                    time: fetchedAgo,
                  })}
                </Typography>
              </Box>
              <Button
                size="small"
                variant="contained"
                color="warning"
                disabled={loading || paused}
                sx={{ flexShrink: 0 }}
                onClick={(e) => {
                  e.stopPropagation()
                  if (activating || loading || paused) return
                  onUpdate(false)
                }}
              >
                {t('profiles.components.menu.update')}
              </Button>
            </Box>
          </>
        )}
        {hwidNotice && (
          <Typography
            title={t(`profiles.components.profileItem.status.${hwidNotice}`)}
            color="error"
            sx={{ fontSize: 12, mt: 1, lineHeight: 1.35 }}
          >
            {t(`profiles.components.profileItem.status.${hwidNotice}`)}
          </Typography>
        )}
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
                  void openProviderLink(itemData.portal_url ?? '')
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
                  void openProviderLink(itemData.support_url ?? '')
                }}
              >
                {t('profiles.components.hwidDialog.support')}
              </Button>
            )}
          </Box>
        )}
      </ProfileBox>

      <Menu
        action={menuActionsRef}
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
        {mainMenu.map((item) => renderMenuItem(item))}
        <Divider sx={{ my: 0.5 }} />
        <MenuItem
          onClick={() => setAdvancedOpen((open) => !open)}
          sx={{ minWidth: 120, color: 'text.secondary', gap: 2 }}
          dense
        >
          <Box component="span" sx={{ flex: 1 }}>
            {t(menuLabels.advanced)}
          </Box>
          {advancedOpen ? (
            <ExpandLessRounded fontSize="small" />
          ) : (
            <ExpandMoreRounded fontSize="small" />
          )}
        </MenuItem>
        {advancedOpen && advancedMenu.map((item) => renderMenuItem(item, true))}
      </Menu>
      {fileOpen && (
        <EditorViewer
          open={true}
          value={profileDocument.value}
          language="yaml"
          path={`profile:${uid}.yaml`}
          title={t(menuLabels.editFile)}
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
          onClose={() => setRulesOpen(false)}
        />
      )}
      {proxiesOpen && (
        <ProxiesEditorViewer
          profileUid={uid}
          property={option?.proxies ?? ''}
          open={true}
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
          title={t(menuLabels.extendConfig)}
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
          title={t(menuLabels.extendScript)}
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
    </Box>
  )
}

export const ProfileItem = memo(ProfileItemBase)

function parseUrl(url?: string) {
  if (!url) return ''
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      return null
    }
    return parsed.host
  } catch {
    return null
  }
}
