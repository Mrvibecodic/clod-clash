import { Box, Typography } from '@mui/material'

import RobotLogo from '@/assets/image/logo-robot.svg?react'
import { useVerge } from '@/hooks/use-verge'

/** Display name when the white-label config sets nothing. */
const DEFAULT_BRAND_NAME = 'Clod Clash'

/**
 * The application brand in the sidebar: mark + name.
 *
 * Both come from the white-label config (`brand_logo` / `brand_name` in
 * verge.yaml), so a provider build can be re-skinned without touching the
 * code: the name is plain text, the logo is any image the config points at
 * (data: URL or bundled file). The defaults are the robot placeholder and
 * the product name.
 */
export const AppBrand = ({ isDark }: { isDark: boolean }) => {
  const { verge } = useVerge()
  const name = verge?.brand_name?.trim() || DEFAULT_BRAND_NAME
  const logo = verge?.brand_logo

  return (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 1,
        minWidth: 0,
        color: isDark ? '#fff' : '#000',
      }}
    >
      {/* inline styles on purpose: `.the-logo img, svg { width: 100% }` from
          the layout stylesheet would override attribute/class sizing */}
      {logo ? (
        <Box
          component="img"
          src={logo}
          alt=""
          style={{
            width: 30,
            height: 30,
            flex: 'none',
            objectFit: 'contain',
          }}
        />
      ) : (
        <RobotLogo style={{ width: 30, height: 30, flex: 'none' }} />
      )}
      <Typography
        noWrap
        sx={{
          fontSize: 19,
          fontWeight: 700,
          letterSpacing: 0.2,
          lineHeight: 1,
        }}
      >
        {name}
      </Typography>
    </Box>
  )
}
