import i18n from 'i18next'
import { type ReactNode, isValidElement } from 'react'

// Относительный путь с расширением, а не `@/`: собственный тест модуля гоняет
// встроенный раннер Node, а он не знает ни алиасов из `tsconfig`, ни
// достраивания расширений.
import { explainErrorKey, trimRawError } from '../utils/error-explanation.ts'

type NoticeType = 'success' | 'error' | 'info'

interface NoticeTranslationDescriptor {
  key: string
  params?: Record<string, unknown>
}

interface NoticeItem {
  readonly id: number
  readonly type: NoticeType
  readonly duration: number
  readonly message?: ReactNode
  readonly i18n?: NoticeTranslationDescriptor
  readonly repeats: number
  readonly signature?: string
  timerId?: ReturnType<typeof setTimeout>
}

type NoticeContent = unknown

type NoticeExtra = unknown

const COLLAPSE_KEY = Symbol('notice.collapseKey')

interface NoticeCollapseKey {
  readonly [COLLAPSE_KEY]: string
}

/**
 * Marks a notice as "the same one" for collapsing purposes.
 *
 * Notices built as ReactNode (an action link, for instance) have no
 * translation descriptor, so their signature cannot be derived from the text.
 * Pass the status — plus whatever data makes two of them genuinely different,
 * such as the port number — and repeats fold into one toast with a counter.
 */
export function collapseBy(key: string): NoticeCollapseKey {
  return { [COLLAPSE_KEY]: key }
}

function isCollapseKey(value: unknown): value is NoticeCollapseKey {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { [COLLAPSE_KEY]?: unknown })[COLLAPSE_KEY] === 'string'
  )
}

type NoticeShortcut = (
  message: NoticeContent,
  ...extras: NoticeExtra[]
) => number

type ShowNotice = ((
  type: NoticeType,
  message: NoticeContent,
  ...extras: NoticeExtra[]
) => number) & {
  success: NoticeShortcut
  error: NoticeShortcut
  info: NoticeShortcut
}

type NoticeSubscriber = () => void

const DEFAULT_DURATIONS: Readonly<Record<NoticeType, number>> = {
  success: 3000,
  info: 5000,
  error: 8000,
}

const TRANSLATION_KEY_PATTERN = /^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+$/

let nextId = 0
let notices: NoticeItem[] = []
const subscribers: Set<NoticeSubscriber> = new Set()

function notifySubscribers() {
  subscribers.forEach((subscriber) => subscriber())
}

interface ParsedNoticeExtras {
  params?: Record<string, unknown>
  raw?: unknown
  duration?: number
  collapseKey?: string
}

function parseNoticeExtras(extras: NoticeExtra[]): ParsedNoticeExtras {
  let params: Record<string, unknown> | undefined
  let raw: unknown
  let duration: number | undefined
  let collapseKey: string | undefined

  // Prioritize objects as translation params, then as raw payloads, while the first number wins as duration.
  for (const extra of extras) {
    if (extra === undefined) continue

    // Checked before isPlainRecord: a collapse key is a plain object too.
    if (isCollapseKey(extra)) {
      collapseKey ??= extra[COLLAPSE_KEY]
      continue
    }

    if (typeof extra === 'number' && duration === undefined) {
      duration = extra
      continue
    }

    if (isPlainRecord(extra)) {
      if (!params) {
        params = extra
        continue
      }
      if (!raw) {
        raw = extra
        continue
      }
    }

    if (!raw) {
      raw = extra
      continue
    }

    if (!params && isPlainRecord(extra)) {
      params = extra
      continue
    }

    if (duration === undefined && typeof extra === 'number') {
      duration = extra
    }
  }

  return { params, raw, duration, collapseKey }
}

function resolveDuration(type: NoticeType, override?: number) {
  return override ?? DEFAULT_DURATIONS[type]
}

function buildNotice(
  id: number,
  type: NoticeType,
  duration: number,
  payload: { message?: ReactNode; i18n?: NoticeTranslationDescriptor },
  signature: string | undefined,
  timerId?: ReturnType<typeof setTimeout>,
): NoticeItem {
  return {
    id,
    type,
    duration,
    timerId,
    repeats: 1,
    signature,
    ...payload,
  }
}

function noticeSignature(
  type: NoticeType,
  i18n?: NoticeTranslationDescriptor,
  collapseKey?: string,
): string | undefined {
  if (collapseKey !== undefined) {
    return `${type}:collapse:${collapseKey}`
  }
  if (!i18n) return undefined
  try {
    return `${type}:${JSON.stringify(i18n)}`
  } catch {
    return undefined
  }
}

function scheduleHide(id: number, duration: number) {
  return duration > 0 ? setTimeout(() => hideNotice(id), duration) : undefined
}

function isMaybeTranslationDescriptor(
  message: unknown,
): message is NoticeTranslationDescriptor {
  if (
    typeof message === 'object' &&
    message !== null &&
    !Array.isArray(message) &&
    !isValidElement(message)
  ) {
    return typeof (message as Record<string, unknown>).key === 'string'
  }
  return false
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  if (
    typeof value !== 'object' ||
    value === null ||
    Array.isArray(value) ||
    value instanceof Error ||
    isValidElement(value)
  ) {
    return false
  }

  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}

function createRawDescriptor(message: string): NoticeTranslationDescriptor {
  // clod:error-mapper — сюда стекается ВСЁ, что показывается пользователю как
  // сырой текст: ошибки ядра, системные коды, чужие обёртки. Одна точка —
  // значит, словарь не придётся вспоминать на каждом новом вызове showNotice.
  const explanationKey = explainErrorKey(message)
  if (explanationKey) {
    return {
      key: 'shared.feedback.notices.explained',
      params: {
        // Переводит `prefixKey` слой отрисовки (`notice-manager`) — тот же
        // механизм, что и у `prefixedRaw`. Звать `t()` здесь нельзя: ключ
        // известен только в момент разбора, а типизированный `t` требует
        // литерал из сгенерированного списка.
        prefixKey: explanationKey,
        rawFull: message,
        message: trimRawError(message),
      },
    }
  }
  return {
    key: 'shared.feedback.notices.raw',
    params: { message },
  }
}

function withRawExplanation(
  params: Record<string, unknown>,
  rawText: string,
): Record<string, unknown> {
  const explanationKey = explainErrorKey(rawText)
  if (!explanationKey) {
    return { ...params, message: rawText }
  }
  return {
    ...params,
    explanationKey,
    rawFull: rawText,
    message: trimRawError(rawText),
  }
}

function isLikelyTranslationKey(key: string) {
  return TRANSLATION_KEY_PATTERN.test(key)
}

function shouldUseTranslationKey(
  key: string,
  params?: Record<string, unknown>,
) {
  if (params && Object.keys(params).length > 0) return true
  if (isLikelyTranslationKey(key)) return true
  if (i18n.isInitialized) {
    return i18n.exists(key)
  }
  return false
}

function extractDisplayText(input: unknown): string | undefined {
  if (input === null || input === undefined) return undefined
  if (typeof input === 'string') return input
  if (typeof input === 'number' || typeof input === 'boolean') {
    return String(input)
  }
  if (input instanceof Error) {
    return input.message || input.name
  }
  if (typeof input === 'object' && input !== null) {
    const maybeMessage = (input as { message?: unknown }).message
    if (typeof maybeMessage === 'string') return maybeMessage
  }
  try {
    return JSON.stringify(input)
  } catch {
    return String(input)
  }
}

function normalizeNoticeMessage(
  message: NoticeContent,
  params?: Record<string, unknown>,
  raw?: unknown,
): { message?: ReactNode; i18n?: NoticeTranslationDescriptor } {
  const rawText = raw !== undefined ? extractDisplayText(raw) : undefined

  if (isValidElement(message)) {
    return { message }
  }

  if (isMaybeTranslationDescriptor(message)) {
    const originalParams = message.params ?? {}
    const mergedParams = Object.keys(params ?? {}).length
      ? { ...originalParams, ...params }
      : { ...originalParams }

    if (rawText !== undefined) {
      return {
        i18n: {
          key: 'shared.feedback.notices.prefixedRaw',
          params: withRawExplanation(
            {
              ...mergedParams,
              prefixKey: message.key,
              prefixParams: originalParams,
            },
            rawText,
          ),
        },
      }
    }

    return {
      i18n: {
        key: message.key,
        params: Object.keys(mergedParams).length ? mergedParams : undefined,
      },
    }
  }

  if (typeof message === 'string') {
    if (rawText !== undefined) {
      if (shouldUseTranslationKey(message, params)) {
        return {
          i18n: {
            key: 'shared.feedback.notices.prefixedRaw',
            params: withRawExplanation(
              { ...(params ?? {}), prefixKey: message },
              rawText,
            ),
          },
        }
      }
      // Prefer showing the original string while still surfacing the raw details below.
      return {
        i18n: {
          key: 'shared.feedback.notices.prefixedRaw',
          params: withRawExplanation(
            { ...(params ?? {}), prefix: message },
            rawText,
          ),
        },
      }
    }

    if (shouldUseTranslationKey(message, params)) {
      return {
        i18n: {
          key: message,
          params: params && Object.keys(params).length ? params : undefined,
        },
      }
    }
    return { i18n: createRawDescriptor(message) }
  }

  if (rawText !== undefined) {
    return { i18n: createRawDescriptor(rawText) }
  }

  const extracted = extractDisplayText(message)
  if (extracted !== undefined) {
    return { i18n: createRawDescriptor(extracted) }
  }

  return { i18n: createRawDescriptor('') }
}

const baseShowNotice = (
  type: NoticeType,
  message: NoticeContent,
  ...extras: NoticeExtra[]
): number => {
  const { params, raw, duration, collapseKey } = parseNoticeExtras(extras)
  const effectiveDuration = resolveDuration(type, duration)
  const normalizedMessage = normalizeNoticeMessage(message, params, raw)

  const signature = noticeSignature(type, normalizedMessage.i18n, collapseKey)
  const same = signature
    ? notices.find((candidate) => candidate.signature === signature)
    : undefined
  if (same) {
    repeatNotice(same.id, 1)
    return same.id
  }

  const id = nextId++
  const notice = buildNotice(
    id,
    type,
    effectiveDuration,
    normalizedMessage,
    signature,
    scheduleHide(id, effectiveDuration),
  )

  notices = [...notices, notice]
  notifySubscribers()
  return id
}

/**
 * Shows a global notice; `showNotice.success / error / info` are the usual entry points.
 *
 * - `message`: i18n key string, `{ key, params }`, ReactNode, Error/any value (message is extracted)
 * - `extras` parsed left-to-right: first plain object is i18n params; next value is raw payload; first number overrides duration (ms, 0 = persistent; defaults: success 3000 / info 5000 / error 8000)
 * - Returns a notice id for manual closing via `hideNotice(id)`
 *
 * @example showNotice.success("profiles.page.feedback.notifications.batchDeleted");
 * @example showNotice.error(err); // pass an Error directly
 * @example showNotice.error("shared.feedback.notifications.common.refreshFailed", err); // Simply pass an Error directly; but we recommend using { err } with i18n key and placeholders.
 * @example showNotice.error("profiles.page.feedback.errors.invalidUrl", { url }, 4000);
 */
export const showNotice: ShowNotice = Object.assign(baseShowNotice, {
  success: (message: NoticeContent, ...extras: NoticeExtra[]) =>
    baseShowNotice('success', message, ...extras),
  error: (message: NoticeContent, ...extras: NoticeExtra[]) =>
    baseShowNotice('error', message, ...extras),
  info: (message: NoticeContent, ...extras: NoticeExtra[]) =>
    baseShowNotice('info', message, ...extras),
})

export function repeatNotice(id: number, times: number) {
  const notice = notices.find((candidate) => candidate.id === id)
  if (!notice || times < 1) return
  if (notice.timerId) {
    clearTimeout(notice.timerId)
  }
  const repeated: NoticeItem = {
    ...notice,
    repeats: notice.repeats + times,
    timerId: scheduleHide(id, notice.duration),
  }
  notices = notices.map((candidate) =>
    candidate.id === id ? repeated : candidate,
  )
  notifySubscribers()
}

export function hideNotice(id: number) {
  const notice = notices.find((candidate) => candidate.id === id)
  if (notice?.timerId) {
    clearTimeout(notice.timerId)
  }
  notices = notices.filter((candidate) => candidate.id !== id)
  notifySubscribers()
}

export function subscribeNotices(subscriber: NoticeSubscriber) {
  subscribers.add(subscriber)
  return () => {
    subscribers.delete(subscriber)
  }
}

export function getSnapshotNotices() {
  return notices
}
