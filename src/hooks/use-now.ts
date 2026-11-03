import { useState, useSyncExternalStore } from 'react'

import { useVisibility } from '@/hooks/use-visibility'

interface Ticker {
  now: number
  timer: number | null
  listeners: Set<() => void>
}

/** Один таймер на период на всё окно, сколько бы строк ни показывали время. */
const tickers = new Map<number, Ticker>()

const tickerFor = (periodMs: number) => {
  let ticker = tickers.get(periodMs)
  if (!ticker) {
    ticker = { now: Date.now(), timer: null, listeners: new Set() }
    tickers.set(periodMs, ticker)
  }
  return ticker
}

const subscribeTo = (periodMs: number) => (listener: () => void) => {
  const ticker = tickerFor(periodMs)
  ticker.listeners.add(listener)
  if (ticker.timer === null) {
    // Вернулись к окну после паузы: время сразу свежее, а не через период.
    ticker.now = Date.now()
    ticker.timer = window.setInterval(() => {
      ticker.now = Date.now()
      ticker.listeners.forEach((notify) => notify())
    }, periodMs)
  }
  return () => {
    ticker.listeners.delete(listener)
    if (ticker.listeners.size === 0 && ticker.timer !== null) {
      window.clearInterval(ticker.timer)
      ticker.timer = null
    }
  }
}

const subscribers = new Map<number, (listener: () => void) => () => void>()
const subscriberFor = (periodMs: number) => {
  let subscribe = subscribers.get(periodMs)
  if (!subscribe) {
    subscribe = subscribeTo(periodMs)
    subscribers.set(periodMs, subscribe)
  }
  return subscribe
}

const subscribeNothing = () => () => {}

/** «Сейчас» с шагом `periodMs`; пока окно не видно, часы стоят. */
export const useNow = (periodMs: number) => {
  const visible = useVisibility()
  const ticker = tickerFor(periodMs)
  // Часы могли встать давно (окно было скрыто): строка, появившаяся сейчас,
  // не показывает время, на котором они встали.
  const [shownAt] = useState(() => Date.now())
  const read = () => ticker.now
  const now = useSyncExternalStore(
    visible ? subscriberFor(periodMs) : subscribeNothing,
    read,
    read,
  )
  return Math.max(now, shownAt)
}
