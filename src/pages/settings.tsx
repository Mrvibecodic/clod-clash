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
import { CARD_SURFACE } from '@/pages/_theme'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

const SettingPage = () => {
  const { t } = useTranslation()

  const onError = (err: any) => {
    showNotice.error(err)
  }

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

  const [showAdvanced, setShowAdvanced] = useState(false)
  const [showBasic, setShowBasic] = useState(false)

  const card = CARD_SURFACE

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
      <Stack spacing={1.5} sx={{ maxWidth: 720, mx: 'auto', width: '100%' }}>
        <Box sx={card}>
          <SettingSystem onError={onError} />
        </Box>

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
