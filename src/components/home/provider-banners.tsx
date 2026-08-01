import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
import InfoOutlinedIcon from '@mui/icons-material/InfoOutlined'
import PercentRoundedIcon from '@mui/icons-material/PercentRounded'
import { Alert, Box, IconButton } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import { patchProfile, openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { parseBannerText } from '@/utils/banner-text'

interface Props {
  profile: IProfileItem
  /** Called after the promo dismissal is persisted. */
  onChanged: () => Promise<unknown> | void
}

/**
 * clod: render `#RRGGBB`-marked words in the colour the panel asked for.
 * Uncoloured text keeps the banner's own colour, so a provider that never
 * heard of the syntax sees exactly what it saw before.
 */
const bannerContent = (text: string) =>
  parseBannerText(text).map((fragment, index) => (
    <Box
      component="span"
      key={`${index}-${fragment.text}`}
      sx={fragment.color ? { color: fragment.color, fontWeight: 600 } : null}
    >
      {fragment.text}
    </Box>
  ))

/**
 * The two provider banners of the home screens.
 *
 * `announce` (the `announce` header) is permanent: no close button, it lives
 * exactly as long as the panel keeps sending it. `promo` (the `clod-promo`
 * header) is the opposite — a dismissable, accented banner for temporary
 * campaigns; a changed text brings it back, a header the panel stopped
 * sending removes it on the next subscription update.
 */
export const ProviderBanners = ({ profile, onChanged }: Props) => {
  const { t } = useTranslation()

  const openLink = useCallback(async (url?: string) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error)
    }
  }, [])

  const dismissPromo = useLockFn(async () => {
    if (!profile.uid) return
    try {
      await patchProfile(profile.uid, { promo_seen: true })
      await onChanged()
    } catch (error) {
      showNotice.error(error)
    }
  })

  const showPromo = Boolean(profile.promo) && !profile.promo_seen

  return (
    <>
      {showPromo ? (
        <Alert
          severity="info"
          icon={<PercentRoundedIcon fontSize="inherit" />}
          onClick={() => void openLink(profile.promo_url)}
          sx={{
            whiteSpace: 'pre-line',
            cursor: profile.promo_url ? 'pointer' : 'default',
            border: 1,
            borderColor: 'primary.main',
          }}
          action={
            <IconButton
              size="small"
              aria-label={t('shared.actions.close')}
              onClick={(event) => {
                event.stopPropagation()
                void dismissPromo()
              }}
            >
              <CloseRoundedIcon fontSize="small" />
            </IconButton>
          }
        >
          {bannerContent(profile.promo ?? '')}
        </Alert>
      ) : null}

      {profile.announce ? (
        <Alert
          severity="info"
          icon={<InfoOutlinedIcon fontSize="inherit" />}
          onClick={() => void openLink(profile.announce_url)}
          sx={{
            whiteSpace: 'pre-line',
            cursor: profile.announce_url ? 'pointer' : 'default',
            bgcolor: 'action.hover',
            color: 'text.secondary',
            '& .MuiAlert-icon': { color: 'text.secondary' },
          }}
        >
          {bannerContent(profile.announce ?? '')}
        </Alert>
      ) : null}
    </>
  )
}
