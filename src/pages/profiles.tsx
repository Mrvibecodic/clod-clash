import {
  closestCenter,
  type CollisionDetection,
  DndContext,
  type DragEndEvent,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  pointerWithin,
  useSensor,
  useSensors,
} from '@dnd-kit/core'
import {
  SortableContext,
  sortableKeyboardCoordinates,
  type SortingStrategy,
} from '@dnd-kit/sortable'
import {
  AddRounded,
  CheckBoxOutlineBlankRounded,
  CheckBoxRounded,
  ClearRounded,
  DeleteRounded,
  IndeterminateCheckBoxRounded,
  RefreshRounded,
} from '@mui/icons-material'
import { Box, Button, Chip, IconButton, Stack } from '@mui/material'
import { listen, TauriEvent } from '@tauri-apps/api/event'
import { readTextFile } from '@tauri-apps/plugin-fs'
import { useLockFn } from 'ahooks'
import { throttle } from 'lodash-es'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation } from 'react-router'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { BasePage } from '@/components/base'
import {
  ProfileViewer,
  type ProfileViewerRef,
} from '@/components/profile/profile-viewer'
import { SortableProfileItem } from '@/components/profile/sortable-profile-item'
import { useTauriEvent } from '@/hooks/use-listen'
import { useProfiles } from '@/hooks/use-profiles'
import {
  createProfile,
  deleteProfile,
  enhanceProfiles,
  reorderProfile,
  updateProfile,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { revalidateQueries } from '@/services/query-client'
import { useLoadingCache, useSetLoadingCache } from '@/services/states'
import { debugLog } from '@/utils/debug'

const PROFILE_UPDATE_WORKER_LIMIT = 8
const PROFILE_SWITCH_LOADING_DELAY = 400

const profileRectSortingStrategy: SortingStrategy = ({
  rects,
  activeIndex,
  overIndex,
  index,
}) => {
  let newIndex = index

  if (index === activeIndex) {
    newIndex = overIndex
  } else if (
    activeIndex < overIndex &&
    index > activeIndex &&
    index <= overIndex
  ) {
    newIndex = index - 1
  } else if (
    activeIndex > overIndex &&
    index >= overIndex &&
    index < activeIndex
  ) {
    newIndex = index + 1
  }

  const oldRect = rects[index]
  const newRect = rects[newIndex]
  if (!oldRect || !newRect) return null

  return {
    x: newRect.left - oldRect.left,
    y: newRect.top - oldRect.top,
    scaleX: newRect.width / oldRect.width,
    scaleY: newRect.height / oldRect.height,
  }
}

interface ProfileSwitchRequest {
  profile: string
  notifySuccess: boolean
  force: boolean
}

const profileCollisionDetection: CollisionDetection = (args) => {
  const pointerCollisions = pointerWithin(args)
  return pointerCollisions.length > 0 ? pointerCollisions : closestCenter(args)
}

const debugProfileSwitch = (action: string, profile: string, extra?: any) => {
  const timestamp = new Date().toISOString().substring(11, 23)
  debugLog(`[Profile-Debug][${timestamp}] ${action}: ${profile}`, extra || '')
}

const ProfilePage = () => {
  const { t } = useTranslation()
  const location = useLocation()
  const [activatings, setActivatings] = useState<string[]>([])
  const [switchTarget, setSwitchTarget] = useState<string | null>(null)
  const [visibleSwitchingProfile, setVisibleSwitchingProfile] = useState<
    string | null
  >(null)
  const [timerUpdateRevisions, setTimerUpdateRevisions] = useState<
    Map<string, number>
  >(() => new Map())
  const [completedUpdateRevisions, setCompletedUpdateRevisions] = useState<
    Map<string, number>
  >(() => new Map())

  const [batchMode, setBatchMode] = useState(false)
  const [selectedProfiles, setSelectedProfiles] = useState<Set<string>>(
    () => new Set(),
  )

  const latestSwitchTargetRef = useRef<string | null>(null)
  const queuedSwitchRef = useRef<ProfileSwitchRequest | null>(null)
  const switchRunnerRef = useRef<Promise<void> | null>(null)
  const switchLoadingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const currentProfileRef = useRef<string | undefined>(undefined)
  const profilePageMountedRef = useRef(true)
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  )
  const { current } = location.state || {}

  const {
    profiles = {},
    patchProfiles,
    mutateProfiles,
    error,
    isStale,
  } = useProfiles()

  useEffect(() => {
    currentProfileRef.current = profiles.current
  }, [profiles])

  useTauriEvent<{ paths: string[] }>(TauriEvent.DRAG_DROP, async (event) => {
    for (const file of event.payload.paths) {
      if (!file.endsWith('.yaml') && !file.endsWith('.yml')) {
        showNotice.error('profiles.page.feedback.errors.onlyYaml')
        continue
      }
      const item = {
        type: 'local',
        name: file.split(/\/|\\/).pop() ?? 'New Profile',
        desc: '',
        url: '',
        option: {
          with_proxy: false,
          self_proxy: false,
        },
      } as IProfileItem
      const data = await readTextFile(file)
      await createProfile(item, data)
      await mutateProfiles()
    }
    await enhanceProfiles()
  })

  const onEmergencyRefresh = useLockFn(async () => {
    debugLog(
      '[Экстренное обновление] Начало принудительного обновления всех данных',
    )

    try {
      await revalidateQueries([['getProfiles']])

      await mutateProfiles()

      await new Promise((resolve) => setTimeout(resolve, 500))
      await onEnhance(false)

      showNotice.success(
        'profiles.page.feedback.notices.forceRefreshCompleted',
        2000,
      )
    } catch (error) {
      console.error('[Экстренное обновление] Ошибка:', error)
      showNotice.error(
        'profiles.page.feedback.notices.emergencyRefreshFailed',
        { message: String(error) },
        4000,
      )
    }
  })

  const viewerRef = useRef<ProfileViewerRef>(null)

  const profileItems = useMemo(() => {
    const items = profiles.items || []

    const type1 = ['local', 'remote']

    return items.filter((i) => i && type1.includes(i.type!))
  }, [profiles])

  const [group, setGroup] = useState('')
  const groups = useMemo(() => {
    const counts = new Map<string, number>()
    for (const item of profileItems) {
      const name = item.group?.trim()
      if (name) counts.set(name, (counts.get(name) ?? 0) + 1)
    }
    return [...counts.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [profileItems])

  const activeGroup = groups.some(([name]) => name === group) ? group : ''

  const visibleItems = useMemo(
    () =>
      activeGroup
        ? profileItems.filter((i) => i.group?.trim() === activeGroup)
        : profileItems,
    [activeGroup, profileItems],
  )

  const currentActivatings = () => {
    return [...new Set([profiles.current ?? ''])].filter(Boolean)
  }

  const onDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event
    if (over) {
      if (active.id !== over.id) {
        await reorderProfile(active.id.toString(), over.id.toString())
        mutateProfiles()
      }
    }
  }

  const executeProfileSwitch = useCallback(
    async ({ profile, notifySuccess, force }: ProfileSwitchRequest) => {
      if (!force && currentProfileRef.current === profile) {
        debugProfileSwitch('ALREADY_CURRENT_IGNORED', profile)
        return
      }

      debugProfileSwitch('SWITCH_START', profile)

      try {
        const outcome = await patchProfiles({ current: profile })
        if (outcome.status === 'busy') {
          debugProfileSwitch('SWITCH_BUSY', profile)
          showNotice.info(
            'profiles.page.feedback.notifications.switchBusy',
            2000,
          )
          return
        }

        if (outcome.status === 'valid') {
          currentProfileRef.current = profile
          void closeAllConnections().catch(() => {})

          if (
            notifySuccess &&
            latestSwitchTargetRef.current === profile &&
            queuedSwitchRef.current === null
          ) {
            showNotice.success(
              'profiles.page.feedback.notifications.profileSwitched',
              1000,
            )
          }
          debugProfileSwitch('SWITCH_SUCCESS', profile)
        } else {
          debugProfileSwitch('SWITCH_REJECTED', profile, outcome)
        }
      } catch (err: any) {
        console.error(`[Profile] Ошибка переключения:`, err)
        showNotice.error(err, 4000)
      } finally {
        debugProfileSwitch('SWITCH_END', profile)
      }
    },
    [patchProfiles],
  )

  const runProfileSwitchQueue = useCallback(async () => {
    while (profilePageMountedRef.current && queuedSwitchRef.current) {
      const request = queuedSwitchRef.current
      queuedSwitchRef.current = null
      await executeProfileSwitch(request)
    }
  }, [executeProfileSwitch])

  const activateProfile = useCallback(
    (profile: string, notifySuccess: boolean, force = false) => {
      if (!profilePageMountedRef.current) return Promise.resolve()

      if (
        !force &&
        currentProfileRef.current === profile &&
        switchRunnerRef.current === null
      ) {
        debugProfileSwitch('ALREADY_CURRENT_IGNORED', profile)
        return Promise.resolve()
      }

      if (
        latestSwitchTargetRef.current === profile &&
        switchRunnerRef.current
      ) {
        debugProfileSwitch('DUPLICATE_SWITCH_IGNORED', profile)
        return switchRunnerRef.current
      }

      latestSwitchTargetRef.current = profile
      queuedSwitchRef.current = { profile, notifySuccess, force }
      setSwitchTarget(profile)
      setVisibleSwitchingProfile(null)
      if (switchLoadingTimerRef.current) {
        window.clearTimeout(switchLoadingTimerRef.current)
      }
      switchLoadingTimerRef.current = window.setTimeout(() => {
        if (
          profilePageMountedRef.current &&
          latestSwitchTargetRef.current === profile
        ) {
          setVisibleSwitchingProfile(profile)
        }
      }, PROFILE_SWITCH_LOADING_DELAY)

      if (switchRunnerRef.current) {
        debugProfileSwitch('SWITCH_QUEUED', profile)
        return switchRunnerRef.current
      }

      const runner = runProfileSwitchQueue().finally(() => {
        if (switchRunnerRef.current === runner) {
          switchRunnerRef.current = null
          latestSwitchTargetRef.current = null
          if (switchLoadingTimerRef.current) {
            window.clearTimeout(switchLoadingTimerRef.current)
            switchLoadingTimerRef.current = null
          }
          if (profilePageMountedRef.current) {
            setSwitchTarget(null)
            setVisibleSwitchingProfile(null)
          }
        }
      })
      switchRunnerRef.current = runner
      return runner
    },
    [runProfileSwitchQueue],
  )

  const onSelect = async (profile: string, force: boolean) => {
    await activateProfile(profile, true, force)
  }

  useEffect(() => {
    let cancelled = false
    void (async () => {
      if (current) {
        await mutateProfiles()
        if (cancelled) return
        await activateProfile(current, false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [current, activateProfile, mutateProfiles])

  const onEnhance = useLockFn(async (notifySuccess: boolean) => {
    if (switchRunnerRef.current) {
      debugLog(
        `[Profile] Переключение профиля уже выполняется (${latestSwitchTargetRef.current}), пропускаем enhance`,
      )
      return
    }

    const currentProfiles = currentActivatings()
    setActivatings((prev) => [...new Set([...prev, ...currentProfiles])])

    try {
      if (!(await enhanceProfiles())) return
      if (notifySuccess) {
        showNotice.success(
          'profiles.page.feedback.notifications.profileReactivated',
          1000,
        )
      }
    } catch (err: any) {
      showNotice.error(err, 3000)
    } finally {
      setActivatings([])
    }
  })

  const onDelete = useLockFn(async (uid: string) => {
    const current = profiles.current === uid
    try {
      setActivatings([...(current ? currentActivatings() : []), uid])
      await deleteProfile(uid)
      mutateProfiles()
      if (current) {
        await onEnhance(false)
      }
    } catch (err: any) {
      showNotice.error(err)
    } finally {
      setActivatings([])
    }
  })

  const loadingCache = useLoadingCache()
  const setLoadingCache = useSetLoadingCache()
  const setLoadingProfiles = useCallback(
    (uids: string[], loading: boolean) => {
      setLoadingCache((cache) => {
        const next = new Set(cache)
        for (const uid of uids) {
          if (loading) {
            next.add(uid)
          } else {
            next.delete(uid)
          }
        }
        return next
      })
    },
    [setLoadingCache],
  )

  useEffect(() => {
    let disposed = false
    let unlisteners: Array<() => void> = []

    Promise.allSettled([
      listen<{ uid?: string }>('profile-update-started', ({ payload }) => {
        if (payload.uid) setLoadingProfiles([payload.uid], true)
      }),
      listen<{ uid?: string }>('profile-update-completed', ({ payload }) => {
        const { uid } = payload
        if (!uid) return
        setLoadingProfiles([uid], false)
        setCompletedUpdateRevisions((current) => {
          const next = new Map(current)
          next.set(uid, (next.get(uid) ?? 0) + 1)
          return next
        })
        void mutateProfiles()
      }),
      listen<string>('verge://timer-updated', ({ payload: uid }) => {
        setTimerUpdateRevisions((current) => {
          const next = new Map(current)
          next.set(uid, (next.get(uid) ?? 0) + 1)
          return next
        })
      }),
    ]).then((results) => {
      const registeredUnlisteners = results.flatMap((result) =>
        result.status === 'fulfilled' ? [result.value] : [],
      )
      results.forEach((result) => {
        if (result.status === 'rejected') console.error(result.reason)
      })

      if (disposed) {
        registeredUnlisteners.forEach((unlisten) => unlisten())
      } else {
        unlisteners = registeredUnlisteners
      }
    })

    return () => {
      disposed = true
      unlisteners.forEach((unlisten) => unlisten())
    }
  }, [mutateProfiles, setLoadingProfiles])

  const runProfileUpdates = useCallback(
    async (uids: string[]) => {
      if (uids.length === 0) return

      const throttleMutate = throttle(mutateProfiles, 2000, {
        trailing: true,
      })
      let cursor = 0

      const updateOne = async (uid: string) => {
        try {
          await updateProfile(uid)
          throttleMutate()
        } catch (err: any) {
          console.error(`Не удалось обновить подписку ${uid}:`, err)
        }
      }

      const worker = async () => {
        while (cursor < uids.length) {
          const uid = uids[cursor++]
          await updateOne(uid)
        }
      }

      try {
        const active = Math.min(PROFILE_UPDATE_WORKER_LIMIT, uids.length)
        await Promise.allSettled(Array.from({ length: active }, worker))
      } finally {
        setLoadingProfiles(uids, false)

        void mutateProfiles()
      }
    },
    [mutateProfiles, setLoadingProfiles],
  )
  const onUpdateAll = useLockFn(async () => {
    const items = profileItems.filter((e) => e.type === 'remote')
    const target = items
      .map((item) => item.uid)
      .filter((uid) => !loadingCache.has(uid))

    setLoadingProfiles(target, true)
    await runProfileUpdates(target)
  })

  const toggleBatchMode = () => {
    setBatchMode(!batchMode)
    if (!batchMode) {
      setSelectedProfiles(new Set())
    }
  }

  const toggleProfileSelection = (uid: string) => {
    setSelectedProfiles((prev) => {
      const newSet = new Set(prev)
      if (newSet.has(uid)) {
        newSet.delete(uid)
      } else {
        newSet.add(uid)
      }
      return newSet
    })
  }

  const selectAllProfiles = () => {
    setSelectedProfiles(new Set(profileItems.map((item) => item.uid)))
  }

  const clearAllSelections = () => {
    setSelectedProfiles(new Set())
  }

  const isAllSelected = () => {
    return (
      profileItems.length > 0 && profileItems.length === selectedProfiles.size
    )
  }

  const getSelectionState = () => {
    if (selectedProfiles.size === 0) {
      return 'none'
    } else if (selectedProfiles.size === profileItems.length) {
      return 'all'
    } else {
      return 'partial'
    }
  }

  const deleteSelectedProfiles = useLockFn(async () => {
    if (selectedProfiles.size === 0) return

    try {
      const currentActivating =
        profiles.current && selectedProfiles.has(profiles.current)
          ? [profiles.current]
          : []

      setActivatings((prev) => [...new Set([...prev, ...currentActivating])])

      for (const uid of selectedProfiles) {
        await deleteProfile(uid)
      }

      await mutateProfiles()

      if (currentActivating.length > 0) {
        await onEnhance(false)
      }

      setSelectedProfiles(new Set())
      setBatchMode(false)

      showNotice.success('profiles.page.feedback.notifications.batchDeleted')
    } catch (err: any) {
      showNotice.error(err)
    } finally {
      setActivatings([])
    }
  })

  useEffect(() => {
    profilePageMountedRef.current = true
    return () => {
      profilePageMountedRef.current = false
      queuedSwitchRef.current = null
      latestSwitchTargetRef.current = null
      if (switchLoadingTimerRef.current) {
        window.clearTimeout(switchLoadingTimerRef.current)
        switchLoadingTimerRef.current = null
      }
    }
  }, [])

  return (
    <BasePage
      full
      title={t('profiles.page.title')}
      contentStyle={{ height: '100%' }}
      header={
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          {!batchMode ? (
            <>
              <IconButton
                size="small"
                color="inherit"
                title={t('profiles.page.batch.title')}
                onClick={toggleBatchMode}
              >
                <CheckBoxOutlineBlankRounded />
              </IconButton>

              <IconButton
                size="small"
                color="inherit"
                title={t('profiles.page.actions.updateAll')}
                onClick={onUpdateAll}
              >
                <RefreshRounded />
              </IconButton>

              {(error || isStale) && (
                <IconButton
                  size="small"
                  color="warning"
                  title={t('profiles.page.tooltips.emergencyRefresh')}
                  onClick={onEmergencyRefresh}
                  sx={{
                    animation: 'pulse 2s infinite',
                    '@keyframes pulse': {
                      '0%': { opacity: 1 },
                      '50%': { opacity: 0.5 },
                      '100%': { opacity: 1 },
                    },
                  }}
                >
                  <ClearRounded />
                </IconButton>
              )}
            </>
          ) : (
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
              <IconButton
                size="small"
                color="inherit"
                title={
                  isAllSelected()
                    ? t('profiles.page.batch.actions.deselectAll')
                    : t('profiles.page.batch.actions.selectAll')
                }
                onClick={
                  isAllSelected() ? clearAllSelections : selectAllProfiles
                }
              >
                {getSelectionState() === 'all' ? (
                  <CheckBoxRounded />
                ) : getSelectionState() === 'partial' ? (
                  <IndeterminateCheckBoxRounded />
                ) : (
                  <CheckBoxOutlineBlankRounded />
                )}
              </IconButton>
              <IconButton
                size="small"
                color="error"
                title={t('profiles.page.batch.actions.delete')}
                onClick={deleteSelectedProfiles}
                disabled={selectedProfiles.size === 0}
              >
                <DeleteRounded />
              </IconButton>
              <Button size="small" variant="outlined" onClick={toggleBatchMode}>
                {t('profiles.page.batch.actions.done')}
              </Button>
              <Box
                sx={{ flex: 1, textAlign: 'right', color: 'text.secondary' }}
              >
                {t('profiles.page.batch.summary.selected')}{' '}
                {selectedProfiles.size} {t('profiles.page.batch.summary.items')}
              </Box>
            </Box>
          )}
        </Box>
      }
    >
      <Stack
        direction="row"
        spacing={1}
        sx={{
          pt: 1,
          mb: 0.5,
          mx: '10px',
          display: 'flex',
          alignItems: 'center',
        }}
      >
        <Button
          variant="contained"
          size="small"
          startIcon={<AddRounded />}
          sx={{ borderRadius: '8px' }}
          onClick={() => viewerRef.current?.create()}
        >
          {t('profiles.page.actions.addSubscription')}
        </Button>
      </Stack>

      {groups.length > 0 && (
        <Stack
          direction="row"
          spacing={0.75}
          sx={{ mx: '10px', mb: 1, flexWrap: 'wrap', rowGap: 0.75 }}
        >
          <Chip
            size="small"
            label={`${t('profiles.page.groups.all')} · ${profileItems.length}`}
            color={activeGroup ? 'default' : 'primary'}
            variant={activeGroup ? 'outlined' : 'filled'}
            onClick={() => setGroup('')}
          />
          {groups.map(([name, count]) => (
            <Chip
              key={name}
              size="small"
              label={`${name} · ${count}`}
              color={activeGroup === name ? 'primary' : 'default'}
              variant={activeGroup === name ? 'filled' : 'outlined'}
              onClick={() => setGroup(name)}
            />
          ))}
        </Stack>
      )}

      <DndContext
        sensors={sensors}
        collisionDetection={profileCollisionDetection}
        onDragEnd={onDragEnd}
      >
        <Box
          sx={{
            pl: '10px',
            pr: '10px',
            height: 'calc(100% - 48px)',
            overflowY: 'auto',
          }}
        >
          <Box sx={{ mb: 1.5 }}>
            <Box
              sx={{
                display: 'grid',
                gap: 1,
                gridTemplateColumns:
                  'repeat(auto-fill, minmax(min(320px, 100%), 1fr))',
              }}
            >
              <SortableContext
                strategy={profileRectSortingStrategy}
                items={visibleItems.map((x) => {
                  return x.uid
                })}
              >
                {visibleItems.map((item) => (
                  <Box sx={{ minWidth: 0 }} key={item.file}>
                    <SortableProfileItem
                      id={item.uid}
                      selected={(switchTarget ?? profiles.current) === item.uid}
                      activating={
                        activatings.includes(item.uid) ||
                        visibleSwitchingProfile === item.uid
                      }
                      itemData={item}
                      timerUpdateRevision={
                        timerUpdateRevisions.get(item.uid) ?? 0
                      }
                      completedUpdateRevision={
                        completedUpdateRevisions.get(item.uid) ?? 0
                      }
                      mutateProfiles={mutateProfiles}
                      onSelect={(f) => onSelect(item.uid, f)}
                      onEdit={() => viewerRef.current?.edit(item)}
                      onSave={async (prev, curr) => {
                        if (prev !== curr && profiles.current === item.uid) {
                          await onEnhance(false)
                        }
                      }}
                      onDelete={() => {
                        if (batchMode) {
                          toggleProfileSelection(item.uid)
                        } else {
                          onDelete(item.uid)
                        }
                      }}
                      batchMode={batchMode}
                      isSelected={selectedProfiles.has(item.uid)}
                      onSelectionChange={() => toggleProfileSelection(item.uid)}
                    />
                  </Box>
                ))}
              </SortableContext>
            </Box>
          </Box>
        </Box>
        <DragOverlay />
      </DndContext>

      <ProfileViewer
        ref={viewerRef}
        onChange={async (isActivating) => {
          mutateProfiles()

          if (isActivating) {
            await onEnhance(false)
          }
        }}
      />
    </BasePage>
  )
}

export default ProfilePage
