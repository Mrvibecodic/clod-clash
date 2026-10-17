import { createElement, isValidElement, ReactNode, useRef } from 'react'

import noop from '@/utils/noop'

interface Props<Value> {
  value?: Value
  valueProps?: string
  onChange?: (value: Value) => void
  onFormat?: (...args: any[]) => Value
  onGuard?: (value: Value, oldValue: Value) => Promise<void>
  onCatch?: (error: Error) => void
  children: ReactNode
}

export function GuardState<T>(props: Props<T>) {
  const {
    value,
    children,
    valueProps = 'value',
    onGuard = noop,
    onCatch = noop,
    onChange = noop,
    onFormat,
  } = props

  const lockRef = useRef(false)

  if (!isValidElement(children)) {
    return children as any
  }

  const childProps = { ...(children.props as Record<string, any>) }

  childProps[valueProps] = value
  childProps.onChange = async (...args: any[]) => {
    // Повторные действия игнорируются
    if (lockRef.current) return
    lockRef.current = true

    try {
      const newValue = onFormat ? onFormat(...args) : (args[0] as T)
      // Сначала отражаем действие в UI
      onChange(newValue)
      await onGuard(newValue, value!)
    } catch (err: any) {
      // Откат состояния
      onChange(value!)
      onCatch(err)
    } finally {
      lockRef.current = false
    }
  }
  const { children: nestedChildren, ...restProps } = childProps

  return createElement(children.type, restProps, nestedChildren)
}
