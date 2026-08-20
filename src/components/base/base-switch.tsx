import { alpha, styled } from '@mui/material/styles'
import { default as MuiSwitch, SwitchProps } from '@mui/material/Switch'

export const Switch = styled((props: SwitchProps) => (
  <MuiSwitch
    disableRipple
    focusVisibleClassName=".Mui-focusVisible"
    {...props}
  />
))(({ theme }) => {
  const light = theme.palette.mode === 'light'

  return {
    width: 40,
    height: 22,
    padding: 0,
    '& .MuiSwitch-switchBase': {
      padding: 2,
      '&.Mui-checked': {
        transform: 'translateX(18px)',
        '& .MuiSwitch-thumb': {
          backgroundColor: theme.palette.common.white,
        },
        '& + .MuiSwitch-track': {
          backgroundColor: theme.palette.primary.main,
          opacity: 1,
          border: 0,
        },
        '&.Mui-disabled + .MuiSwitch-track': {
          opacity: light ? 0.4 : 0.3,
        },
      },
      '&:not(.Mui-checked)': {
        '& + .MuiSwitch-track': {
          backgroundColor: alpha(
            theme.palette.text.primary,
            light ? 0.42 : 0.5,
          ),
          opacity: light ? 0.72 : 0.42,
        },
        '& .MuiSwitch-thumb': {
          backgroundColor: light
            ? theme.palette.common.white
            : theme.palette.grey[300],
        },
        '&.Mui-disabled + .MuiSwitch-track': {
          opacity: light ? 0.3 : 0.2,
        },
      },
    },
    '& .MuiSwitch-thumb': {
      boxSizing: 'border-box',
      width: 18,
      height: 18,
      boxShadow: '0 1px 3px rgba(0, 0, 0, 0.3)',
    },
    '& .MuiSwitch-track': {
      borderRadius: 11,
      transition: theme.transitions.create(['background-color'], {
        duration: theme.transitions.duration.short,
      }),
    },
    '&.MuiSwitch-sizeSmall': {
      width: 32,
      height: 18,
      '& .MuiSwitch-switchBase': {
        padding: 2,
        '&.Mui-checked': {
          transform: 'translateX(14px)',
        },
      },
      '& .MuiSwitch-thumb': {
        width: 14,
        height: 14,
      },
      '& .MuiSwitch-track': {
        borderRadius: 9,
      },
    },
  }
})
