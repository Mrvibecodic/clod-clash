import { useEffect, useMemo, useState } from 'react'

import { useVisibility } from '@/hooks/use-visibility'

const HOUR = 60 * 60
const DAY = 24 * HOUR

/**
 * Потолок сна таймера. Дальше последних суток ждать до самой границы было бы
 * точнее, но длинный таймер не переживает сон машины: система засыпает на час,
 * просыпается — а он всё ещё «ждёт». Просыпаться раз в четверть часа дёшево,
 * тем более что перерисовки от этого не происходит (см. ниже).
 */
const MAX_SLEEP_MS = 15 * 60 * 1000

/** То, что увидит пользователь. Совпало — перерисовывать нечего. */
const label = (expire: number, at: number) => {
  const left = Math.max(0, expire - at)
  return left > DAY ? `d${Math.ceil(left / DAY)}` : `h${Math.ceil(left / HOUR)}`
}

export interface ExpiryCountdown {
  /** Секунд до конца подписки; 0 — уже истекла. */
  secondsLeft: number
  /** Дней (вверх), пока их больше одних суток. */
  daysLeft?: number
  /** Часов (вверх), когда остались последние сутки; 0 — время вышло. */
  hoursLeft?: number
}

/**
 * clod: сколько осталось до конца подписки — по системным часам клиента.
 *
 * Дату истечения панель прислала один раз, и она лежит в профиле на диске,
 * поэтому отсчёт живёт и без сети: приложение может неделю не видеть панели, а
 * «осталось 4 часа» покажет верно.
 *
 * Последние сутки показываются в часах: «1 дн» за десять минут до конца — это
 * не округление, а неправда ровно в тот момент, когда точность нужнее всего.
 *
 * Два правила, из-за которых это не стоит ничего:
 * 1. Таймер живёт, только пока окно видно. Свёрнутое в трей приложение не
 *    перерисовывает то, чего никто не видит; при показе окна значение
 *    пересчитывается сразу и с системными часами, а не «доигрывает» пропущенное.
 * 2. Состояние меняется, только если сменилась ПОКАЗАННАЯ подпись. Пробуждение,
 *    после которого на экране было бы то же число, ничего не перерисовывает.
 */
export const useExpiryCountdown = (expire?: number): ExpiryCountdown => {
  const visible = useVisibility()
  const [now, setNow] = useState(() => Date.now() / 1000)

  useEffect(() => {
    if (!expire || !visible) return

    let timer: number | undefined

    const schedule = () => {
      const current = Date.now() / 1000
      setNow((previous) =>
        label(expire, previous) === label(expire, current) ? previous : current,
      )

      const left = expire - current
      if (left <= 0) return

      // До следующей границы: пока суток больше одних — до входа в часовой
      // режим, дальше — до ближайшей целой границы часа. Секунда сверху, чтобы
      // проснуться ПОСЛЕ границы, а не в неё саму.
      const untilNextLabel = left > DAY ? left - DAY : left % HOUR || HOUR
      timer = window.setTimeout(
        schedule,
        Math.min(untilNextLabel * 1000 + 1000, MAX_SLEEP_MS),
      )
    }

    schedule()

    return () => {
      if (timer) window.clearTimeout(timer)
    }
  }, [expire, visible])

  return useMemo(() => {
    if (!expire) return { secondsLeft: 0 }

    const secondsLeft = Math.max(0, expire - now)
    return secondsLeft > DAY
      ? { secondsLeft, daysLeft: Math.ceil(secondsLeft / DAY) }
      : { secondsLeft, hoursLeft: Math.ceil(secondsLeft / HOUR) }
  }, [expire, now])
}
