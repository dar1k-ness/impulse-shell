#!/usr/bin/env bash

QUICKSHELL_CONFIG_NAME="impulse-shell"
XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
XDG_STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
CONFIG_DIR="$XDG_CONFIG_HOME/rustshell/$QUICKSHELL_CONFIG_NAME"
CACHE_DIR="$XDG_CACHE_HOME/rustshell"
STATE_DIR="$XDG_STATE_HOME/rustshell"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

term_alpha=100 #Set this to < 100 make all your terminals transparent
# sleep 0 # idk i wanted some delay or colors dont get applied properly
if [ ! -d "$STATE_DIR"/user/generated ]; then
  mkdir -p "$STATE_DIR"/user/generated
fi
cd "$CONFIG_DIR" || exit

# Never fill templates from an empty or broken scheme: that writes unfilled `#$primary #` placeholders
if ! grep -q '^\$primary: #' "$STATE_DIR"/user/generated/material_colors.scss 2>/dev/null; then
  echo "[applycolor.sh] material_colors.scss is empty or invalid, keeping current terminal colors" >&2
  exit 1
fi

colornames=''
colorstrings=''
colorlist=()
colorvalues=()

colornames=$(cat $STATE_DIR/user/generated/material_colors.scss | cut -d: -f1)
colorstrings=$(cat $STATE_DIR/user/generated/material_colors.scss | cut -d: -f2 | cut -d ' ' -f2 | cut -d ";" -f1)
IFS=$'\n'
colorlist=($colornames)     # Array of color names
colorvalues=($colorstrings) # Array of color values

apply_kitty() {  
  # Check if terminal escape sequence template exists
  if [ ! -f "$SCRIPT_DIR/terminal/kitty-theme.conf" ]; then
    echo "Template file not found for Kitty theme. Skipping that."
    return
  fi
  # Copy template
  mkdir -p "$STATE_DIR"/user/generated/terminal
  local out="$STATE_DIR"/user/generated/terminal/kitty-theme.conf
  cp "$SCRIPT_DIR/terminal/kitty-theme.conf" "$out.tmp"
  # Apply colors
  for i in "${!colorlist[@]}"; do
    sed -i "s/${colorlist[$i]} #/${colorvalues[$i]#\#}/g" "$out.tmp"
  done
  if grep -q '#\$' "$out.tmp"; then
    echo "[applycolor.sh] Unfilled colors in kitty theme, keeping the current one" >&2
    rm -f "$out.tmp"
    return
  fi
  mv "$out.tmp" "$out"

  # Reload
  if ! pgrep -f kitty >/dev/null; then
    return
  fi
  kill -SIGUSR1 $(pidof kitty)
}

apply_anyterm() {
  # Check if terminal escape sequence template exists
  if [ ! -f "$SCRIPT_DIR/terminal/sequences.txt" ]; then
    echo "Template file not found for Terminal. Skipping that."
    return
  fi
  # Copy template
  mkdir -p "$STATE_DIR"/user/generated/terminal
  local out="$STATE_DIR"/user/generated/terminal/sequences.txt
  cp "$SCRIPT_DIR/terminal/sequences.txt" "$out.tmp"
  # Apply colors
  for i in "${!colorlist[@]}"; do
    sed -i "s/${colorlist[$i]} #/${colorvalues[$i]#\#}/g" "$out.tmp"
  done

  sed -i "s/\$alpha/$term_alpha/g" "$out.tmp"
  if grep -q '#\$' "$out.tmp"; then
    echo "[applycolor.sh] Unfilled colors in terminal sequences, keeping the current ones" >&2
    rm -f "$out.tmp"
    return
  fi
  mv "$out.tmp" "$out"

  for file in /dev/pts/*; do
    if [[ $file =~ ^/dev/pts/[0-9]+$ ]]; then
      {
      cat "$STATE_DIR"/user/generated/terminal/sequences.txt >"$file"
      } & disown || true
    fi
  done
}

apply_term() {
  apply_anyterm &
  apply_kitty &
}

# Check if terminal theming is enabled in config
CONFIG_FILE="$XDG_CONFIG_HOME/impulse-shell/config.json"
if [ -f "$CONFIG_FILE" ]; then
  enable_terminal=$(jq -r '.appearance.wallpaperTheming.enableTerminal' "$CONFIG_FILE")
  if [ "$enable_terminal" = "true" ]; then
    apply_term &
  fi
else
  echo "Config file not found at $CONFIG_FILE. Applying terminal theming by default."
  apply_term &
fi

# apply_qt & # Qt theming is already handled by kde-material-colors
