type Platform =
  | 'aix'
  | 'android'
  | 'darwin'
  | 'freebsd'
  | 'haiku'
  | 'linux'
  | 'openbsd'
  | 'sunos'
  | 'win32'
  | 'cygwin'
  | 'netbsd'

declare const OS_PLATFORM: Platform

type ValidationOutcome =
  | { status: 'valid' | 'busy' }
  | { status: 'invalid'; kind: string; message: string }
  | { status: 'skipped'; reason: string }

interface DnsSaveOutcome {
  saved: boolean
  validation: ValidationOutcome
}

interface IConfigData {
  port: number
  mode: string
  ipv6: boolean
  'socket-port': number
  'allow-lan': boolean
  'log-level'?: string
  'mixed-port': number
  'redir-port': number
  'socks-port': number
  'tproxy-port': number
  'external-controller': string
  'external-controller-cors': {
    'allow-private-network': boolean
    'allow-origins': string[]
  }
  secret: string
  'unified-delay'?: boolean | 'auto'
  tun: {
    stack?: string
    device: string
    'auto-route': boolean
    'auto-redirect'?: boolean
    'auto-detect-interface': boolean
    'dns-hijack'?: string[]
    'route-exclude-address'?: string[]
    'strict-route'?: boolean
    mtu: number
  }
  dns?: {
    enable?: boolean
    listen?: string
    'enhanced-mode'?: 'fake-ip' | 'redir-host'
    'fake-ip-range'?: string
    'fake-ip-range6'?: string
    'fake-ip-filter'?: string[]
    'fake-ip-filter-mode'?: 'blacklist' | 'whitelist'
    'prefer-h3'?: boolean
    'respect-rules'?: boolean
    nameserver?: string[]
    fallback?: string[]
    'default-nameserver'?: string[]
    'proxy-server-nameserver'?: string[]
    'direct-nameserver'?: string[]
    'direct-nameserver-follow-policy'?: boolean
    'nameserver-policy'?: Record<string, any>
    'use-hosts'?: boolean
    'use-system-hosts'?: boolean
    'fallback-filter'?: {
      geoip?: boolean
      'geoip-code'?: string
      ipcidr?: string[]
      domain?: string[]
    }
  }
  tunnels?: {
    network: string[]
    address: string
    target: string
    proxy?: string
  }[]
  'proxy-groups'?: IProxyGroupItem[]
}

interface IProxyItem {
  name: string
  type: string
  udp: boolean
  xudp: boolean
  tfo: boolean
  mptcp: boolean
  smux: boolean
  history: {
    time: string
    delay: number
  }[]
  testUrl?: string
  extra?: Record<
    string,
    { alive?: boolean; history?: { time: string; delay: number }[] } | undefined
  >
  all?: string[]
  now?: string
  hidden?: boolean
  icon?: string
  provider?: string
  fixed?: string
}

type IProxyGroupItem = Omit<IProxyItem, 'all'> & {
  all: IProxyItem[]
}

interface IProxyProviderItem {
  name: string
  type: string
  proxies: IProxyItem[]
  updatedAt: string
  vehicleType: string
  subscriptionInfo?: {
    Upload: number
    Download: number
    Total: number
    Expire: number
  }
}

interface IRuleProviderItem {
  name: string
  behavior: string
  format: string
  ruleCount: number
  type: string
  updatedAt: string
  vehicleType: string
}

interface ITrafficEstimate {
  profile: string
  baselineUpload: number
  baselineDownload: number
  localBytes: number
  baselineAt: number
}

interface ITrafficItem {
  up: number
  down: number
  upTotal?: number
  downTotal?: number
}

interface ILogItem {
  type: string
  time?: string
  payload: string
}

type LogLevel = import('tauri-plugin-mihomo-api').LogLevel
type LogFilter = 'all' | 'debug' | 'info' | 'warn' | 'err'
type LogOrder = 'asc' | 'desc'

interface IClashLog {
  enable: boolean
  logLevel: LogLevel
  logFilter: LogFilter
  logOrder: LogOrder
}

interface IConnectionsItem {
  id: string
  metadata: {
    network: string
    type: string
    host: string
    sourceIP: string
    sourcePort: string
    destinationPort: string
    destinationIP?: string
    remoteDestination?: string
    process?: string
    processPath?: string
  }
  upload: number
  download: number
  start: string
  chains: string[]
  rule: string
  rulePayload: string
  curUpload?: number
  curDownload?: number
}

interface IConnections {
  downloadTotal: number
  uploadTotal: number
  connections: IConnectionsItem[]
}

type IConnectionGroupBy = 'none' | 'process' | 'chain' | 'rule'

interface IConnectionSetting {
  layout: 'table' | 'list'
  groupBy?: IConnectionGroupBy
  summary?: boolean
}

interface IClashInfo {
  mixed_port?: number
  socks_port?: number
  port?: number
  server?: string
  secret?: string
}

interface IProfileItem {
  uid: string
  type?: 'local' | 'remote' | 'merge' | 'script'
  name?: string
  desc?: string
  file?: string
  url?: string
  updated?: number
  selected?: {
    name?: string
    now?: string
  }[]
  extra?: {
    upload: number
    download: number
    total: number
    expire: number
  }
  option?: IProfileOption
  home?: string
  group?: string
  support_url?: string
  logo?: string
  announce?: string
  announce_url?: string
  portal_url?: string
  bot_url?: string
  monitor_url?: string
  guide_url?: string
  promo?: string
  promo_url?: string
  promo_seen?: boolean
  lock_mode?: boolean
  connect_mode?: 'tun' | 'proxy' | 'both'
  theme_accent?: string
  theme_mode?: 'light' | 'dark'
  theme_background?: string
  latency_style?: 'bars' | 'dot' | 'number'
  disable_ping?: boolean
  device_remove_url?: string
  show_zero_hosts?: boolean
  refill_date?: number
  clock_skew?: number
  clock_skew_at?: number
  interval_locked?: boolean
  fallback_url?: string
  fallback_domain?: string
  previous_urls?: string[]
  migration_hops?: number
  hwid_state?: 'ok' | 'limit' | 'not_supported'
  hwid_max_devices?: number
  name_customized?: boolean
  notify_expire_days?: number[]
  notify_traffic_percent?: number[]
  notified?: Record<string, number>
  from_fallback?: boolean
  /** Подписка скачалась, но ядро её не приняло — на диске прежний конфиг. */
  not_applied?: boolean
  simple_mode?: boolean
  favorites?: string[]
}

interface IProfileOption {
  user_agent?: string
  with_proxy?: boolean
  self_proxy?: boolean
  secure?: boolean
  chan_pin?: string
  update_interval?: number
  timeout_seconds?: number
  danger_accept_invalid_certs?: boolean
  allow_auto_update?: boolean
  merge?: string
  script?: string
  rules?: string
  proxies?: string
  groups?: string
}

interface IProfilesConfig {
  current?: string
  items?: IProfileItem[]
}

interface IAddress {
  V4?: {
    ip: string
    broadcast?: string
    netmask?: string
  }
  V6?: {
    ip: string
    broadcast?: string
    netmask?: string
  }
}
interface INetworkInterface {
  name: string
  addr: IAddress[]
  mac_addr?: string
  index: number
}

interface ISeqProfileConfig {
  prepend: []
  append: []
  delete: []
}

interface IProxyGroupConfig {
  name: string
  type: 'select' | 'url-test' | 'fallback' | 'load-balance' | 'relay'
  proxies?: string[]
  use?: string[]
  url?: string
  interval?: number
  lazy?: boolean
  timeout?: number
  'max-failed-times'?: number
  'disable-udp'?: boolean
  'interface-name': string
  'routing-mark'?: number
  'include-all'?: boolean
  'include-all-proxies'?: boolean
  'include-all-providers'?: boolean
  filter?: string
  'exclude-filter'?: string
  'exclude-type'?: string
  'expected-status'?: string
  hidden?: boolean
  icon?: string
}

interface WsOptions {
  path?: string
  headers?: {
    [key: string]: string
  }
  'max-early-data'?: number
  'early-data-header-name'?: string
  'v2ray-http-upgrade'?: boolean
  'v2ray-http-upgrade-fast-open'?: boolean
}

interface HttpOptions {
  method?: string
  path?: string[]
  headers?: {
    [key: string]: string[]
  }
}

interface H2Options {
  path?: string
  host?: string
}

interface GrpcOptions {
  'grpc-service-name'?: string
}

interface XHttpOptions {
  path?: string
  host?: string
  mode?: string
}

interface RealityOptions {
  'public-key'?: string
  'short-id'?: string
}
type ClientFingerprint =
  | 'chrome'
  | 'firefox'
  | 'safari'
  | 'iOS'
  | 'android'
  | 'edge'
  | '360'
  | 'qq'
  | 'random'
type NetworkType = 'ws' | 'http' | 'h2' | 'grpc' | 'tcp' | 'xhttp'
type CipherType =
  | 'none'
  | 'auto'
  | 'dummy'
  | 'aes-128-gcm'
  | 'aes-192-gcm'
  | 'aes-256-gcm'
  | 'lea-128-gcm'
  | 'lea-192-gcm'
  | 'lea-256-gcm'
  | 'aes-128-gcm-siv'
  | 'aes-256-gcm-siv'
  | '2022-blake3-aes-128-gcm'
  | '2022-blake3-aes-256-gcm'
  | 'aes-128-cfb'
  | 'aes-192-cfb'
  | 'aes-256-cfb'
  | 'aes-128-ctr'
  | 'aes-192-ctr'
  | 'aes-256-ctr'
  | 'chacha20'
  | 'chacha20-ietf'
  | 'chacha20-ietf-poly1305'
  | '2022-blake3-chacha20-poly1305'
  | 'rabbit128-poly1305'
  | 'xchacha20-ietf-poly1305'
  | 'xchacha20'
  | 'aegis-128l'
  | 'aegis-256'
  | 'aez-384'
  | 'deoxys-ii-256-128'
  | 'rc4-md5'
type MieruTransport = 'TCP' | 'UDP'
type MieruMultiplexing =
  | 'MULTIPLEXING_OFF'
  | 'MULTIPLEXING_LOW'
  | 'MULTIPLEXING_MIDDLE'
  | 'MULTIPLEXING_HIGH'
type SudokuAeadMethod = 'chacha20-poly1305' | 'aes-128-gcm' | 'none'
type SudokuTableType = 'prefer_ascii' | 'prefer_entropy'
type SudokuHttpMaskMode = 'legacy' | 'stream' | 'poll' | 'auto'
type SudokuHttpMaskStrategy = 'random' | 'post' | 'websocket'
interface IProxyBaseConfig {
  tfo?: boolean
  mptcp?: boolean
  'interface-name'?: string
  'routing-mark'?: number
  'ip-version'?: 'dual' | 'ipv4' | 'ipv6' | 'ipv4-prefer' | 'ipv6-prefer'
  'dialer-proxy'?: string
}
interface IProxyDirectConfig extends IProxyBaseConfig {
  name: string
  type: 'direct'
}
interface IProxyDnsConfig extends IProxyBaseConfig {
  name: string
  type: 'dns'
}
interface IProxyHttpConfig extends IProxyBaseConfig {
  name: string
  type: 'http'
  server?: string
  port?: number
  username?: string
  password?: string
  tls?: boolean
  sni?: string
  'skip-cert-verify'?: boolean
  fingerprint?: string
  headers?: {
    [key: string]: string
  }
}
interface IProxySocks5Config extends IProxyBaseConfig {
  name: string
  type: 'socks5'
  server?: string
  port?: number
  username?: string
  password?: string
  tls?: boolean
  udp?: boolean
  'skip-cert-verify'?: boolean
  fingerprint?: string
}
interface IProxySshConfig extends IProxyBaseConfig {
  name: string
  type: 'ssh'
  server?: string
  port?: number
  username?: string
  password?: string
  'private-key'?: string
  'private-key-passphrase'?: string
  'host-key'?: string
  'host-key-algorithms'?: string
}
interface IProxyTrojanConfig extends IProxyBaseConfig {
  name: string
  type: 'trojan'
  server?: string
  port?: number
  password?: string
  alpn?: string[]
  sni?: string
  'skip-cert-verify'?: boolean
  fingerprint?: string
  udp?: boolean
  network?: NetworkType
  'reality-opts'?: RealityOptions
  'grpc-opts'?: GrpcOptions
  'ws-opts'?: WsOptions
  'ss-opts'?: {
    enabled?: boolean
    method?: string
    password?: string
  }
  'client-fingerprint'?: ClientFingerprint
}
interface IProxyAnyTLSConfig extends IProxyBaseConfig {
  name: string
  type: 'anytls'
  server?: string
  port?: number
  password?: string
  alpn?: string[]
  sni?: string
  'client-fingerprint'?: ClientFingerprint
  'skip-cert-verify'?: boolean
  fingerprint?: string
  certificate?: string
  'private-key'?: string
  'ech-opts'?: {
    enable?: boolean
    config?: string
  }
  udp?: boolean
  'idle-session-check-interval'?: number
  'idle-session-timeout'?: number
  'min-idle-session'?: number
}
interface IProxyTuicConfig extends IProxyBaseConfig {
  name: string
  type: 'tuic'
  server?: string
  port?: number
  token?: string
  uuid?: string
  password?: string
  ip?: string
  'heartbeat-interval'?: number
  alpn?: string[]
  'reduce-rtt'?: boolean
  'request-timeout'?: number
  'udp-relay-mode'?: string
  'congestion-controller'?: string
  'disable-sni'?: boolean
  'max-udp-relay-packet-size'?: number
  'fast-open'?: boolean
  'max-open-streams'?: number
  cwnd?: number
  'skip-cert-verify'?: boolean
  fingerprint?: string
  ca?: string
  'ca-str'?: string
  'recv-window-conn'?: number
  'recv-window'?: number
  'disable-mtu-discovery'?: boolean
  'max-datagram-frame-size'?: number
  sni?: string
  'udp-over-stream'?: boolean
  'udp-over-stream-version'?: number
}
interface IProxyMieruConfig extends IProxyBaseConfig {
  name: string
  type: 'mieru'
  server?: string
  port?: number
  'port-range'?: string
  transport?: MieruTransport
  udp?: boolean
  username?: string
  password?: string
  multiplexing?: MieruMultiplexing
  'handshake-mode'?: string
}
interface IProxyMasqueConfig extends IProxyBaseConfig {
  name: string
  type: 'masque'
  server?: string
  port?: number
  'private-key'?: string
  'public-key'?: string
  ip?: string
  ipv6?: string
  mtu?: number
  udp?: boolean
  'remote-dns-resolve'?: boolean
  dns?: string[]
}
interface IProxyVlessConfig extends IProxyBaseConfig {
  name: string
  type: 'vless'
  server?: string
  port?: number
  uuid?: string
  flow?: string
  tls?: boolean
  alpn?: string[]
  udp?: boolean
  'packet-addr'?: boolean
  xudp?: boolean
  'packet-encoding'?: string
  network?: NetworkType
  'reality-opts'?: RealityOptions
  'http-opts'?: HttpOptions
  'h2-opts'?: H2Options
  'grpc-opts'?: GrpcOptions
  'ws-opts'?: WsOptions
  'xhttp-opts'?: XHttpOptions
  'ws-path'?: string
  'ws-headers'?: {
    [key: string]: string
  }
  'skip-cert-verify'?: boolean
  fingerprint?: string
  servername?: string
  'client-fingerprint'?: ClientFingerprint
  smux?: boolean
  encryption?: string
}
interface IProxyVmessConfig extends IProxyBaseConfig {
  name: string
  type: 'vmess'
  server?: string
  port?: number
  uuid?: string
  alterId?: number
  cipher?: CipherType
  udp?: boolean
  network?: NetworkType
  tls?: boolean
  alpn?: string[]
  'skip-cert-verify'?: boolean
  fingerprint?: string
  servername?: string
  'reality-opts'?: RealityOptions
  'http-opts'?: HttpOptions
  'h2-opts'?: H2Options
  'grpc-opts'?: GrpcOptions
  'ws-opts'?: WsOptions
  'packet-addr'?: boolean
  xudp?: boolean
  'packet-encoding'?: string
  'global-padding'?: boolean
  'authenticated-length'?: boolean
  'client-fingerprint'?: ClientFingerprint
  smux?: boolean
}
interface WireGuardPeerOptions {
  server?: string
  port?: number
  'public-key'?: string
  'pre-shared-key'?: string
  reserved?: number[]
  'allowed-ips'?: string[]
}
interface IProxyWireguardConfig extends IProxyBaseConfig, WireGuardPeerOptions {
  name: string
  type: 'wireguard'
  ip?: string
  ipv6?: string
  'private-key'?: string
  workers?: number
  mtu?: number
  udp?: boolean
  'persistent-keepalive'?: number
  peers?: WireGuardPeerOptions[]
  'remote-dns-resolve'?: boolean
  dns?: string[]
  'refresh-server-ip-interval'?: number
}
interface IProxyHysteriaConfig extends IProxyBaseConfig {
  name: string
  type: 'hysteria'
  server?: string
  port?: number
  ports?: string
  protocol?: string
  'obfs-protocol'?: string
  up?: string
  'up-speed'?: number
  down?: string
  'down-speed'?: number
  auth?: string
  'auth-str'?: string
  obfs?: string
  sni?: string
  'skip-cert-verify'?: boolean
  fingerprint?: string
  alpn?: string[]
  ca?: string
  'ca-str'?: string
  'recv-window-conn'?: number
  'recv-window'?: number
  'disable-mtu-discovery'?: boolean
  'fast-open'?: boolean
  'hop-interval'?: number
}
interface IProxyHysteria2Config extends IProxyBaseConfig {
  name: string
  type: 'hysteria2'
  server?: string
  port?: number
  ports?: string
  'hop-interval'?: number
  protocol?: string
  'obfs-protocol'?: string
  up?: string
  down?: string
  password?: string
  obfs?: string
  'obfs-password'?: string
  sni?: string
  'skip-cert-verify'?: boolean
  fingerprint?: string
  alpn?: string[]
  ca?: string
  'ca-str'?: string
  cwnd?: number
  'udp-mtu'?: number
}
interface IProxyShadowsocksConfig extends IProxyBaseConfig {
  name: string
  type: 'ss'
  server?: string
  port?: number
  password?: string
  cipher?: CipherType
  udp?: boolean
  plugin?: 'obfs' | 'v2ray-plugin' | 'shadow-tls' | 'restls'
  'plugin-opts'?: {
    mode?: string
    host?: string
    password?: string
    path?: string
    tls?: string
    fingerprint?: string
    headers?: {
      [key: string]: string
    }
    'skip-cert-verify'?: boolean
    version?: number
    mux?: boolean
    'v2ray-http-upgrade'?: boolean
    'v2ray-http-upgrade-fast-open'?: boolean
    'version-hint'?: string
    'restls-script'?: string
  }
  'udp-over-tcp'?: boolean
  'udp-over-tcp-version'?: number
  'client-fingerprint'?: ClientFingerprint
  smux?: boolean
}
interface IProxySudokuConfig extends IProxyBaseConfig {
  name: string
  type: 'sudoku'
  server?: string
  port?: number
  key?: string
  'aead-method'?: SudokuAeadMethod
  'padding-min'?: number
  'padding-max'?: number
  'table-type'?: SudokuTableType
  'enable-pure-downlink'?: boolean
  'http-mask'?: boolean
  'http-mask-mode'?: SudokuHttpMaskMode
  'http-mask-tls'?: boolean
  'http-mask-host'?: string
  'http-mask-strategy'?: SudokuHttpMaskStrategy
  'custom-table'?: string
  'custom-tables'?: string[]
}
interface IProxyshadowsocksRConfig extends IProxyBaseConfig {
  name: string
  type: 'ssr'
  server?: string
  port?: number
  password?: string
  cipher?: CipherType
  obfs?: string
  'obfs-param'?: string
  protocol?: string
  'protocol-param'?: string
  udp?: boolean
}
interface IProxySmuxConfig {
  smux?: {
    enabled?: boolean
    protocol?: 'smux' | 'yamux' | 'h2mux'
    'max-connections'?: number
    'min-streams'?: number
    'max-streams'?: number
    padding?: boolean
    statistic?: boolean
    'only-tcp'?: boolean
    'brutal-opts'?: {
      enabled?: boolean
      up?: string
      down?: string
    }
  }
}
interface IProxySnellConfig extends IProxyBaseConfig {
  name: string
  type: 'snell'
  server?: string
  port?: number
  psk?: string
  udp?: boolean
  version?: number
}
interface IProxyConfig
  extends IProxyBaseConfig,
    IProxyDirectConfig,
    IProxyDnsConfig,
    IProxyHttpConfig,
    IProxySocks5Config,
    IProxySshConfig,
    IProxyTrojanConfig,
    IProxyAnyTLSConfig,
    IProxyTuicConfig,
    IProxyMieruConfig,
    IProxyMasqueConfig,
    IProxyVlessConfig,
    IProxyVmessConfig,
    IProxyWireguardConfig,
    IProxyHysteriaConfig,
    IProxyHysteria2Config,
    IProxyShadowsocksConfig,
    IProxySudokuConfig,
    IProxyshadowsocksRConfig,
    IProxySmuxConfig,
    IProxySnellConfig {
  type:
    | 'ss'
    | 'ssr'
    | 'direct'
    | 'dns'
    | 'snell'
    | 'http'
    | 'trojan'
    | 'anytls'
    | 'hysteria'
    | 'hysteria2'
    | 'tuic'
    | 'wireguard'
    | 'ssh'
    | 'socks5'
    | 'masque'
    | 'vmess'
    | 'vless'
    | 'mieru'
    | 'sudoku'
}

interface ITunState {
  desired: boolean
  active: boolean
  capable: boolean
  setup_declined: boolean
  needs_repair: boolean
  runtime_stack?: string | null
  failure?: string | null
}

interface ICoreLadder {
  log_level?: string | null
  unified_delay?: boolean | null
}

interface IVergeConfig {
  app_log_level?: 'trace' | 'debug' | 'info' | 'warn' | 'error' | string
  app_log_max_size?: number
  app_log_max_count?: number
  enable_verbose_diagnostics?: boolean
  language?: string
  tray_event?:
    | 'main_window'
    | 'tray_menu'
    | 'system_proxy'
    | 'tun_mode'
    | string
  env_type?: 'bash' | 'cmd' | 'powershell' | 'fish' | string
  startup_script?: string
  start_page?: string
  clash_core?: string
  theme_mode?: 'light' | 'dark' | 'system'
  enable_group_icon?: boolean
  notice_position?: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right'
  tray_icon?: 'monochrome' | 'colorful'
  common_tray_icon?: boolean
  sysproxy_tray_icon?: boolean
  tun_tray_icon?: boolean
  enable_tray_speed?: boolean
  enable_dns_override?: boolean
  tray_proxy_groups_display_mode?: 'default' | 'inline' | 'disable'
  tray_inline_outbound_modes?: boolean
  enable_tun_mode?: boolean
  tun_stack?: string
  tun_strict_route?: string
  tun_dns_hijack?: string
  enable_auto_light_weight_mode?: boolean
  auto_light_weight_minutes?: number
  enable_auto_launch?: boolean
  enable_silent_start?: boolean
  enable_system_proxy?: boolean
  enable_global_hotkey?: boolean
  enable_dns_settings?: boolean
  proxy_auto_config?: boolean
  pac_file_content?: string
  proxy_host?: string
  verge_mixed_port?: number
  verge_socks_port?: number
  verge_redir_port?: number
  verge_tproxy_port?: number
  verge_port?: number
  verge_redir_enabled?: boolean
  verge_tproxy_enabled?: boolean
  verge_socks_enabled?: boolean
  verge_http_enabled?: boolean
  enable_proxy_guard?: boolean
  enable_bypass_check?: boolean
  use_default_bypass?: boolean
  proxy_guard_duration?: number
  system_proxy_bypass?: string
  web_ui_list?: string[]
  hotkeys?: string[]
  theme_setting?: {
    primary_color?: string
    secondary_color?: string
    primary_text?: string
    secondary_text?: string
    info_color?: string
    error_color?: string
    warning_color?: string
    success_color?: string
    font_family?: string
    css_injection?: string
    provider_theme?: boolean
    background_image?: string
    background_blend_mode?: string
    background_opacity?: number
  }
  auto_close_connection?: boolean
  auto_check_update?: boolean
  receive_prereleases?: boolean
  default_latency_test?: string
  default_latency_timeout?: number
  enable_builtin_enhanced?: boolean
  auto_log_clean?: 0 | 1 | 2 | 3 | 4
  enable_auto_backup_schedule?: boolean
  auto_backup_interval_hours?: number
  auto_backup_on_change?: boolean
  proxy_layout_column?: number
  webdav_url?: string
  webdav_username?: string
  webdav_password?: string
  enable_hover_jump_navigator?: boolean
  hover_jump_navigator_delay?: number
  enable_external_controller?: boolean
  enable_hwid?: boolean
  hwid?: string
  simple_mode?: boolean
  connect_system_proxy?: boolean
  connect_tun_mode?: boolean
  connect_on_launch?: boolean
  tun_setup_declined?: string
  window_fit_content?: boolean
  home_tool_shortcuts?: string[]
  use_managed_core?: boolean
  managed_core_channel?: string
  core_auto_check?: boolean
  enable_sub_notifications?: boolean
}

interface IWebDavFile {
  filename: string
  href: string
  last_modified: string
  content_length: number
  content_type: string
  tag: string
}

interface ILocalBackupFile {
  filename: string
  path: string
  last_modified: string
  content_length: number
}

interface IWebDavConfig {
  url: string
  username: string
  password: string
}
