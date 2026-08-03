#!/bin/bash

# clod: состояние пишем НЕ в текущий каталог (это ресурсы приложения, они
# переживают не каждое обновление и могут быть только для чтения), а в файл,
# путь к которому передаёт приложение вторым аргументом. В файле две строки:
# имя сетевого сервиса, на котором мы подменили DNS, и сам исходный DNS.
# Имя сервиса важно: после переключения Wi-Fi ↔ Ethernet «текущий» сервис уже
# другой, и восстановление ушло бы не туда.

# 验证IPv4地址格式
function is_valid_ipv4() {
    local ip=$1
    local IFS='.'
    local -a octets

    [[ ! $ip =~ ^([0-9]+\.){3}[0-9]+$ ]] && return 1
    read -r -a octets <<<"$ip"
    [ "${#octets[@]}" -ne 4 ] && return 1

    for octet in "${octets[@]}"; do
        if ! [[ "$octet" =~ ^[0-9]+$ ]] || ((octet < 0 || octet > 255)); then
            return 1
        fi
    done
    return 0
}

# 验证IPv6地址格式
function is_valid_ipv6() {
    local ip=$1
    if [[ ! $ip =~ ^([0-9a-fA-F]{0,4}:){1,7}[0-9a-fA-F]{0,4}$ ]] &&
        [[ ! $ip =~ ^(([0-9a-fA-F]{0,4}:){0,7}:|(:[0-9a-fA-F]{0,4}:){0,6}:[0-9a-fA-F]{0,4})$ ]]; then
        return 1
    fi
    return 0
}

# 验证IP地址是否为有效的IPv4或IPv6
function is_valid_ip() {
    is_valid_ipv4 "$1" || is_valid_ipv6 "$1"
}

# 检查参数
[ $# -lt 1 ] && echo "Usage: $0 <IP address> [state file]" && exit 1
! is_valid_ip "$1" && echo "$1 is not a valid IP address." && exit 1

state_file="${2:-.original_dns.txt}"

# 获取网络接口和硬件端口
nic=$(route -n get default | grep "interface" | awk '{print $2}')
# 从网络服务列表中获取硬件端口
hardware_port=$(networksetup -listnetworkserviceorder | awk -v dev="$nic" '
    /^\([0-9]+\) /{port=$0; sub(/^\([0-9]+\) /, "", port)} 
    /\(Hardware Port:/{interface=$NF;sub(/\)/, "", interface); if (interface == dev) {print port; exit}}
')

[ -z "$hardware_port" ] && echo "cannot resolve the network service for $nic" && exit 1

# clod: оригинал запоминаем ОДИН раз. Если файл уже есть, значит подменяли мы
# же — перечитывать «текущий» DNS нельзя, иначе нашей же подменой затрём
# настоящую настройку пользователя и восстанавливать будет нечего.
if [ ! -f "$state_file" ]; then
    original_dns=$(networksetup -getdnsservers "$hardware_port")

    is_valid_dns=false
    for ip in $original_dns; do
        ip=$(echo "$ip" | tr -d '[:space:]')
        if [ -n "$ip" ] && (is_valid_ipv4 "$ip" || is_valid_ipv6 "$ip"); then
            is_valid_dns=true
            break
        fi
    done

    {
        echo "$hardware_port"
        if [ "$is_valid_dns" = false ]; then
            echo "empty"
        else
            echo "$original_dns"
        fi
    } >"$state_file"
fi

networksetup -setdnsservers "$hardware_port" "$1"
