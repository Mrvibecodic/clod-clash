const WS_ERROR_PREFIX = 'Websocket error'

export const isWsErrorMessage = (data: string) =>
  data.startsWith(WS_ERROR_PREFIX)
