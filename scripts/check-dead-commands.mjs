import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const HANDLER_FILE = 'src-tauri/src/lib.rs'
const FRONTEND_DIR = 'src'
const FRONTEND_EXT = ['.ts', '.tsx']
const ALLOWED = new Set([])

// Список берётся целиком, до парной скобки: атрибут вроде `#[cfg(...)]`
// внутри списка иначе обрывал бы его на своей `]`, и остаток молча не
// проверялся. Каждый элемент обязан быть путём к функции — непонятное роняет
// проверку, а не прячет команду.
function registeredCommands() {
  const source = readFileSync(HANDLER_FILE, 'utf8')
  const opener = 'generate_handler!['
  const start = source.indexOf(opener)
  if (start < 0) {
    throw new Error(`generate_handler! не найден в ${HANDLER_FILE}`)
  }
  let depth = 1
  let end = start + opener.length
  for (; end < source.length && depth > 0; end++) {
    if (source[end] === '[') depth++
    else if (source[end] === ']') depth--
  }
  if (depth !== 0) {
    throw new Error(`список generate_handler! не закрыт в ${HANDLER_FILE}`)
  }
  const list = source
    .slice(start + opener.length, end - 1)
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/\/\/[^\n]*/g, ' ')
    .replace(/#\[[^\]]*\]/g, ' ')
  const names = []
  for (const item of list.split(',')) {
    const path = item.trim()
    if (!path) continue
    if (!/^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*$/.test(path)) {
      throw new Error(
        `не разобран элемент generate_handler! в ${HANDLER_FILE}: ${path}`,
      )
    }
    names.push(path.split('::').pop())
  }
  if (names.length === 0) {
    throw new Error(`в generate_handler! не найдено ни одной команды`)
  }
  return names
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
