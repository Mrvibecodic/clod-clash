#!/bin/bash
case "$1" in
    remove | purge | 0)
        /usr/bin/clod-clash-service-uninstall
        ;;
esac
