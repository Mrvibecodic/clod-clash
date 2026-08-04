import { useEffect, useMemo, useState } from 'react'

import { useVisibility } from '@/hooks/use-visibility'

const HOUR = 60 * 60
const DAY = 24 * HOUR

/**
 * Потолок сна таймера — страховка от сна машины.
 *
 * Ждать ровно до границы было бы точнее и дешевле, но таймер вебвью идёт по
 * монотонным часам, которые в suspend не тикают: отложенный на час выстрелит
 * через час ПОСЛЕ пробуждения, а не в срок. Проверка видимости тут не спасает
 * — крышку закрывают с открытым окном, и видимость не меняется.
 *
 * Поэтому в последние сутки, где число живое, просыпаемся не реже чем раз в
 * четверть часа, а пока идут дни — раз в час. Просыпание почти всегда холостое
 * и ничего не перерисовывает (см. ниже), так что стоит оно недорого. Заодно
 * это держит задержку далеко от 32-битного предела `setTimeout`: всё длиннее
 * ~24.8 суток он выполняет немедленно, и годовая подписка стала бы busy-loop.
 */
const MAX_SLEEP_MS = 60 * 60 * 1000
const MAX_SLEEP_LAST_DAY_MS = 15 * 60 * 1000

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
 * clod: сколько осталось до конца подписки.
 *
 * Дату истечения панель прислала один раз, и она лежит в профиле на диске,
 * поэтому отсчёт живёт и без сети: приложение может неделю не видеть панели, а
 * «осталось 4 часа» покажет верно.
 *
 * Считаем по часам устройства, сдвинутым на `skew` — разницу с часами панели,
 * снятую из заголовка `Date` при последнем обновлении подписки
 * (`PrfItem::clock_skew`). Срок истекает в конкретный момент, и устройство с
 * ушедшими часами отсчитывает его настолько же неверно; сверять же было не с
 * чем — панель отвечает раз в час, а то и раз в сутки.
 *
 * Последние сутки показываются в часах: «1 дн» за десять минут до конца — это
 * не округление, а неправда ровно в тот момент, когда точность нужнее всего.
 *
 * Два правила, из-за которых это не стоит ничего:
 * 1. Таймер живёт, только пока окно видно. Свёрнутое в трей приложение не
 *    перерисовывает то, чего никто не видит; при показе окна значение
 *    пересчитывается сразу и по часам, а не «доигрывает» пропущенное.
 * 2. Просыпаемся к смене ПОКАЗАННОЙ подписи — к границе суток, пока идут дни,
 *    и к границе часа в последние сутки (плюс страховочный потолок сна выше).
 *    Состояние меняется, только если сменилась сама подпись: пробуждение,
 *    после которого на экране было бы то же число, не перерисовывает ничего.
 *    Проснувшись, отсчёт заново смотрит на часы, а не досчитывает пропущенное.
 */
export const useExpiryCountdown = (
  expire?: number,
  skew = 0,
): ExpiryCountdown => {
  const visible = useVisibility()
  const [now, setNow] = useState(() => Date.now() / 1000 + skew)

  useEffect(() => {
    if (!expire || !visible) return

    let timer: number | undefined

    const schedule = () => {
      const current = Date.now() / 1000 + skew
      setNow((previous) =>
        label(expire, previous) === label(expire, current) ? previous : current,
      )

      const left = expire - current
      if (left <= 0) return

      // До следующей границы: пока суток больше одних — до целой границы
      // суток (она же вход в часовой режим, когда сутки последние), дальше —
      // до ближайшей целой границы часа. Секунда сверху, чтобы проснуться
      // ПОСЛЕ границы, а не в неё саму.
      const lastDay = left <= DAY
      const untilNextLabel = lastDay ? left % HOUR || HOUR : left % DAY || DAY
      timer = window.setTimeout(
        schedule,
        Math.min(
          untilNextLabel * 1000 + 1000,
          lastDay ? MAX_SLEEP_LAST_DAY_MS : MAX_SLEEP_MS,
        ),
      )
    }

    schedule()

    return () => {
      if (timer) window.clearTimeout(timer)
    }
  }, [expire, visible, skew])

  return useMemo(() => {
    if (!expire) return { secondsLeft: 0 }

    const secondsLeft = Math.max(0, expire - now)
    return secondsLeft > DAY
      ? { secondsLeft, daysLeft: Math.ceil(secondsLeft / DAY) }
      : { secondsLeft, hoursLeft: Math.ceil(secondsLeft / HOUR) }
  }, [expire, now])
}
