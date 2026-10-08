import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

const path = (relative: string) => new URL(relative, import.meta.url)
const source = (relative: string) => readFileSync(path(relative), 'utf8')

describe('общие хуки и функции окна', () => {
  it('включение TUN — один хук на тумблеры и кнопку «Подключить»', () => {
    for (const file of [
      '../components/shared/proxy-control-switches.tsx',
      '../components/home/quick-actions.tsx',
      './use-connect-targets.ts',
    ]) {
      const text = source(file)
      assert.match(text, /useTunSwitch\(\)/, file)
      assert.match(text, /prepareTun\(\)/, file)
      assert.doesNotMatch(text, /ensureTunReady/, file)
    }
    for (const file of [
      '../components/shared/proxy-control-switches.tsx',
      '../components/home/quick-actions.tsx',
    ]) {
      assert.match(source(file), /switchTun\(\w+\)/, file)
    }
    // Исходы подготовки — в prepare-tun.test.ts; здесь только проводка.
    assert.match(
      source('./use-tun-switch.ts'),
      /prepareTunWith\(ensureTunReady, \(\) =>\s*Promise\.all\(\[mutateSystemState\(\), mutateTunState\(\)\]\)/,
    )
  })

  it('кнопка «Подключить» на обеих Главных — один хук', () => {
    for (const file of [
      '../pages/home-simple.tsx',
      '../pages/home-advanced.tsx',
    ]) {
      const text = source(file)
      assert.match(text, /useConnectButton\(\)/, file)
      assert.doesNotMatch(text, /toggleConnection|connectFailureText/, file)
    }
  })

  it('кнопка «Подключить»: под ней всегда слово, таймер — на кнопке', () => {
    const button = source('../components/home/connect-button.tsx')
    // Подпись под кнопкой одна на все состояния, при подключении — «Подключено».
    assert.match(
      button,
      /<Box sx=\{\{ mt: 1\.25 \}\}>\s*<Typography variant="subtitle1" sx=\{\{ color, fontWeight: 600 \}\}>\s*\{label\}\s*<\/Typography>\s*<\/Box>/,
    )
    // Таймер — внутри самой кнопки, имя кнопки для чтения с экрана — слово.
    const face = button.slice(
      button.indexOf('component="button"'),
      button.indexOf('</Box>'),
    )
    assert.match(face, /aria-label=\{label\}/)
    assert.match(
      face,
      /<ConnectUptime iconSize=\{iconSize\} compact=\{compact\} \/>/,
    )
    const uptime = source('../components/home/connect-uptime.tsx')
    assert.match(uptime, /export const ConnectUptime = memo\(/)
    assert.match(uptime, /aria-hidden/)
    assert.match(uptime, /sessionTimeText\(seconds\)/)
  })

  it('форматы дат и цвет причины не повторяются по экранам', () => {
    for (const file of [
      '../components/profile/profile-item.tsx',
      '../components/proxy/provider-button.tsx',
    ]) {
      assert.doesNotMatch(source(file), /parseExpire/, file)
    }
    for (const file of [
      '../components/home/no-servers-status.tsx',
      '../components/home/subscription-card.tsx',
      '../components/home/server-select.tsx',
    ]) {
      const text = source(file)
      assert.match(text, /refillDateText\(/, file)
      assert.doesNotMatch(text, /refill_date \* 1000/, file)
    }
    for (const file of [
      '../components/home/no-servers-status.tsx',
      '../components/home/server-select.tsx',
    ]) {
      assert.match(source(file), /noServersSeverity\(reason\)/, file)
    }
  })

  it('своей копии debounce нет — берётся lodash-es', () => {
    assert.equal(existsSync(path('../utils/debounce.ts')), false)
    for (const file of [
      '../providers/window/window-provider.tsx',
      '../components/profile/editor-viewer.tsx',
    ]) {
      assert.match(source(file), /import \{ debounce \} from 'lodash-es'/, file)
    }
  })

  it('удаление службы — одна команда бэкенда, шаги окно берёт из ответа', () => {
    const text = source('./use-service-uninstaller.ts')
    assert.match(text, /serviceUninstallNotices\(await uninstallService\(\)\)/)
    assert.doesNotMatch(text, /stopCore|restartCore/)
  })
})
