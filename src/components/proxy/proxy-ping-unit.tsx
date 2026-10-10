import { styled } from '@mui/material'
import { useTranslation } from 'react-i18next'

const Unit = styled('span')(({ theme }) => ({
  marginLeft: 3,
  fontSize: 11,
  fontWeight: 400,
  color: theme.palette.text.secondary,
}))

export const PingUnit = () => {
  const { t } = useTranslation()
  return <Unit>{t('shared.units.ms')}</Unit>
}
