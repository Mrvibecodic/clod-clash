import { useCallback, useEffect, useReducer } from 'react'

import { useProfiles } from '@/hooks/use-profiles'

import { ProxySortType } from './use-filter-sort'

export interface HeadState {
  open?: boolean
  showType: boolean
  sortType: ProxySortType
  filterText: string
  filterMatchCase?: boolean
  filterMatchWholeWord?: boolean
  filterUseRegularExpression?: boolean
  textState: 'url' | 'filter' | null
  testUrl: string
}

type HeadStateStorage = Record<string, Record<string, HeadState>>

const forgetUnknownProfiles = (
  data: HeadStateStorage,
  known: string | undefined,
  current: string,
): HeadStateStorage => {
  if (known === undefined) return data
  const kept = new Set([...known.split('\n').filter(Boolean), current])
  return Object.fromEntries(
    Object.entries(data).filter(([uid]) => kept.has(uid)),
  )
}

const HEAD_STATE_KEY = 'proxy-head-state'
export const DEFAULT_STATE: HeadState = {
  open: false,
  showType: true,
  sortType: 0,
  filterText: '',
  filterMatchCase: false,
  filterMatchWholeWord: false,
  filterUseRegularExpression: false,
  textState: null,
  testUrl: '',
}

type HeadStateAction =
  | { type: 'reset' }
  | { type: 'replace'; payload: Record<string, HeadState> }
  | { type: 'update'; groupName: string; patch: Partial<HeadState> }

function headStateReducer(
  state: Record<string, HeadState>,
  action: HeadStateAction,
): Record<string, HeadState> {
  switch (action.type) {
    case 'reset':
      return {}
    case 'replace':
      return action.payload
    case 'update': {
      const prev = state[action.groupName] || DEFAULT_STATE
      return { ...state, [action.groupName]: { ...prev, ...action.patch } }
    }
    default:
      return state
  }
}

export function useHeadStateNew() {
  const { profiles } = useProfiles()
  const current = profiles?.current || ''
  const knownProfiles = profiles?.items?.map((item) => item.uid).join('\n')

  const [state, dispatch] = useReducer(headStateReducer, {})

  useEffect(() => {
    try {
      const data = JSON.parse(
        localStorage.getItem(HEAD_STATE_KEY)!,
      ) as HeadStateStorage

      const value = data[current] || {}

      if (value && typeof value === 'object') {
        dispatch({ type: 'replace', payload: value })
      } else {
        dispatch({ type: 'reset' })
      }
    } catch {
      dispatch({ type: 'reset' })
    }
  }, [current])

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        const item = localStorage.getItem(HEAD_STATE_KEY)

        let data = (item ? JSON.parse(item) : {}) as HeadStateStorage

        if (!data || typeof data !== 'object') data = {}

        data[current] = state

        localStorage.setItem(
          HEAD_STATE_KEY,
          JSON.stringify(forgetUnknownProfiles(data, knownProfiles, current)),
        )
      } catch {}
    })

    return () => clearTimeout(timer)
  }, [state, current, knownProfiles])

  const setHeadState = useCallback(
    (groupName: string, obj: Partial<HeadState>) => {
      dispatch({ type: 'update', groupName, patch: obj })
    },
    [],
  )

  return [state, setHeadState] as const
}
