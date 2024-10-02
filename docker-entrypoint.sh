#!/bin/sh
set -e

if [ "$1" = "sidelb-daemon" ] || [ -z "$1" ]; then
    sidelb_args=""

    if [ -n "$SIDELB_BIND_ADDR" ]; then
        sidelb_args="$SIDELB_BIND_ADDR"
    else
        echo "Error: SIDELB_BIND_ADDR environment variable is not set and is mandatory." >&2
        echo "Example: -e SIDELB_BIND_ADDR=\"0.0.0.0:5432\"" >&2
        exit 1
    fi

    if [ -n "$SIDELB_BACKENDS" ]; then
        # SIDELB_BACKENDS should be a comma-separated string e.g., "10.0.0.1:80,10.0.0.2:80"
        # Prepend "backends=" to form the argument "backends=10.0.0.1:80,10.0.0.2:80"
        sidelb_args="$sidelb_args backends=$SIDELB_BACKENDS"
    fi

    sidelb_effective_mode="${SIDELB_MODE:-round-robin}"
    sidelb_args="$sidelb_args mode=$sidelb_effective_mode"

    sidelb_effective_proto="${SIDELB_PROTO:-tcp}"
    sidelb_args="$sidelb_args proto=$sidelb_effective_proto"

    if [ -n "$SIDELB_RING_DOMAIN" ]; then
        sidelb_args="$sidelb_args ring_domain=$SIDELB_RING_DOMAIN"
    fi

    echo "Starting sidelb configured by environment variables..."
    echo "Executing: /usr/local/bin/sidelb $sidelb_args"
    exec /usr/local/bin/sidelb $sidelb_args

elif [ "$1" = "bash" ] || [ "$1" = "sh" ] || [ "$1" = "help" ] || [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    if [ "$1" = "help" ] || [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
        echo "Displaying sidelb help via entrypoint:"
        exec /usr/local/bin/sidelb --help
    else
        # shellcheck disable=SC2145
        echo "Executing command: $@"
        exec "$@"
    fi
else
    echo "Received command: '$1'. Assuming it's intended for direct execution or a sidelb override."
    echo "Note: Primary configuration for sidelb in this image is via SIDELB_xxx environment variables."
    # shellcheck disable=SC2145
    echo "Executing: /usr/local/bin/sidelb $@"
    exec /usr/local/bin/sidelb "$@"
fi
