#!/bin/bash

state_file="${1:-.original_dns.txt}"
placeholder="${2:-}"

[ ! -f "$state_file" ] && exit 0

current_service() {
    local nic
    nic=$(route -n get default | grep "interface" | awk '{print $2}')
    networksetup -listnetworkserviceorder | awk -v dev="$nic" '
        /^\([0-9]+\) /{port=$0; sub(/^\([0-9]+\) /, "", port)}
        /\(Hardware Port:/{interface=$NF;sub(/\)/, "", interface); if (interface == dev) {print port; exit}}
    '
}

first_line=$(head -n 1 "$state_file")
if [[ "$first_line" == "empty" || "$first_line" =~ ^[0-9a-fA-F:.]+$ ]]; then
    hardware_port=$(current_service)
    original_dns=$(cat "$state_file")
else
    hardware_port="$first_line"
    original_dns=$(tail -n +2 "$state_file")
fi

[ -z "$hardware_port" ] && hardware_port=$(current_service)
[ -z "$hardware_port" ] && exit 1

# Подмены на службе уже нет: её сменили поверх нас (человек вручную, другая
# программа) или она так и не встала. Выбор остаётся как есть, а записанные
# адреса больше ничего не описывают. Прочитать не вышло — возвращаем, как и
# прежде.
if [ -n "$placeholder" ] && current=$(networksetup -getdnsservers "$hardware_port" 2>&1) &&
    [ -n "$current" ] && [[ "$current" != *"** Error"* ]]; then
    if [ "$(tr -d '[:space:]' <<<"$current")" != "$placeholder" ]; then
        echo "DNS of $hardware_port is not the override any more; leaving it as it is"
        rm -f "$state_file"
        exit 0
    fi
fi

# networksetup умеет сообщить об отказе строкой «** Error» и выйти с нулём.
out=$(networksetup -setdnsservers "$hardware_port" $original_dns 2>&1)
code=$?
[ -n "$out" ] && echo "$out"
[ "$code" -eq 0 ] && [[ "$out" == *"** Error"* ]] && code=1

if [ "$code" -ne 0 ]; then
    # Службы, на которой стоит подмена, больше нет (сбросили сеть, удалили
    # адаптер) — вместе с ней ушла и подмена, возвращать некуда. Без этого
    # запись висела бы вечно, а следующая подмена ложилась бы мимо неё.
    # Список — только текущего размещения сети (Location): служба, оставшаяся
    # в другом размещении, тоже считается ушедшей.
    # Не удалось прочитать список — не знаем, поэтому запись держим.
    services=$(networksetup -listallnetworkservices) || exit "$code"
    if ! sed 's/^\*//' <<<"$services" | grep -Fxq -- "$hardware_port"; then
        echo "network service $hardware_port is gone; nothing to restore"
        rm -f "$state_file"
        exit 0
    fi
    exit "$code"
fi

rm -f "$state_file"
exit 0
