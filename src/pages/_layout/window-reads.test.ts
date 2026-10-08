import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

/** Текст от `start` до первого `end` после него. */
const block = (text: string, start: string, end: string) => {
  const from = text.indexOf(start)
  if (from < 0) return ''
  const to = text.indexOf(end, from + start.length)
  return to < 0 ? '' : text.slice(from, to + end.length)
}

describe('окно не перечитывает лишнего', () => {
  it('правила ядра читает только экран «Правила», вне его они сбрасываются', () => {
    const events = source('./hooks/use-layout-events.ts')
    const whenVisible = block(
      events,
      'const CLASH_CONFIG_KEYS_WHEN_VISIBLE = [',
      '] as const',
    )
    assert.ok(whenVisible, 'список ключей не найден — тест ослеп')
    assert.doesNotMatch(whenVisible, /getRules|getRuleProviders/)
    assert.match(events, /removeCacheData\(\[key\]\)/)

    const provider = source('../../providers/app-data-provider.tsx')
    for (const key of ['getRules', 'getRuleProviders']) {
      const query = block(provider, `queryKey: ['${key}']`, '})')
      assert.ok(query, `${key} не найден — тест ослеп`)
      assert.match(query, /revalidateOnMount: false/, key)
    }
  })

  it('описания узлов и пометки 16–20 читает список, а не строка', () => {
    for (const path of [
      '../../components/proxy/proxy-item.tsx',
      '../../components/proxy/proxy-item-mini.tsx',
    ]) {
      const text = source(path)
      assert.doesNotMatch(text, /useServerDescriptions|useFreezeMarks/, path)
    }
    for (const path of [
      '../../components/proxy/proxy-groups.tsx',
      '../../components/proxy/proxy-groups-chain.tsx',
    ]) {
      const text = source(path)
      assert.match(text, /useServerDescriptions\(\)/, path)
      assert.match(text, /useFreezeMarks\(\)/, path)
    }
  })

  it('после команд, которые сами шлют обновление конфига, ручных перечитываний нет', () => {
    const clash = source('../../hooks/use-clash.ts')
    const patchClash = block(clash, 'const patchClash = useLockFn', '})')
    assert.ok(patchClash, 'patchClash не найден — тест ослеп')
    assert.match(patchClash, /await refetchLadder\(\)/)
    assert.doesNotMatch(patchClash, /mutateClash\(/)
    const patchInfo = block(clash, 'const patchInfo = useLockFn', '})')
    assert.ok(patchInfo, 'patchInfo не найден — тест ослеп')
    assert.doesNotMatch(patchInfo, /mutateInfo|revalidateQuery/)

    assert.doesNotMatch(source('../proxies.tsx'), /refreshClashConfig/)
    assert.doesNotMatch(
      source('../../components/setting/mods/tunnels-viewer.tsx'),
      /mutateClash/,
    )
    assert.doesNotMatch(
      source('../../components/setting/setting-clash.tsx'),
      /setTimeout\(\(\) => \{\s*mutateClash\(\)/,
    )
    assert.doesNotMatch(
      source('../../components/setting/mods/clash-core-viewer.tsx'),
      /invalidateClashConfig|setTimeout\(resolve, 500\)/,
    )
  })

  it('выбор способа подключения пишется той же записью, что и тумблер', () => {
    const sysproxy = source('../../hooks/use-system-proxy-state.ts')
    assert.match(sysproxy, /connect_system_proxy: target/)
    assert.match(sysproxy, /failed \? \[\['getVergeConfig'\]\]/)
    assert.doesNotMatch(
      source('../../hooks/use-connect-targets.ts'),
      /useRememberTargets/,
    )
    for (const path of [
      '../../components/home/quick-actions.tsx',
      '../../components/shared/proxy-control-switches.tsx',
    ]) {
      const text = source(path)
      assert.doesNotMatch(text, /rememberTarget/, path)
      assert.match(text, /toggleSystemProxy\(\w+, true\)/, path)
      assert.match(text, /connect_tun_mode: \w+/, path)
    }
  })

  it('очередь уведомлений забирается один раз за показ окна', () => {
    const layout = source('../_layout.tsx')
    assert.match(layout, /if \(!drainedRef\.current\) drainPendingNotices\(\)/)
    assert.match(layout, /drainedRef\.current = false/)
  })
})
