#!/usr/bin/env python3
"""Проверяет, что ignore в .cargo/audit.toml остаётся безопасным.

RUSTSEC-2026-0258 закрыт в h2 0.4.16; всё, что ниже, уязвимо. В Cargo.lock
уязвимая ветка присутствует только как транзитивная зависимость
tauri-plugin-devtools, который подключается опциональной фичей `tauri-dev`
и в релизные артефакты не попадает. Скрипт падает, если это перестаёт быть
правдой:

  * в .cargo/audit.toml заглушено что-то помимо ожидаемого списка;
  * фича `tauri-dev` попала в default либо плагин перестал быть optional;
  * уязвимая версия h2 достижима из воркспейса в обход плагина;
  * безопасной версии h2 в локе не осталось — заглушать стало нечего.
"""

import re
import sys

MIN_SAFE_H2 = (0, 4, 16, 1)
MIN_SAFE_H2_TEXT = "0.4.16"
EXPECTED_IGNORES = {"RUSTSEC-2026-0258"}
DEV_ONLY_GATE = "tauri-plugin-devtools"
DEV_ONLY_FEATURE = "tauri-dev"

LOCKFILE = "Cargo.lock"
MANIFEST = "src-tauri/Cargo.toml"
AUDIT_CONFIG = ".cargo/audit.toml"


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
            "уязвимая ветка h2 поедет в релиз"
        )
    default = re.search(r"^default = \[(.*?)\]", text, re.M | re.S)
    if default and f'"{DEV_ONLY_FEATURE}"' in default.group(1):
        errors.append(
            f'фича "{DEV_ONLY_FEATURE}" попала в default в {MANIFEST} — '
            "уязвимая ветка h2 поедет в релиз"
        )


def check_lock(errors):
    packages = parse_lock(read(LOCKFILE))
    versions = sorted({key[1] for key in packages if key[0] == "h2"}, key=version_key)
    if not versions:
        print("::error::h2 в Cargo.lock не найден — сторож потерял цель")
        return None
    safe = [v for v in versions if version_key(v) >= MIN_SAFE_H2]
    if not safe:
        errors.append(
            f"безопасной версии h2 (>= {MIN_SAFE_H2_TEXT}) в {LOCKFILE} не осталось — "
            "ignore RUSTSEC-2026-0258 больше не безопасен"
        )
    without_gate = reachable_without_gate(packages)
    for version in versions:
        if version_key(version) >= MIN_SAFE_H2:
            continue
        if any(key[1] == version for key in without_gate if key[0] == "h2"):
            errors.append(
                f"h2 {version} достижим из воркспейса в обход {DEV_ONLY_GATE} — "
                f"уязвимость затрагивает не только dev-сборку"
            )
    if not any(key[0] == "h2" for key in without_gate):
        errors.append(
            f"h2 вообще не достижим в обход {DEV_ONLY_GATE} — проверьте, "
            "что сторож всё ещё стережёт релизный путь"
        )
    return versions


def main():
    errors = []
    check_audit_config(errors)
    check_manifest(errors)
    try:
        versions = check_lock(errors)
    except LookupError as failure:
        print(f"::error::{failure} — разбор Cargo.lock не удался")
        return 1
    if versions is None:
        return 1
    for message in errors:
        print(f"::error::{message}")
    if errors:
        return 1
    print(f"h2 в Cargo.lock: {', '.join(versions)}")
    print(f"на релизном пути только версии >= {MIN_SAFE_H2_TEXT}")
    print(f"уязвимая ветка доступна только через {DEV_ONLY_GATE} (фича {DEV_ONLY_FEATURE})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
