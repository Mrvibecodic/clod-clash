#!/bin/bash
case "$1" in
    remove | purge | 0)
        /usr/bin/clash-verge-service-uninstall
        ;;
esac
