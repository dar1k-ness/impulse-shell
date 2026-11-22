local home_dir = os.getenv("HOME")

-- Wayland
hl.env("ELECTRON_OZONE_PLATFORM_HINT", "auto")

-- Applications
local xdg_data_dirs_prefix = home_dir .. "/.local/share/flatpak/exports/share:/var/lib/flatpak/exports/share:/usr/local/share:/usr/share:"
local xdg_data_dirs_old = os.getenv("XDG_DATA_DIRS") or ""
-- reload runs this again in the same process: do not prepend twice
if xdg_data_dirs_old:sub(1, #xdg_data_dirs_prefix) == xdg_data_dirs_prefix then
    xdg_data_dirs_old = xdg_data_dirs_old:sub(#xdg_data_dirs_prefix + 1)
end
hl.env("XDG_DATA_DIRS", xdg_data_dirs_prefix .. xdg_data_dirs_old)

-- Themes
hl.env("QT_QPA_PLATFORM", "wayland;xcb")
hl.env("QT_QPA_PLATFORMTHEME", "kde")
hl.env("XDG_MENU_PREFIX", "plasma-")

