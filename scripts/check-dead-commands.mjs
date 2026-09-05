import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const HANDLER_FILE = 'src-tauri/src/lib.rs'
const FRONTEND_DIR = 'src'
const FRONTEND_EXT = ['.ts', '.tsx']
const ALLOWED = new Set([])

function registeredCommands() {
  const source = readFileSync(HANDLER_FILE, 'utf8')
  const block = source.match(/generate_handler!\[([\s\S]*?)\]/)
  if (!block) {
    throw new Error(`generate_handler! не найден в ${HANDLER_FILE}`)
  }
  return block[1]
    .split('\n')
    .map((line) => line.trim().replace(/,$/, ''))
    .filter((line) => line && !line.startsWith('//') && !line.startsWith('#'))
    .map((path) => path.split('::').pop())
}

function frontendFiles(dir) {
  const found = []
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) {
      found.push(...frontendFiles(full))
    } else if (FRONTEND_EXT.some((ext) => entry.endsWith(ext))) {
      found.push(full)
    }
  }
  return found
}

function frontendLiterals() {
  const literals = new Set()
  for (const file of frontendFiles(FRONTEND_DIR)) {
    const source = readFileSync(file, 'utf8')
      .replace(/\/\*[\s\S]*?\*\//g, ' ')
      .replace(/(^|[^:'"`])\/\/[^\n]*/g, '$1')
    for (const match of source.matchAll(/['"`]([A-Za-z0-9_]+)['"`]/g)) {
      literals.add(match[1])
    }
  }
  return literals
}

const commands = registeredCommands()
const literals = frontendLiterals()
const dead = commands.filter(
  (name) => !literals.has(name) && !ALLOWED.has(name),
)

if (dead.length > 0) {
  console.error(
    `Команды зарегистрированы в ${HANDLER_FILE}, но интерфейс их не зовёт:`,
  )
  for (const name of dead) {
    console.error(`  ${name}`)
  }
  console.error('')
  console.error('Убрать команду из generate_handler! вместе с её функцией,')
  console.error(
    `либо, если её зовут не из ${FRONTEND_DIR}, вписать имя в ALLOWED в этом скрипте.`,
  )
  process.exit(1)
}

console.log(
  `Команд зарегистрировано: ${commands.length}, все вызываются из ${FRONTEND_DIR}.`,
)
