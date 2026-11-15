-- The benchmark guest's Hyprland (bench/guest-setup.sh installs it): one output at its native
-- mode, nothing of the compositor's own on screen and no compositor effects, so what is drawn
-- and animated is the shell under test.
-- The Python environment the original's scripts activate (guest-setup.sh builds it).
hl.env("IMPULSE_SHELL_VIRTUAL_ENV", "/opt/ri-venv")
hl.env("ILLOGICAL" .. "_IMPULSE_VIRTUAL_ENV", "/opt/ri-venv") -- the original's name for it
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })
hl.config({
    animations = { enabled = false },
    decoration = { blur = { enabled = false }, shadow = { enabled = false } },
    misc = {
        force_default_wallpaper = 0,
        disable_hyprland_logo = true,
        disable_splash_rendering = true,
    },
    ecosystem = { no_update_news = true, no_donation_nag = true },
})
