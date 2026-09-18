import {
  decodeAndTrim,
  decodeBase64OrOriginal,
  getCipher,
  getIfNotBlank,
  getIfPresent,
  normalizeHost,
  parseBoolOrPresence,
  parseQueryString,
  parseRequiredPort,
  pickKnownKeys,
  pickStringMap,
  splitOnce,
  stripUriScheme,
} from './helpers'

const V2RAY_PLUGIN_OPTS_KEYS = [
  'mode',
  'host',
  'path',
  'tls',
  'ech-opts',
  'fingerprint',
  'certificate',
  'private-key',
  'headers',
  'skip-cert-verify',
  'name-cert-verify',
  'mux',
  'v2ray-http-upgrade',
  'v2ray-http-upgrade-fast-open',
] as const

export function URI_SS(line: string): IProxyShadowsocksConfig {
  const afterScheme = stripUriScheme(line, 'ss', 'Invalid ss uri')
  if (!afterScheme) {
    throw new Error('Invalid ss uri')
  }

  const [withoutHash, hashRaw] = splitOnce(afterScheme, '#')
  const nameFromHash = decodeAndTrim(hashRaw)

  const [mainRaw, queryRaw] = splitOnce(withoutHash, '?')
  const queryParams = parseQueryString(queryRaw)

  const main = mainRaw.includes('@') ? mainRaw : decodeBase64OrOriginal(mainRaw)
  const atIdx = main.lastIndexOf('@')
  if (atIdx === -1) {
    throw new Error("Invalid ss uri: missing '@'")
  }

  const userInfoStr = decodeBase64OrOriginal(main.slice(0, atIdx))
  const serverAndPortWithPath = main.slice(atIdx + 1)
  const serverAndPort = serverAndPortWithPath.split('/')[0]

  const portIdx = serverAndPort.lastIndexOf(':')
  if (portIdx === -1) {
    throw new Error('Invalid ss uri: missing port')
  }
  const server = normalizeHost(serverAndPort.slice(0, portIdx))
  const portRaw = serverAndPort.slice(portIdx + 1)
  const port = parseRequiredPort(portRaw, 'Invalid ss uri: invalid port')

  const userInfo = userInfoStr.match(/(^.*?):(.*$)/)

  const proxy: IProxyShadowsocksConfig = {
    name: nameFromHash ?? `SS ${server}:${port}`,
    type: 'ss',
    server,
    port,
    cipher: getCipher(userInfo?.[1]),
    password: userInfo?.[2],
  }

  // plugin from `plugin=...`
  const pluginParam = queryParams.plugin
  if (pluginParam) {
    const pluginParts = pluginParam.split(';')
    const pluginName = pluginParts[0]
    const pluginOptions: Record<string, any> = { plugin: pluginName }
    for (const raw of pluginParts.slice(1)) {
      if (!raw) continue
      const [key, val] = splitOnce(raw, '=')
      if (!key) continue
      pluginOptions[key] = val === undefined || val === '' ? true : val
    }

    switch (pluginOptions.plugin) {
      case 'obfs-local':
      case 'simple-obfs':
        proxy.plugin = 'obfs'
        proxy['plugin-opts'] = {
          mode: pluginOptions.obfs,
          host: getIfNotBlank(pluginOptions['obfs-host']),
        }
        break
      case 'v2ray-plugin':
        proxy.plugin = 'v2ray-plugin'
        proxy['plugin-opts'] = {
          mode: 'websocket',
          host: getIfNotBlank(pluginOptions['obfs-host'] ?? pluginOptions.host),
          path: getIfNotBlank(pluginOptions.path),
          tls: getIfPresent(pluginOptions.tls),
        }
        break
      default:
        throw new Error(`Unsupported plugin option: ${pluginOptions.plugin}`)
    }
  }

  // plugin from `v2ray-plugin=...` (base64 JSON)
  const v2rayPluginParam = queryParams['v2ray-plugin']
  if (!proxy.plugin && v2rayPluginParam) {
    proxy.plugin = 'v2ray-plugin'
    try {
      const opts = pickKnownKeys(
        JSON.parse(decodeBase64OrOriginal(v2rayPluginParam)),
        V2RAY_PLUGIN_OPTS_KEYS,
      )
      if ('headers' in opts) {
        const headers = pickStringMap(opts.headers)
        if (Object.keys(headers).length > 0) opts.headers = headers
        else delete opts.headers
      }
      proxy['plugin-opts'] = opts
    } catch (e) {
      console.warn('[URI_SS] v2ray-plugin JSON.parse failed:', e)
      proxy['plugin-opts'] = {}
    }
  }

  if (
    Object.prototype.hasOwnProperty.call(queryParams, 'uot') &&
    parseBoolOrPresence(queryParams.uot)
  ) {
    proxy['udp-over-tcp'] = true
  }
  if (
    Object.prototype.hasOwnProperty.call(queryParams, 'tfo') &&
    parseBoolOrPresence(queryParams.tfo)
  ) {
    proxy.tfo = true
  }

  return proxy
}
