import { alpha, styled } from '@mui/material/styles'
import { default as MuiSwitch, SwitchProps } from '@mui/material/Switch'

/**
 * clod: единственный тумблер приложения.
 *
 * Раньше здесь жил крупный iOS-образный переключатель (трек 42×26 с контуром):
 * в плотных списках настроек он занимал почти всю строку, и соседние строки
 * читались как наехавшие друг на друга. Берём системную геометрию MUI —
 * тонкий трек и бегунок поверх него, — она спокойно вписывается и в карточку
 * быстрых действий, и в диалоги.
 *
 * Меняем ровно одно: выключенное состояние на светлой теме. Дефолтные 38%
 * прозрачности на белой карточке читаются как пустое место — поднимаем до
 * уровня, на котором тумблер видно, но он не спорит с включённым (тот в цвете
 * акцента и всегда заметнее).
 */
export const Switch = styled((props: SwitchProps) => (
  <MuiSwitch focusVisibleClassName=".Mui-focusVisible" {...props} />
))(({ theme }) => {
  const light = theme.palette.mode === 'light'

  return {
    '& .MuiSwitch-switchBase:not(.Mui-checked)': {
      '& + .MuiSwitch-track': {
        backgroundColor: alpha(theme.palette.text.primary, light ? 0.42 : 0.5),
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
  }
})
