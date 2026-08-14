import SupportAgentRoundedIcon from '@mui/icons-material/SupportAgentRounded'
import { Box, Collapse, IconButton, Stack } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BasePage } from '@/components/base'
import { SettingItem } from '@/components/setting/mods/setting-comp'
import SettingClash from '@/components/setting/setting-clash'
import { SettingProviderLinks } from '@/components/setting/setting-provider-links'
import SettingSystem from '@/components/setting/setting-system'
import SettingTools from '@/components/setting/setting-tools'
import SettingVergeAdvanced from '@/components/setting/setting-verge-advanced'
import SettingVergeBasic from '@/components/setting/setting-verge-basic'
import { useProfiles } from '@/hooks/use-profiles'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useThemeMode } from '@/services/states'

const SettingPage = () => {
  const { t } = useTranslation()

  const onError = (err: any) => {
    showNotice.error(err)
  }

  // clod:branding — the upstream Telegram/GitHub/manual buttons linked to the
  // upstream project; a white-label build can only point at the provider's
  // own support channel, which comes from the subscription.
  const { current } = useProfiles()
  const supportUrl = current?.support_url

  const toSupport = useLockFn(async () => {
    if (!supportUrl) return
    try {
      await openWebUrl(supportUrl)
    } catch (error) {
      showNotice.error(error)
    }
  })

  const mode = useThemeMode()
  const isDark = mode === 'light' ? false : true

  // clod:simple-settings — раскрытие живёт в состоянии страницы, а не в
  // настройках: это не выбор пользователя, который надо помнить, а разовый
  // заход «покажи всё». Уход со страницы сворачивает блок обратно.
  const [showAdvanced, setShowAdvanced] = useState(false)
  // clod:provider-links — «Основные» тоже свёрнуты по умолчанию: язык и тему
  // задают один раз, а первым на экране должно стоять то, за чем сюда
  // заходят чаще, — система и ссылки провайдера.
  const [showBasic, setShowBasic] = useState(false)

  const card = {
    borderRadius: 2,
    backgroundColor: isDark ? '#282a36' : '#ffffff',
  }

  return (
    <BasePage
      title={t('settings.page.title')}
      header={
        supportUrl ? (
          <IconButton
            size="medium"
            color="inherit"
            title={t('profiles.components.hwidDialog.support')}
            onClick={() => void toSupport()}
          >
            <SupportAgentRoundedIcon fontSize="inherit" />
          </IconButton>
        ) : null
      }
    >
      {/* clod:simple-settings — одна колонка «сверху повседневное, ниже —
          всё остальное под одной крышкой». ТЗ F3.3 требовало спрятать
          технические пункты, но спрятать — не значит отобрать: они уезжают под
          «Продвинутые настройки», а не исчезают.

          Раскладка ОДНА на оба режима интерфейса. Раньше перегруппировка
          работала только в простом — а в него ещё и не заходят те, кто уже
          переключился в расширенный, так что вся работа была для них невидима.
          Разные экраны настроек под одним названием — это к тому же две разные
          карты одного и того же места: человек, который однажды нашёл пункт,
          после смены режима искал бы его заново. */}
      <Stack spacing={1.5} sx={{ maxWidth: 720, mx: 'auto', width: '100%' }}>
        <Box sx={card}>
          <SettingSystem onError={onError} />
        </Box>

        {/* clod:provider-links — ссылки текущей подписки. Своя карточка, а не
            строки внутри чужой группы: они принадлежат провайдеру, а не
            приложению. */}
        <Box sx={card}>
          <SettingProviderLinks />
        </Box>

        <Box sx={card}>
          <SettingItem
            label={t('settings.components.verge.basic.title')}
            secondary={t('settings.sections.basicGroup.hint')}
            expanded={showBasic}
            onClick={() => setShowBasic((open) => !open)}
          />
          <Collapse in={showBasic} unmountOnExit>
            <SettingVergeBasic onError={onError} variant="core" />
            {/* Отчёт для поддержки и версия — продолжение той же карточки, без
                второго заголовка: без них пользователь не может ни сказать, что
                у него за сборка, ни попросить помощь. */}
            <SettingVergeAdvanced onError={onError} variant="core" />
          </Collapse>
        </Box>

        <Box sx={card}>
          <SettingItem
            label={t('settings.sections.advancedGroup.title')}
            secondary={t('settings.sections.advancedGroup.hint')}
            expanded={showAdvanced}
            onClick={() => setShowAdvanced((open) => !open)}
          />
          {/* Внутри — не литой список, а четыре подписанные группы:
              оформление и поведение, ядро, инструменты (прокси/правила/
              соединения/логи), продвинутое. */}
          <Collapse in={showAdvanced} unmountOnExit>
            <SettingVergeBasic onError={onError} variant="rest" />
            <SettingClash onError={onError} />
            <SettingTools />
            <SettingVergeAdvanced onError={onError} variant="rest" />
          </Collapse>
        </Box>
      </Stack>
    </BasePage>
  )
}

export default SettingPage
