-- Monitor: 1920x1080 @ 100 Hz
hl.monitor({
    output = "HDMI-A-2",
    mode = "1920x1080@100",
    position = "0x0",
    scale = 1
})

-- On kernel 7.3 the hardware cursor is invisible unless the patched aquamarine
-- (~/build/aquamarine, GBM.cpp: linear cursor buffers) is the one loaded; aquamarine and
-- the hypr* stack are pinned in /etc/pacman.conf IgnorePkg. Any other build: software cursor.
local function patchedAquamarineLoaded()
    local f = io.open("/proc/self/maps")
    if not f then return false end
    local maps = f:read("a")
    f:close()
    local line = maps:match("[^\n]*libaquamarine[^\n]*")
    return line ~= nil and line:find("libaquamarine.so.0.15.1", 1, true) ~= nil and not line:find("(deleted)", 1, true)
end

-- Keyboard layouts: English + Russian (switch with SUPER + Space, see custom/keybinds.lua)
hl.config({
    input = {
        kb_layout = "us,ru"
    },
    cursor = {
        no_hardware_cursors = 0
    }
})
