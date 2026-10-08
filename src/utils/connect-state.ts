export type ConnectState =
  | 'off'
  | 'connecting'
  | 'disconnecting'
  | 'on'
  | 'error'

/** Провал нажатия и факт подключения, при котором он случился. */
export interface ConnectFailure {
  text: string
  at: boolean
}

/**
 * Что показывает кнопка «Подключить». Ошибка видна, пока факт тот же, что при
 * провале: после неё кнопка не делает вид, будто ничего не было, а сменился
 * факт — ошибка устарела. Ошибка важнее занятости, занятость — факта.
 */
export const connectState = ({
  failure,
  busy,
  intent,
  connected,
}: {
  failure?: ConnectFailure
  busy: boolean
  intent?: 'connecting' | 'disconnecting'
  connected: boolean
}): { state: ConnectState; errorText?: string } => {
  const errorText = failure?.at === connected ? failure.text : undefined
  const state: ConnectState = errorText
    ? 'error'
    : busy
      ? (intent ?? 'connecting')
      : connected
        ? 'on'
        : 'off'
  return { state, errorText }
}
