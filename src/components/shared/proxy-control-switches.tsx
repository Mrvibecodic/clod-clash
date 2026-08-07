import {
  BuildRounded,
  DeleteForeverRounded,
  PauseCircleOutlineRounded,
  PlayCircleOutlineRounded,
  SettingsRounded,
  WarningRounded,
} from '@mui/icons-material'
import { Box, Typography, alpha, useTheme } from '@mui/material'
import { useLockFn } from 'ahooks'
import React, { useCallback, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { DialogRef, Switch, TooltipIcon } from '@/components/base'
import { SysproxyViewer } from '@/components/setting/mods/sysproxy-viewer'
import { TunViewer } from '@/components/setting/mods/tun-viewer'
import { useRememberTargets } from '@/hooks/use-connect-targets'
import { useServiceUninstaller } from '@/hooks/use-service-uninstaller'
import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useSystemState } from '@/hooks/use-system-state'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { ensureTunReady } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

interface ProxySwitchProps {
  label?: string
  onError?: (err: Error) => void
  noRightPadding?: boolean
}

interface SwitchRowProps {
  label: string
  active: boolean
  disabled?: boolean
  infoTitle: string
  onInfoClick?: () => void
  extraIcons?: React.ReactNode
  onToggle: (value: boolean) => Promise<void>
  onError?: (err: Error) => void
  highlight?: boolean
}

/**
 * Вынесенный подкомпонент: единый UI переключателя
 * active = фактическое состояние ОС/конфига, оптимистичное обновление
 */
const SwitchRow = ({
  label,
  active,
  disabled,
  infoTitle,
  onInfoClick,
  extraIcons,
  onToggle,
  onError,
  highlight,
}: SwitchRowProps) => {
  const theme = useTheme()
  const [checked, setChecked] = useState(active)
  const pendingRef = useRef(false)

  if (pendingRef.current) {
    if (active === checked) pendingRef.current = false
  } else if (checked !== active) {
    setChecked(active)
  }

  const handleChange = (_: React.ChangeEvent, value: boolean) => {
    pendingRef.current = true
    setChecked(value)
    onToggle(value)
      .catch((err: any) => {
        setChecked(active)
        onError?.(err)
      })
      .finally(() => {
        pendingRef.current = false
      })
  }

  return (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        p: 1,
        pr: 2,
        borderRadius: 1.5,
        bgcolor: highlight
          ? alpha(theme.palette.success.main, 0.07)
          : 'transparent',
        opacity: disabled ? 0.6 : 1,
        transition: 'background-color 0.3s',
      }}
    >
      <Box sx={{ display: 'flex', alignItems: 'center' }}>
        {active ? (
          <PlayCircleOutlineRounded sx={{ color: 'success.main', mr: 1 }} />
        ) : (
          <PauseCircleOutlineRounded sx={{ color: 'text.disabled', mr: 1 }} />
        )}
        <Typography
          variant="subtitle1"
          sx={{ fontWeight: 500, fontSize: '15px' }}
        >
          {label}
        </Typography>
        <TooltipIcon
          title={infoTitle}
          icon={SettingsRounded}
          onClick={onInfoClick}
          sx={{ ml: 1 }}
        />
        {extraIcons}
      </Box>

      <Switch
        edge="end"
        disabled={disabled}
        checked={checked}
        onChange={handleChange}
      />
    </Box>
  )
}

const ProxyControlSwitches = ({
  label,
  onError,
  noRightPadding = false,
}: ProxySwitchProps) => {
  const { t } = useTranslation()
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { uninstallServiceAndRestartCore } = useServiceUninstaller()
  const { indicator: systemProxyIndicator, toggleSystemProxy } =
    useSystemProxyState()
  // clod: тумблер показывает ФАКТ (`tunActive`), как и быстрые действия на
  // главной. Раньше настройки читали `enable_tun_mode` — желание из конфига —
  // и расходились с главной: там туннель погашен подавлением, здесь горит.
  // clod:service-repair — готовность и нужду в ремонте считает бэкенд: он один
  // смотрит версию службы. Прежняя формула «админ ИЛИ служба отвечает» на
  // устаревшей службе давала «доступно», поэтому ни предупреждения, ни кнопки
  // здесь не появлялось — а TUN всё равно не работал.
  const { tunActive, tunCapable, tunNeedsRepair, mutateTunState } =
    useTunState()
  const { isServiceOk, mutateSystemState } = useSystemState()
  // Тумблеры здесь и есть выбор режима для кнопки Connect — отдельной пары
  // настроек «Подключение: …» больше нет.
  const rememberTarget = useRememberTargets()

  const sysproxyRef = useRef<DialogRef>(null)
  const tunRef = useRef<DialogRef>(null)

  const showErrorNotice = useCallback(
    (msg: string) => showNotice.error(msg),
    [],
  )

  const handleTunToggle = async (value: boolean) => {
    // clod:tun-ready — включение TUN само доводит систему до рабочего
    // состояния: если службы нет, ставим её (один запрос прав). Ошибка
    // остаётся только для случая, когда пользователь отказал.
    if (value && !tunCapable) {
      const ready = await ensureTunReady()
      await Promise.all([mutateSystemState(), mutateTunState()])
      if (!ready) {
        const msgKey = 'settings.sections.proxyControl.tooltips.tunUnavailable'
        showErrorNotice(msgKey)
        throw new Error(t(msgKey))
      }
    }
    mutateVerge({ ...verge, enable_tun_mode: value }, false)
    try {
      await patchVerge({ enable_tun_mode: value })
      void rememberTarget('tun', value)
    } catch (err) {
      // Бэкенд откатил патч (discard) — перечитываем конфиг, иначе в кэше
      // останется оптимистичное значение, которого нет на диске.
      mutateVerge()
      throw err
    } finally {
      // Тумблер показывает факт — спрашиваем его у бэкенда после патча. В
      // `finally`, а не в `try`: провал этого запроса не делает патч неудачным
      // и не должен откатывать переключатель.
      await mutateTunState().catch(() => undefined)
    }
  }

  // clod:service-repair — одна кнопка на оба случая: минимальное действие
  // выбирает бэкенд (`ensure_ready` → поставить, запустить или починить), и
  // это единственный путь, у которого одинаковая логика с тумблером.
  const onFixService = useLockFn(async () => {
    try {
      await ensureTunReady()
      await Promise.all([mutateSystemState(), mutateTunState()])
    } catch (err) {
      showNotice.error(err)
    }
  })

  const onUninstallService = useLockFn(async () => {
    try {
      if (verge?.enable_tun_mode) {
        await handleTunToggle(false)
      }
      await uninstallServiceAndRestartCore()
      await mutateSystemState()
    } catch (err) {
      showNotice.error(err)
    }
  })

  const isSystemProxyMode =
    label === t('settings.sections.system.toggles.systemProxy') || !label
  const isTunMode = label === t('settings.sections.system.toggles.tunMode')

  return (
    <Box sx={{ width: '100%', pr: noRightPadding ? 1 : 2 }}>
      {isSystemProxyMode && (
        <SwitchRow
          label={t('settings.sections.proxyControl.fields.systemProxy')}
          active={systemProxyIndicator}
          infoTitle={t('settings.sections.proxyControl.tooltips.systemProxy')}
          onInfoClick={() => sysproxyRef.current?.open()}
          onToggle={async (value) => {
            await toggleSystemProxy(value)
            void rememberTarget('sys', value)
          }}
          onError={onError}
          highlight={systemProxyIndicator}
        />
      )}

      {isTunMode && (
        <SwitchRow
          label={t('settings.sections.proxyControl.fields.tunMode')}
          active={tunActive}
          infoTitle={t('settings.sections.proxyControl.tooltips.tunMode')}
          onInfoClick={() => tunRef.current?.open()}
          onToggle={handleTunToggle}
          onError={onError}
          highlight={tunActive}
          extraIcons={
            <>
              {!tunCapable && (
                <>
                  <TooltipIcon
                    title={t(
                      tunNeedsRepair
                        ? 'settings.sections.proxyControl.tooltips.serviceOutdated'
                        : 'settings.sections.proxyControl.tooltips.tunUnavailable',
                    )}
                    icon={WarningRounded}
                    sx={{ color: 'warning.main', ml: 1 }}
                  />
                  <TooltipIcon
                    title={t(
                      tunNeedsRepair
                        ? 'settings.sections.proxyControl.actions.repairService'
                        : 'settings.sections.proxyControl.actions.installService',
                    )}
                    icon={BuildRounded}
                    color="primary"
                    onClick={onFixService}
                    sx={{ ml: 1 }}
                  />
                </>
              )}
              {isServiceOk && (
                <TooltipIcon
                  title={t(
                    'settings.sections.proxyControl.actions.uninstallService',
                  )}
                  icon={DeleteForeverRounded}
                  color="secondary"
                  onClick={onUninstallService}
                  sx={{ ml: 1 }}
                />
              )}
            </>
          }
        />
      )}

      <SysproxyViewer ref={sysproxyRef} />
      <TunViewer ref={tunRef} />
    </Box>
  )
}

export default ProxyControlSwitches
