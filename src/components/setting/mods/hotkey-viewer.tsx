import { Box } from '@mui/material'
import { useLockFn } from 'ahooks'
import { forwardRef, useImperativeHandle, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  DialogRef,
  FormRow,
  FormSection,
  FormTile,
  Switch,
} from '@/components/base'
import { useChangeCount } from '@/hooks/use-change-count'
import { useVerge } from '@/hooks/use-verge'
import { showNotice } from '@/services/notice-service'

import { HotkeyInput } from './hotkey-input'

const HOTKEY_FUNC = [
  'open_or_close_dashboard',
  'clash_mode_rule',
  'clash_mode_global',
  'clash_mode_direct',
  'toggle_system_proxy',
  'toggle_tun_mode',
  'entry_lightweight_mode',
  'reactivate_profiles',
] as const

const snapshot = (map: Record<string, string[]>, enabled: boolean) => {
  const result: Record<string, string | boolean> = { enabled }
  for (const func of HOTKEY_FUNC) result[func] = (map[func] ?? []).join('+')
  return result
}

const HOTKEY_FUNC_LABELS: Record<(typeof HOTKEY_FUNC)[number], string> = {
  open_or_close_dashboard:
    'settings.modals.hotkey.functions.openOrCloseDashboard',
  clash_mode_rule: 'settings.modals.hotkey.functions.rule',
  clash_mode_global: 'settings.modals.hotkey.functions.global',
  clash_mode_direct: 'settings.modals.hotkey.functions.direct',
  toggle_system_proxy: 'settings.modals.hotkey.functions.toggleSystemProxy',
  toggle_tun_mode: 'settings.modals.hotkey.functions.toggleTunMode',
  entry_lightweight_mode:
    'settings.modals.hotkey.functions.entryLightweightMode',
  reactivate_profiles: 'settings.modals.hotkey.functions.reactivateProfiles',
}

export const HotkeyViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)

  const { verge, patchVerge } = useVerge()

  const [hotkeyMap, setHotkeyMap] = useState<Record<string, string[]>>({})
  const [enableGlobalHotkey, setEnableGlobalHotkey] = useState(
    verge?.enable_global_hotkey ?? true,
  )
  const [initial, setInitial] = useState({
    map: hotkeyMap,
    enabled: enableGlobalHotkey,
  })
  const [inputGeneration, setInputGeneration] = useState(0)
  const changes = useChangeCount(
    useMemo(() => snapshot(initial.map, initial.enabled), [initial]),
    useMemo(
      () => snapshot(hotkeyMap, enableGlobalHotkey),
      [hotkeyMap, enableGlobalHotkey],
    ),
  )

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)

      const map = {} as typeof hotkeyMap

      verge?.hotkeys?.forEach((text) => {
        const [func, key] = text.split(',').map((e) => e.trim())

        if (!func || !key) return

        map[func] = key
          .split('+')
          .map((e) => e.trim())
          .map((k) => (k === 'PLUS' ? '+' : k))
      })

      setHotkeyMap(map)
      setEnableGlobalHotkey(verge?.enable_global_hotkey ?? true)
      setInitial({ map, enabled: verge?.enable_global_hotkey ?? true })
    },
    close: () => setOpen(false),
  }))

  const onSave = useLockFn(async () => {
    const owners = new Map<string, (typeof HOTKEY_FUNC)[number]>()
    for (const func of HOTKEY_FUNC) {
      const keys = hotkeyMap[func] ?? []
      const combo = keys
        .map((k) => k.trim())
        .filter(Boolean)
        .sort()
        .join('+')
      if (!combo) continue
      const other = owners.get(combo)
      if (other) {
        showNotice.error('settings.modals.hotkey.messages.duplicate', {
          keys: keys.join('+'),
          other: t(HOTKEY_FUNC_LABELS[other]),
          func: t(HOTKEY_FUNC_LABELS[func]),
        })
        return
      }
      owners.set(combo, func)
    }

    const hotkeys = Object.entries(hotkeyMap)
      .map(([func, keys]) => {
        if (!func || !keys?.length) return ''

        const key = keys
          .map((k) => k.trim())
          .filter(Boolean)
          .map((k) => (k === '+' ? 'PLUS' : k))
          .join('+')

        if (!key) return ''
        return `${func},${key}`
      })
      .filter(Boolean)

    try {
      await patchVerge({
        hotkeys,
        enable_global_hotkey: enableGlobalHotkey,
      })
      setOpen(false)
    } catch (err) {
      showNotice.error(err)
    }
  })

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.hotkey.title')}
      dividers
      changes={changes}
      onReset={() => {
        setHotkeyMap(initial.map)
        setEnableGlobalHotkey(initial.enabled)
        setInputGeneration((n) => n + 1)
      }}
      contentSx={{ width: 572 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormRow label={t('settings.modals.hotkey.toggles.enableGlobal')}>
        <Switch
          edge="end"
          checked={enableGlobalHotkey}
          onChange={(e) => setEnableGlobalHotkey(e.target.checked)}
        />
      </FormRow>

      <FormSection
        title={t('settings.modals.hotkey.sections.actions')}
        count={HOTKEY_FUNC.length}
        hint={t('settings.modals.hotkey.messages.recordHint')}
      />
      {HOTKEY_FUNC.map((func) => (
        <FormTile key={func} sx={{ pr: 0.5 }}>
          <Box sx={{ flex: 1, minWidth: 0, fontSize: 14 }}>
            {t(HOTKEY_FUNC_LABELS[func])}
          </Box>
          <HotkeyInput
            key={inputGeneration}
            value={hotkeyMap[func] ?? []}
            onChange={(v) => setHotkeyMap((m) => ({ ...m, [func]: v }))}
          />
        </FormTile>
      ))}
    </BaseDialog>
  )
})
