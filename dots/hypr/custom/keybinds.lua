hl.bind("CTRL+SUPER+ALT+Slash", hl.dsp.exec_cmd("xdg-open ~/.config/hypr/custom/keybinds.lua"), {description = "Edit user keybinds"} )

-- Switch keyboard layout EN/RU. The global interrupt keeps the quickshell search from opening when SUPER is released
hl.bind("SUPER + Space", hl.dsp.global("rustshell:searchToggleReleaseInterrupt"))
hl.bind("SUPER + Space", hl.dsp.exec_cmd("hyprctl switchxkblayout all next"), { description = "Keyboard: Switch layout EN/RU", locked = true })

-- Volume keys: cap at 100% (the dots default `-l 1.5` allows 150%)
hl.unbind("XF86AudioRaiseVolume")
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 2%+ -l 1"),
    { locked = true, repeating = true })
hl.bind("SUPER + ALT + E", hl.dsp.global("rustshell:editModeToggle"), { description = "Shell: Toggle edit mode" })
