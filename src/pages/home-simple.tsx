import {
  Box,
  Button,
  Checkbox,
  FormControlLabel,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import {
  ConnectButton,
  type ConnectState,
} from '@/components/home/connect-button'
import { ModeStatus } from '@/components/home/mode-status'
import { ProviderBanners } from '@/components/home/provider-banners'
import { ProviderHeader } from '@/components/home/provider-header'
import { ProviderLinksCard } from '@/components/home/provider-links'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { SessionTraffic } from '@/components/home/session-traffic'
import { SubscriptionCard } from '@/components/home/subscription-card'
import { TunStatus } from '@/components/home/tun-status'
import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useProfiles } from '@/hooks/use-profiles'
import { useSessionUptime } from '@/hooks/use-session-uptime'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useFitWindowToContent } from '@/hooks/use-window-fit'
import { createProfile, enhanceProfiles, importProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

const HomeSimplePage = () => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const {
    current,
    profiles,
    error: profilesError,
    mutateProfiles,
  } = useProfiles()
  const { connected, willConnect, toggleConnection } = useConnectTargets()
  const { setSimpleMode } = useSimpleMode()
  const uptime = useSessionUptime(connected)
  // clod:fit-window — окно тянется под содержимое, а на маленьком экране
  // сначала поджимается вёрстка и только потом появляется прокрутка
  const { fitRef, compact } = useFitWindowToContent()

  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<{ text: string; at: boolean }>()
  const [serverOpen, setServerOpen] = useState(false)
  const [intent, setIntent] = useState<'connecting' | 'disconnecting'>()
  const [subUrl, setSubUrl] = useState('')
  // clod:chan — защищённый канал выбирается здесь же, при добавлении: признак
  // липкий (снять его потом нельзя), а до первой подписки другого места, где
  // его можно было бы поставить, у человека нет.
  const [subSecure, setSubSecure] = useState(false)

  // clod:tun-ready — ошибка запоминается вместе с состоянием подключения, в
  // котором случилась, и перестаёт показываться, как только оно изменилось:
  // поставил службу, включил TUN тумблером — кнопка больше не красная.
  const errorText = failure?.at === connected ? failure.text : undefined

  const state: ConnectState = errorText
    ? 'error'
    : busy
      ? // Намерение фиксируется в момент нажатия: `connected` по ходу дела
        // гаснет (TUN гасится первым, системный прокси следом), и подпись
        // посреди отключения превращалась бы в «Подключение…».
        (intent ?? 'connecting')
      : connected
        ? 'on'
        : 'off'

  const toggle = useLockFn(async () => {
    // Подпись — от того, что нажатие СДЕЛАЕТ. Это почти всегда обратное
    // показанному, но подавленный туннель оставляет кнопку тёмной, а нажатие
    // при поднятом системном прокси всё равно отключает.
    setIntent(willConnect ? 'connecting' : 'disconnecting')
    setBusy(true)
    setFailure(undefined)
    try {
      await toggleConnection()
    } catch (error) {
      setFailure({
        text: error instanceof Error ? error.message : String(error),
        at: connected,
      })
    } finally {
      setBusy(false)
      setIntent(undefined)
    }
  })

  const addSubscription = useLockFn(async () => {
    const url = subUrl.trim()
    if (!url) return
    // clod: без enhance ядро не перечитывает конфиг после импорта — список
    // серверов оставался пустым, пока подписку не обновишь руками
    const activate = async () => {
      await mutateProfiles()
      try {
        await enhanceProfiles()
      } catch (error) {
        console.error('[import] enhance after import failed:', error)
      }
    }
    // Значение по умолчанию у `importProfile` — `{ with_proxy: true }`;
    // повторяем его, иначе галочка молча меняла бы ещё и способ загрузки.
    const option = subSecure ? { with_proxy: true, secure: true } : undefined
    try {
      await importProfile(url, option)
      await activate()
      setSubUrl('')
    } catch {
      // A URL that is not a subscription is still a perfectly normal mistake;
      // fall back to creating a remote profile so the error names the cause.
      try {
        // clod:panel-name — без имени: если вторая попытка всё-таки принесёт
        // подписку, назовёт её панель своим `profile-title`, а не адрес,
        // который пользователь вставил (он же ещё и с токеном).
        await createProfile({ type: 'remote', url, option })
        await activate()
        setSubUrl('')
      } catch (error) {
        showNotice.error(error)
      }
    }
  })

  // clod:first-paint — «Добавьте подписку» показывается только тогда, когда
  // список подписок УЖЕ пришёл и он пуст. Первый кадр рисуется раньше ответа
  // бэкенда, и до этой проверки экран приглашения успевал моргнуть на каждом
  // запуске у человека с давно добавленной подпиской. Пустое место
  // естественнее ложного приглашения. Если запрос ОТКАЗАЛ, приглашение всё же
  // показываем: пустой экран навсегда — худший из исходов.
  if (!profiles && !profilesError) {
    return <Stack sx={{ height: '100%' }} />
  }

  // No subscription yet: the only thing worth showing is how to add one.
  if (!current) {
    return (
      <Stack
        ref={fitRef}
        sx={{
          height: '100%',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 2,
          px: 4,
        }}
      >
        <Typography variant="h5">{t('home.pages.simple.welcome')}</Typography>
        <Typography color="text.secondary" sx={{ textAlign: 'center' }}>
          {t('home.pages.simple.welcomeHint')}
        </Typography>
        <Stack sx={{ gap: 0.5, width: '100%', maxWidth: 480 }}>
          <Stack direction="row" sx={{ gap: 1 }}>
            <TextField
              fullWidth
              size="small"
              value={subUrl}
              onChange={(event) => setSubUrl(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') void addSubscription()
              }}
              placeholder={t('home.pages.simple.subscriptionPlaceholder')}
            />
            <Button
              variant="contained"
              disabled={!subUrl.trim()}
              onClick={() => void addSubscription()}
            >
              {t('shared.actions.new')}
            </Button>
          </Stack>
          {/* clod:chan — та же галочка, что в диалоге добавления подписки. */}
          <FormControlLabel
            sx={{ ml: 0.5, mr: 0 }}
            control={
              <Checkbox
                size="small"
                checked={subSecure}
                onChange={(event) => setSubSecure(event.target.checked)}
              />
            }
            label={
              <Typography variant="body2" color="text.secondary">
                {t('profiles.modals.profileForm.fields.secureChannel')}
              </Typography>
            }
          />
        </Stack>
        {/* clod:design-v2 — боковой колонки нет: и расширенный режим, и сами
            настройки должны оставаться достижимы ещё до первой подписки. */}
        <Stack direction="row" sx={{ gap: 1 }}>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => void navigate('/settings')}
          >
            {t('layout.components.navigation.tabs.settings')}
          </Button>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => void setSimpleMode(false)}
          >
            {t('home.pages.simple.toAdvanced')}
          </Button>
        </Stack>
      </Stack>
    )
  }

  return (
    // clod:fit-window — окно садится по высоте этого содержимого; ref нужен
    // именно на прокручиваемом корне: его `scrollHeight` и есть полная высота.
    <Stack ref={fitRef} sx={{ height: '100%', overflowY: 'auto' }}>
      {/* One column at every window size: it fills a narrow window and stays
          a readable centred strip on a large display. */}
      {/* clod: gap 1.5, не 2 — после появления строки режима под кнопкой
          контент перестал влезать в simple-окно и вылезала прокрутка */}
      {/* clod: `1 0 auto`, а не `1` — с базой 0 колонка упиралась в высоту
          окна и лишнее не прокручивалось, а сжимало карточки (тот же дефект,
          что в расширенном режиме). */}
      {/* clod:fit-window — плотная раскладка включается ТОЛЬКО когда окно уже
          упёрлось в рабочую область экрана: это последняя попытка обойтись
          без прокрутки. */}
      <Stack
        sx={{
          p: compact ? 1.25 : 2,
          gap: compact ? 1 : 1.5,
          width: '100%',
          maxWidth: 520,
          mx: 'auto',
          flex: '1 0 auto',
        }}
      >
        <ProviderHeader profile={current} showSettings />

        <ProviderBanners profile={current} onChanged={mutateProfiles} />

        <Box
          sx={{
            display: 'flex',
            justifyContent: 'center',
            pt: compact ? 0 : 0.5,
          }}
        >
          <ConnectButton
            state={state}
            uptime={uptime}
            errorText={errorText}
            compact={compact}
            onToggle={() => void toggle()}
          />
        </Box>

        {/* clod: и в простом режиме видно, что дёргает Connect (системный
            прокси / TUN / оба) и какой режим маршрутизации активен */}
        <Box sx={{ display: 'flex', justifyContent: 'center', mt: -1 }}>
          <ModeStatus locked={Boolean(current.lock_mode)} />
        </Box>

        {/* clod:tun-ready — в простом режиме карточки быстрых действий нет, а
            объяснить «TUN не поднялся» и дать кнопку починки надо и здесь */}
        <Box sx={{ display: 'flex', justifyContent: 'center' }}>
          <TunStatus />
        </Box>

        {/* clod: session totals (downloaded/uploaded since the core started) */}
        <SessionTraffic />

        <ServerSelectRow onOpen={() => setServerOpen(true)} />
        <ServerSelect open={serverOpen} onClose={() => setServerOpen(false)} />

        <SubscriptionCard profile={current} />

        {/* clod:provider-links — все ссылки провайдера одной строкой; кабинет
            и поддержка переехали сюда же, чтобы кнопки провайдера не были
            разбросаны по экрану в двух разных видах. */}
        <ProviderLinksCard profile={current} compact={compact} />

        {/* clod:design-v2 — the mockups' footlink into the advanced mode */}
        <Box sx={{ mt: 'auto', textAlign: 'center', pt: 1 }}>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => void setSimpleMode(false)}
          >
            {t('home.pages.simple.toAdvanced')}
          </Button>
        </Box>
      </Stack>
    </Stack>
  )
}

export default HomeSimplePage
