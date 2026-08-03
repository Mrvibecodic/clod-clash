import { alpha, styled } from '@mui/material/styles'
import { default as MuiSwitch, SwitchProps } from '@mui/material/Switch'

/**
 * clod: выключенный тумблер раньше был серым треком (#BBBBBB) с белым
 * бегунком — на светлой теме, где карточка белая, он читался как пустое
 * место. Теперь у выключенного прозрачный трек с контуром и бегунок цветом
 * контура: видно на любом фоне, при любом акценте и в обеих темах. Включённый
 * вид не менялся — он и так заметен, он в цвете акцента.
 */
export const Switch = styled((props: SwitchProps) => (
  <MuiSwitch
    focusVisibleClassName=".Mui-focusVisible"
    disableRipple
    {...props}
  />
))(({ theme }) => {
  const outline =
    theme.palette.mode === 'light'
      ? alpha(theme.palette.text.primary, 0.36)
      : alpha(theme.palette.text.primary, 0.42)

  return {
    width: 42,
    height: 26,
    padding: 0,
    marginRight: 1,
    '& .MuiSwitch-switchBase': {
      padding: 0,
      // Выключенный бегунок меньше и сидит с отступом от контура.
      margin: 4,
      transitionDuration: '300ms',
      '&.Mui-checked': {
        transform: 'translateX(16px)',
        margin: 2,
        color: '#fff',
        '& + .MuiSwitch-track': {
          backgroundColor: theme.palette.primary.main,
          borderColor: theme.palette.primary.main,
          opacity: 1,
        },
        '&.Mui-disabled + .MuiSwitch-track': {
          opacity: 0.5,
        },
      },
      '&.Mui-focusVisible + .MuiSwitch-track': {
        boxShadow: `0 0 0 3px ${alpha(theme.palette.primary.main, 0.25)}`,
      },
      '&.Mui-disabled .MuiSwitch-thumb': {
        color:
          theme.palette.mode === 'light'
            ? theme.palette.grey[300]
            : theme.palette.grey[700],
      },
      '&.Mui-disabled + .MuiSwitch-track': {
        opacity: theme.palette.mode === 'light' ? 0.5 : 0.4,
      },
    },
    '& .MuiSwitch-thumb': {
      boxSizing: 'border-box',
      width: 18,
      height: 18,
      color: outline,
      boxShadow: 'none',
      transition: theme.transitions.create(['width', 'height', 'color'], {
        duration: 200,
      }),
    },
    '& .Mui-checked .MuiSwitch-thumb': {
      width: 22,
      height: 22,
      color: '#fff',
    },
    '& .MuiSwitch-track': {
      borderRadius: 26 / 2,
      backgroundColor: 'transparent',
      border: `1.5px solid ${outline}`,
      opacity: 1,
      transition: theme.transitions.create(
        ['background-color', 'border-color'],
        { duration: 300 },
      ),
    },
  }
})
