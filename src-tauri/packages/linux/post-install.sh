#!/bin/bash
chmod +x /usr/bin/clod-clash-service-install
chmod +x /usr/bin/clod-clash-service-uninstall
chmod +x /usr/bin/clod-clash-service

install_tun_service() {
    command -v systemctl >/dev/null 2>&1 || return 0
    [ -d /run/systemd/system ] || return 0
    if command -v getenforce >/dev/null 2>&1 && [ "$(getenforce 2>/dev/null)" = "Enforcing" ]; then
        mkdir -p /var/lib/clod-clash-service/bin /var/lib/clod-clash-service/cores >/dev/null 2>&1 || true
        if command -v semanage >/dev/null 2>&1; then
            semanage fcontext -a -t bin_t '/var/lib/clod-clash-service/(bin|cores)(/.*)?' >/dev/null 2>&1 || true
        fi
        if command -v chcon >/dev/null 2>&1; then
            chcon -R -t bin_t /var/lib/clod-clash-service/bin /var/lib/clod-clash-service/cores >/dev/null 2>&1 || true
        fi
    fi
    systemctl reset-failed clod-clash-service.service >/dev/null 2>&1 || true
    /usr/bin/clod-clash-service-install >/dev/null 2>&1 || true
    if command -v chcon >/dev/null 2>&1 && [ -f /var/lib/clod-clash-service/bin/clod-clash-service ]; then
        chcon -t bin_t /var/lib/clod-clash-service/bin/clod-clash-service >/dev/null 2>&1 || true
    fi
    systemctl start clod-clash-service.service >/dev/null 2>&1 || true
}

# Служба прежних версий общая с Clash Verge Rev: при нём из её записей о запуске
# уходит наше ядро (иначе после перезагрузки оно поднялось бы вторым), без него
# служба снимается.
forget_our_core_in_legacy_service() {
    ours=$(find /var/lib/clash-verge-service -name desired-state.json -type f \
        -exec grep -l 'io.clodclash.app' {} + 2>/dev/null)
    [ -n "$ours" ] || return 0
    # Останавливается, только если сейчас держит наше ядро: ядро службы — от root,
    # с нашей папкой данных в командной строке, её процесс — родитель.
    held=0
    for pid in $(pgrep -u root -f 'io.clodclash.app' 2>/dev/null); do
        parent=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
        [ -n "$parent" ] || continue
        if [ "$(ps -o comm= -p "$parent" 2>/dev/null)" = clash-verge-ser ]; then
            held=1
        fi
    done
    if [ "$held" = 1 ]; then
        systemctl stop clash-verge-service.service >/dev/null 2>&1 || true
    fi
    printf '%s\n' "$ours" | while IFS= read -r f; do mv -f "$f" "$f.clodclash.bak"; done
    if [ "$held" = 1 ]; then
        systemctl start clash-verge-service.service >/dev/null 2>&1 || true
    fi
}

remove_legacy_service() {
    command -v systemctl >/dev/null 2>&1 || return 0
    if [ -x /usr/bin/clash-verge ]; then
        forget_our_core_in_legacy_service
        return 0
    fi
    if [ -f /etc/systemd/system/clash-verge-service.service ]; then
        systemctl disable --now clash-verge-service.service >/dev/null 2>&1 || true
        rm -f /etc/systemd/system/clash-verge-service.service
        systemctl daemon-reload >/dev/null 2>&1 || true
        systemctl reset-failed clash-verge-service.service >/dev/null 2>&1 || true
    fi
    if command -v semanage >/dev/null 2>&1; then
        semanage fcontext -d '/var/lib/clash-verge-service/bin(/.*)?' >/dev/null 2>&1 || true
    fi
    rm -rf /var/lib/clash-verge-service /run/clash-verge-service
}

install_tun_service
remove_legacy_service
