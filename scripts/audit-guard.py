#!/usr/bin/env python3
"""Проверяет, что ignore в .cargo/audit.toml остаётся безопасным.

Заглушаются только те advisory, чья уязвимая версия присутствует в Cargo.lock
исключительно как транзитивная зависимость tauri-plugin-devtools: плагин
подключается опциональной фичей `tauri-dev` и в релизные артефакты не попадает.
Скрипт падает, если это перестаёт быть правдой:

  * в .cargo/audit.toml заглушено что-то помимо ожидаемого списка;
  * фича `tauri-dev` попала в default либо плагин перестал быть optional;
  * уязвимая версия заглушённого пакета достижима из воркспейса в обход плагина;
  * заглушённого пакета в локе не осталось — заглушать стало нечего;
  * у пакета, который нужен и на релизном пути, не осталось безопасной версии.
"""

import re
import sys
from dataclasses import dataclass

DEV_ONLY_GATE = "tauri-plugin-devtools"
DEV_ONLY_FEATURE = "tauri-dev"

LOCKFILE = "Cargo.lock"
MANIFEST = "src-tauri/Cargo.toml"
AUDIT_CONFIG = ".cargo/audit.toml"


@dataclass(frozen=True)
class Guarded:
    advisory: str
    crate: str
    min_safe: str
    on_the_release_path: bool


GUARDED = (
    Guarded(advisory="RUSTSEC-2026-0258", crate="h2", min_safe="0.4.16", on_the_release_path=True),
    Guarded(
        advisory="RUSTSEC-2026-0293",
        crate="ringbuf",
        min_safe="0.5.2",
        on_the_release_path=False,
    ),
)

EXPECTED_IGNORES = {entry.advisory for entry in GUARDED}


def read(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read().replace("\r\n", "\n")


def version_key(version):
    core, _, suffix = version.partition("-")
    parts = []
    for chunk in core.split(".")[:3]:
        parts.append(int(chunk) if chunk.isdigit() else 0)
    while len(parts) < 3:
        parts.append(0)
    return (*parts, 0 if suffix else 1)


def parse_lock(text):
    packages = {}
    for raw in text.split("[[package]]")[1:]:
        block = re.split(r"^\[", raw, maxsplit=1, flags=re.M)[0]
        name = re.search(r'^name = "(.+)"$', block, re.M)
        version = re.search(r'^version = "(.+)"$', block, re.M)
        if not name or not version:
            continue
        source = re.search(r'^source = "(.+)"$', block, re.M)
        deps_block = re.search(r"^dependencies = \[\n(.*?)^\]$", block, re.M | re.S)
        deps = re.findall(r'^\s*"(.+?)",?$', deps_block.group(1), re.M) if deps_block else []
        key = (name.group(1), version.group(1), source.group(1) if source else None)
        packages[key] = deps
    return packages


def resolve(dep, packages):
    parts = dep.split(" ", 2)
    name = parts[0]
    candidates = [key for key in packages if key[0] == name]
    if len(parts) > 1:
        candidates = [key for key in candidates if key[1] == parts[1]]
    if len(parts) > 2:
        source = parts[2].strip("()")
        candidates = [key for key in candidates if key[2] == source]
    if not candidates:
        raise LookupError(f'зависимость "{dep}" не нашлась в {LOCKFILE}')
    return candidates


def reachable_without_gate(packages):
    seen = set()
    stack = [key for key in packages if key[2] is None]
    while stack:
        key = stack.pop()
        if key in seen or key[0] == DEV_ONLY_GATE:
            continue
        seen.add(key)
        for dep in packages[key]:
            stack.extend(resolve(dep, packages))
    return seen


def check_audit_config(errors):
    text = read(AUDIT_CONFIG)
    block = re.search(r"^ignore = \[(.*?)\]", text, re.M | re.S)
    ignored = set(re.findall(r'"([^"]+)"', block.group(1))) if block else set()
    if ignored != EXPECTED_IGNORES:
        errors.append(
            f"список ignore в {AUDIT_CONFIG} изменился: {sorted(ignored)} вместо "
            f"{sorted(EXPECTED_IGNORES)} — сторож проверяет не то, что заглушено"
        )


def check_manifest(errors):
    text = read(MANIFEST)
    declaration = re.search(rf"^{re.escape(DEV_ONLY_GATE)} = (.+)$", text, re.M)
    if not declaration:
        errors.append(f"{DEV_ONLY_GATE} не найден в {MANIFEST} — сторож потерял цель")
    elif "optional = true" not in declaration.group(1):
        errors.append(
            f"{DEV_ONLY_GATE} в {MANIFEST} перестал быть optional — "
            "уязвимая ветка поедет в релиз"
        )
    default = re.search(r"^default = \[(.*?)\]", text, re.M | re.S)
    if default and f'"{DEV_ONLY_FEATURE}"' in default.group(1):
        errors.append(
            f'фича "{DEV_ONLY_FEATURE}" попала в default в {MANIFEST} — '
            "уязвимая ветка поедет в релиз"
        )


def check_crate(entry, packages, without_gate, errors):
    versions = sorted({key[1] for key in packages if key[0] == entry.crate}, key=version_key)
    if not versions:
        errors.append(
            f"{entry.crate} в {LOCKFILE} не найден — заглушать {entry.advisory} больше нечего"
        )
        return None
    safe_from = version_key(entry.min_safe)
    if entry.on_the_release_path:
        if not any(version_key(version) >= safe_from for version in versions):
            errors.append(
                f"безопасной версии {entry.crate} (>= {entry.min_safe}) в {LOCKFILE} "
                f"не осталось — ignore {entry.advisory} больше не безопасен"
            )
        if not any(key[0] == entry.crate for key in without_gate):
            errors.append(
                f"{entry.crate} вообще не достижим в обход {DEV_ONLY_GATE} — проверьте, "
                "что сторож всё ещё стережёт релизный путь"
            )
    for version in versions:
        if version_key(version) >= safe_from:
            continue
        if any(key[1] == version for key in without_gate if key[0] == entry.crate):
            errors.append(
                f"{entry.crate} {version} достижим из воркспейса в обход {DEV_ONLY_GATE} — "
                f"{entry.advisory} затрагивает не только dev-сборку"
            )
    return versions


def check_lock(errors):
    packages = parse_lock(read(LOCKFILE))
    without_gate = reachable_without_gate(packages)
    found = {}
    for entry in GUARDED:
        versions = check_crate(entry, packages, without_gate, errors)
        if versions is not None:
            found[entry.crate] = versions
    return found


def main():
    errors = []
    check_audit_config(errors)
    check_manifest(errors)
    try:
        found = check_lock(errors)
    except LookupError as failure:
        print(f"::error::{failure} — разбор {LOCKFILE} не удался")
        return 1
    for message in errors:
        print(f"::error::{message}")
    if errors:
        return 1
    for entry in GUARDED:
        print(f"{entry.crate} в {LOCKFILE}: {', '.join(found[entry.crate])}")
        if entry.on_the_release_path:
            print(f"на релизном пути только версии >= {entry.min_safe}")
    print(f"уязвимые ветки доступны только через {DEV_ONLY_GATE} (фича {DEV_ONLY_FEATURE})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
