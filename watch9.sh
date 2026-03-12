#!/bin/bash

command=$(cat << 'BASH'
cargo run --color always 2>&1 | while IFS= read -r line; do
    printf '%s\n' "$line"
    if [[ "$line" == *"UI loaded"* ]]; then
        sleep 0.5
        hyprctl dispatch movetoworkspace 9,pid:$(pgrep -n jv)
    fi
done
BASH
)

watchexec -r -e rs -- "$command"
