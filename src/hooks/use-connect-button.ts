import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { type ConnectFailure, connectState } from '@/utils/connect-state'
import { connectFailureText } from '@/utils/tun-notice'

import { useConnectTargets } from './use-connect-targets'

/**
 * Кнопка «Подключить» на Главной (обе раскладки): что она показывает
 * (`connectState`) и что делает по нажатию.
 */
export const useConnectButton = () => {
  const { t } = useTranslation()
  const { connected, willConnect, toggleConnection } = useConnectTargets()

  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<ConnectFailure>()
  const [intent, setIntent] = useState<'connecting' | 'disconnecting'>()

  const { state, errorText } = connectState({
    failure,
    busy,
    intent,
    connected,
  })

  const toggle = useLockFn(async () => {
    setIntent(willConnect ? 'connecting' : 'disconnecting')
    setBusy(true)
    setFailure(undefined)
    try {
      await toggleConnection()
    } catch (error) {
      setFailure({ text: connectFailureText(error, t), at: connected })
    } finally {
      setBusy(false)
      setIntent(undefined)
    }
  })

  return { connected, state, errorText, toggle }
}
