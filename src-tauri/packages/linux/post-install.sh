#!/bin/bash
chmod +x /usr/bin/clash-verge-service-install
chmod +x /usr/bin/clash-verge-service-uninstall
chmod +x /usr/bin/clash-verge-service

install_tun_service() {
    command -v systemctl >/dev/null 2>&1 || return 0
    [ -d /run/systemd/system ] || return 0
    if command -v getenforce >/dev/null 2>&1 && [ "$(getenforce 2>/dev/null)" = "Enforcing" ]; then
        mkdir -p /var/lib/clash-verge-service/bin >/dev/null 2>&1 || true
        if command -v semanage >/dev/null 2>&1; then
            semanage fcontext -a -t bin_t '/var/lib/clash-verge-service/bin(/.*)?' >/dev/null 2>&1 || true
        fi
        if command -v chcon >/dev/null 2>&1; then
            chcon -R -t bin_t /var/lib/clash-verge-service/bin >/dev/null 2>&1 || true
        fi
    fi
    systemctl reset-failed clash-verge-service.service >/dev/null 2>&1 || true
    /usr/bin/clash-verge-service-install >/dev/null 2>&1 || true
    if command -v chcon >/dev/null 2>&1 && [ -f /var/lib/clash-verge-service/bin/clash-verge-service ]; then
        chcon -t bin_t /var/lib/clash-verge-service/bin/clash-verge-service >/dev/null 2>&1 || true
    fi
    systemctl start clash-verge-service.service >/dev/null 2>&1 || true
}

install_tun_service
