import { Box } from '@mui/material'

import { countryFromName, flagSrc } from '@/utils/country'

interface Props {
  /** Node or location name the country is guessed from. */
  name?: string
  size?: number
}

/**
 * Round country flag for a proxy node.
 *
 * The images are the bundled circle-flags set (public/flags): emoji flags are
 * not an option because Windows renders them as bare letters, and a remote
 * flag CDN is not an option for a client whose whole point is that the network
 * may be hostile. An unrecognised name gets the neutral placeholder.
 */
export const CountryFlag = ({ name, size = 22 }: Props) => {
  const code = name ? countryFromName(name) : undefined
  return (
    <Box
      component="img"
      src={flagSrc(code)}
      alt=""
      width={size}
      height={size}
      sx={{
        borderRadius: '50%',
        flex: 'none',
        display: 'block',
        boxShadow: (theme) => `0 0 0 1px ${theme.palette.divider}`,
      }}
    />
  )
}
