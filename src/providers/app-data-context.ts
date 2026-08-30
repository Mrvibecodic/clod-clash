import { Context, createContext, use } from 'react'
import {
  BaseConfig,
  ProxyProvider,
  Rule,
  RuleProvider,
} from 'tauri-plugin-mihomo-api'

export interface ProxiesContextType {
  proxies: any
  proxyProviders: Record<string, ProxyProvider | undefined>
}

export interface RulesContextType {
  rules: Rule[]
  ruleProviders: Record<string, RuleProvider | undefined>
}

export interface ClashConfigContextType {
  clashConfig: BaseConfig | undefined
}

export interface SystemContextType {
  sysproxy: any
  systemProxyAddress: string
}

export interface RefreshersContextType {
  refreshProxy: () => Promise<any>
  refreshClashConfig: () => Promise<any>
  refreshRules: () => Promise<any>
  refreshProxyProviders: () => Promise<any>
  refreshRuleProviders: () => Promise<any>
}

export const ProxiesContext = createContext<ProxiesContextType | null>(null)
export const RulesContext = createContext<RulesContextType | null>(null)
export const ClashConfigContext = createContext<ClashConfigContextType | null>(
  null,
)
export const SystemContext = createContext<SystemContextType | null>(null)
export const RefreshersContext = createContext<RefreshersContextType | null>(
  null,
)

const useCtx = <T>(ctx: Context<T | null>, hookName: string): T => {
  const v = use(ctx)
  if (!v) throw new Error(`${hookName} must be used within AppDataProvider`)
  return v
}

export const useProxiesData = () => {
  const { proxies, proxyProviders } = useCtx(ProxiesContext, 'useProxiesData')

  return {
    proxies,
    proxyProviders: proxyProviders as Record<string, ProxyProvider>,
  }
}

export const useRulesData = () => {
  const { rules, ruleProviders } = useCtx(RulesContext, 'useRulesData')

  return {
    rules,
    ruleProviders: ruleProviders as Record<string, RuleProvider>,
  }
}

export const useClashConfigData = (): ClashConfigContextType =>
  useCtx(ClashConfigContext, 'useClashConfigData')

export const useSystemData = (): SystemContextType =>
  useCtx(SystemContext, 'useSystemData')

export const useAppRefreshers = (): RefreshersContextType =>
  useCtx(RefreshersContext, 'useAppRefreshers')
