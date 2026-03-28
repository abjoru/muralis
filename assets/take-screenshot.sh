#!/usr/bin/env bash
# Takes a screenshot of muralis-gui on an empty workspace using a safe demo config.
# Usage: ./assets/take-screenshot.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT="$SCRIPT_DIR/screenshot.png"
DEMO_CONFIG="$SCRIPT_DIR/demo-config.toml"
EMPTY_WS="99"

# Set up isolated XDG dirs so we don't touch real config/data
DEMO_XDG=$(mktemp -d)
mkdir -p "$DEMO_XDG/muralis"
cp "$DEMO_CONFIG" "$DEMO_XDG/muralis/config.toml"

trap 'rm -rf "$DEMO_XDG"' EXIT

# Remember current workspace
CURRENT_WS=$(hyprctl activeworkspace -j | jq -r '.id')

echo "Current workspace: $CURRENT_WS"
echo "Switching to empty workspace $EMPTY_WS..."

# Add temporary window rules to float and resize muralis-gui
hyprctl keyword windowrule 'float on, match:title ^(Muralis)$'
hyprctl keyword windowrule 'size 1400 900, match:title ^(Muralis)$'
hyprctl keyword windowrule 'center on, match:title ^(Muralis)$'

# Switch to empty workspace
hyprctl dispatch workspace "$EMPTY_WS"
sleep 0.5

# Launch muralis-gui with isolated config and initial search
echo "Launching muralis-gui..."
XDG_CONFIG_HOME="$DEMO_XDG" muralis-gui --query "landscape" &
GUI_PID=$!

# Wait for window to appear and thumbnails to load
sleep 8

# Get window geometry
echo "Capturing muralis-gui window..."
GEOM=$(hyprctl clients -j | jq -r '.[] | select(.pid == '"$GUI_PID"' or .initialTitle == "muralis-gui" or .class == "muralis-gui") | "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"' | head -1)

if [ -n "$GEOM" ] && [ "$GEOM" != "null" ]; then
  echo "Window geometry: $GEOM"
  grim -g "$GEOM" "$OUTPUT"
else
  echo "Could not find window, capturing full screen..."
  grim "$OUTPUT"
fi

# Cleanup
echo "Cleaning up..."
kill "$GUI_PID" 2>/dev/null || true

sleep 0.3
hyprctl dispatch workspace "$CURRENT_WS"

echo "Screenshot saved to: $OUTPUT"
