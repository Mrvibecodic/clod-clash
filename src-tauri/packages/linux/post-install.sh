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
    state=/var/lib/clash-verge-service
    # Путь к ядру у обоих пакетов одинаковый (/usr/bin/verge-mihomo): наше узнаётся
    # по записи службы о том, из какой папки данных пришёл последний запуск.
    key=""
    if grep -qs 'io.clodclash.app' "$state/active-owner.json"; then
        key=$(sed -n 's/.*"owner_key"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$state/active-owner.json" | head -n 1)
    fi
    ours=$(find "$state" -name desired-state.json -type f -exec grep -l 'io.clodclash.app' {} + 2>/dev/null)
    if [ -n "$key" ]; then
        ours=$(printf '%s\n%s\n%s\n' "$ours" "$state/active-owner.json" "$state/users/$key/desired-state.json" | grep . | sort -u)
    fi
    [ -n "$ours" ] || return 0
    # Останавливается, только если сейчас держит наше ядро: ядро от root, его родитель —
    # процесс службы, в командной строке — папка этой записи или наша папка данных.
    held=0
    for pid in $(pgrep -u root -f verge-mihomo 2>/dev/null); do
        parent=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
        [ -n "$parent" ] || continue
        [ "$(ps -o comm= -p "$parent" 2>/dev/null)" = clash-verge-ser ] || continue
        cmd=$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null)
        case "$cmd" in *io.clodclash.app*) held=1 ;; esac
        if [ -n "$key" ]; then
            case "$cmd" in *"$state/users/$key/"*) held=1 ;; esac
        fi
    done
    if [ "$held" = 1 ]; then
        systemctl stop clash-verge-service.service >/dev/null 2>&1 || true
    fi
    printf '%s\n' "$ours" | while IFS= read -r f; do
        [ -f "$f" ] && mv -f "$f" "$f.clodclash.bak"
    done
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
