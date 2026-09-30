#!/bin/bash

state_file="${1:-.original_dns.txt}"

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

networksetup -setdnsservers "$hardware_port" $original_dns
code=$?

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
