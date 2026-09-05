#!/bin/bash

function is_valid_ipv4() {
    local ip="$1"
    local IFS='.'
    read -ra parts <<<"$ip"
    [ "${#parts[@]}" -ne 4 ] && return 1
    for part in "${parts[@]}"; do
        [[ ! "$part" =~ ^[0-9]+$ ]] && return 1
        [ "$part" -lt 0 ] || [ "$part" -gt 255 ] && return 1
        [[ "${#part}" -gt 1 && "${part:0:1}" == "0" ]] && return 1
    done
    return 0
}

function is_valid_ipv6() {
    local ip="$1"
    [[ ! "$ip" =~ ^[0-9a-fA-F:]+$ ]] && return 1
    [[ "$ip" =~ :::+ ]] && return 1
    [[ "$(grep -o '::' <<<"$ip" | wc -l)" -gt 1 ]] && return 1
    return 0
}

function is_valid_ip() {
    is_valid_ipv4 "$1" || is_valid_ipv6 "$1"
}

[ $# -lt 1 ] && echo "Usage: $0 <IP address> [state file]" && exit 1
! is_valid_ip "$1" && echo "$1 is not a valid IP address." && exit 1

state_file="${2:-.original_dns.txt}"

nic=$(route -n get default | grep "interface" | awk '{print $2}')
hardware_port=$(networksetup -listnetworkserviceorder | awk -v dev="$nic" '
    /^\([0-9]+\) /{port=$0; sub(/^\([0-9]+\) /, "", port)}
    /\(Hardware Port:/{interface=$NF;sub(/\)/, "", interface); if (interface == dev) {print port; exit}}
')

[ -z "$hardware_port" ] && echo "cannot resolve the network service for $nic" && exit 1

state_written_now=false
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

    tmp_file="$state_file.tmp"
    {
        echo "$hardware_port"
        if [ "$is_valid_dns" = false ]; then
            echo "empty"
        else
            echo "$original_dns"
        fi
    } >"$tmp_file"
    if ! mv -f "$tmp_file" "$state_file"; then
        rm -f "$tmp_file"
        echo "cannot record the original DNS for $hardware_port"
        exit 1
    fi
    state_written_now=true
fi

networksetup -setdnsservers "$hardware_port" "$1"
code=$?

if [ "$code" -ne 0 ]; then
    [ "$state_written_now" = true ] && rm -f "$state_file"
    exit "$code"
fi

exit 0
