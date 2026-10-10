import { useMemo } from 'react'

const blank = (v: unknown) =>
  v === undefined || v === null || v === '' || v === false

const same = (a: unknown, b: unknown) =>
  (blank(a) && blank(b)) || JSON.stringify(a) === JSON.stringify(b)

export const useChangeCount = <T extends Record<string, unknown>>(
  initial: T,
  current: T,
) =>
  useMemo(() => {
    const keys = new Set([...Object.keys(initial), ...Object.keys(current)])
    let count = 0
    for (const key of keys) {
      if (!same(initial[key], current[key])) {
        count += 1
      }
    }
    return count
  }, [initial, current])
