import { outboundClass, outboundName } from './connection-text'

export const HostText = ({ host, port }: { host?: string; port: string }) => (
  <>
    {host}
    <span className="cc-muted">:{port}</span>
  </>
)

export const SpeedText = ({
  value,
  text,
  up,
  arrow,
}: {
  value: number
  text: string
  up?: boolean
  arrow?: boolean
}) => {
  const mark = arrow ? (up ? '↑ ' : '↓ ') : ''
  return value > 0 ? (
    <span className={up ? 'cc-up' : 'cc-dn'}>
      {mark}
      {text}
    </span>
  ) : (
    <span className="cc-muted">{mark}—</span>
  )
}

export const ChainText = ({
  chains,
  directLabel,
}: {
  chains: readonly string[]
  directLabel: string
}) => {
  const parts = []
  for (let i = chains.length - 1; i > 0; i--) {
    parts.push(
      <span key={i} className="cc-hop">
        {chains[i]}
      </span>,
      <span key={`s${i}`} className="cc-sep">
        ›
      </span>,
    )
  }
  const last = chains[0] ?? ''
  return (
    <span className="cc-chain" title={[...chains].reverse().join(' › ')}>
      {parts}
      <span className={`cc-last ${outboundClass(last)}`}>
        {outboundName(last, directLabel)}
      </span>
    </span>
  )
}

export const RuleText = ({
  rule,
  payload,
  fallback,
  wrap,
}: {
  rule: string
  payload: string
  fallback: string
  wrap?: boolean
}) => (
  <span
    className={wrap ? 'cc-rule cc-wrap' : 'cc-rule'}
    title={payload ? `${rule}(${payload})` : rule}
  >
    <span className="cc-tchip">{rule}</span>
    <span className={payload ? 'cc-pl' : 'cc-pl cc-muted'}>
      {payload || fallback}
    </span>
  </span>
)
