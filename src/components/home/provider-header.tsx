import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import {
  Avatar,
  CircularProgress,
  IconButton,
  Stack,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useProfiles } from '@/hooks/use-profiles'
import { updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

interface Props {
  profile: IProfileItem
}

/** Provider identity row: logo, plan name, subscription refresh. */
export const ProviderHeader = ({ profile }: Props) => {
  const { t } = useTranslation()
  const { mutateProfiles } = useProfiles()
  const [refreshing, setRefreshing] = useState(false)

  const refresh = useLockFn(async () => {
    if (!profile.uid) return
    setRefreshing(true)
    try {
      await updateProfile(profile.uid)
      await mutateProfiles()
    } catch (error) {
      showNotice.error(error)
    } finally {
      setRefreshing(false)
    }
  })

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
      {profile.logo ? (
        <Avatar src={profile.logo} alt="" sx={{ width: 40, height: 40 }} />
      ) : null}
      <Typography variant="h6" noWrap sx={{ flex: 1, minWidth: 0 }}>
        {profile.name}
      </Typography>
      <IconButton
        onClick={() => void refresh()}
        disabled={refreshing}
        aria-label={t('shared.actions.refresh')}
      >
        {refreshing ? <CircularProgress size={20} /> : <RefreshRoundedIcon />}
      </IconButton>
    </Stack>
  )
}
