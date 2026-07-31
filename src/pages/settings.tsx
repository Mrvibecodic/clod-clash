import SupportAgentRoundedIcon from '@mui/icons-material/SupportAgentRounded'
import { Box, IconButton, Stack } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useTranslation } from 'react-i18next'

import { BasePage } from '@/components/base'
import SettingClash from '@/components/setting/setting-clash'
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
      {/* clod: одна колонка «сверху простое, снизу сложное»:
          Система → Основные → Ядро → Инструменты → Расширенные */}
      <Stack
        spacing={1.5}
        sx={{ maxWidth: 720, mx: 'auto', width: '100%' }}
      >
        <Box
          sx={{
            borderRadius: 2,
            backgroundColor: isDark ? '#282a36' : '#ffffff',
          }}
        >
          <SettingSystem onError={onError} />
        </Box>
        <Box
          sx={{
            borderRadius: 2,
            backgroundColor: isDark ? '#282a36' : '#ffffff',
          }}
        >
          <SettingVergeBasic onError={onError} />
        </Box>
        <Box
          sx={{
            borderRadius: 2,
            backgroundColor: isDark ? '#282a36' : '#ffffff',
          }}
        >
          <SettingClash onError={onError} />
        </Box>
        {/* clod:design-v2 — proxies/rules/connections/logs entrances,
            moved here from the advanced home tiles */}
        <Box
          sx={{
            borderRadius: 2,
            backgroundColor: isDark ? '#282a36' : '#ffffff',
          }}
        >
          <SettingTools />
        </Box>
        <Box
          sx={{
            borderRadius: 2,
            backgroundColor: isDark ? '#282a36' : '#ffffff',
          }}
        >
          <SettingVergeAdvanced onError={onError} />
        </Box>
      </Stack>
    </BasePage>
  )
}

export default SettingPage
