#!/usr/bin/env python3
"""(Re)write crates/colorgen/tests/kde/cases/ (inputs of the golden runs)."""
import os
import shutil

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../crates/colorgen/tests/kde/cases")
PLASMA = os.path.join(ROOT, "../plasma")

OWNER = """[CUSTOM]
monitor = 0
file = ~/.local/state/impulse-shell/user/generated/wallpaper/path.txt
custom_colors_list = #ED8796 #A6DA95 #EED49F #8AADF4 #F5BDE6 #8BD5CA #f5a97f
ncolor = 0
iconslight = breeze-plus
iconsdark = breeze-plus-dark
pywal=False
scheme_variant = 5
chroma_multiplier = 1
tone_multiplier = 1
"""


def conf(drop=(), extra="", replace=None):
    lines = [l for l in OWNER.splitlines() if l.split("=")[0].strip() not in drop]
    if replace:
        lines = [replace.get(l.split("=")[0].strip(), l) for l in lines]
    return "\n".join(lines) + "\n" + extra


def case(name, mode, seed="#5E7A91", sv=5, config=OWNER, home=None):
    d = os.path.join(ROOT, name)
    with open(os.path.join(mk(d), "args"), "w") as f:
        f.write("\n".join([f"-{mode[0]}", "--color", seed, "-sv", str(sv)]) + "\n")
    if config is not None:
        with open(os.path.join(d, "config.conf"), "w") as f:
            f.write(config)
    for rel, text in (home or {}).items():
        p = os.path.join(d, "home", rel)
        mk(os.path.dirname(p))
        with open(p, "w") as f:
            f.write(text)


def mk(d):
    os.makedirs(d, exist_ok=True)
    return d


KDEGLOBALS = """# user comment kept?
[$Version]
update_info=filepicker.upd:filepicker-remove-old-previews-entry

[General]
ColorScheme=MaterialYouDark
XftHintStyle=hintslight
font=Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1

; another comment
[Icons]
Theme=breeze-plus

[KDE]
LookAndFeelPackage=org.kde.breezedark.desktop
SingleClick=false
widgetStyle=Breeze

[WM]
activeBackground=39,44,49
activeFont=Noto Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1
activeForeground=252,252,252

[Colors:View]
BackgroundNormal=1,2,3
"""

KONSOLE_COLORSCHEME = """[Background]
Color=1,2,3

[Color0]
Color=4,5,6

[General]
Description=Old
Opacity=1
Wallpaper=/some/wallpaper.png

[Extra]
Custom=yes
"""

MINE_PROFILE = """[Appearance]
ColorScheme=Breeze
Font=JetBrains Mono,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1

[General]
Command=/bin/zsh
Name=Mine
Parent=FALLBACK/

[Scrolling]
HistoryMode=2
"""

USER_TEMPLATE = """{{literal}} {wallpaper} {alpha}
{background} {backgroundIntense} {backgroundFaint} {foreground} {foregroundIntense} {foregroundFaint} {cursor}
{color0} {color8} {color15} {color16} {color23}
{color1.rgb} {color2.xrgba} {color3.rgba} {color4.alpha} {color5.octal} {color6.octal_strip} {color7.strip}
{background.hex_color} {background.alpha_num}
"""


def main():
    if os.path.isdir(ROOT):
        shutil.rmtree(ROOT)
    for seed in ["#E1E7EC", "#5E7A91", "#B3261E", "#00FF00", "#FFFFFF", "#000000", "#7D5260"]:
        for mode in ["dark", "light"]:
            case(f"seed-{seed[1:].lower()}-{mode}", mode, seed=seed)
    for sv in range(9):
        case(f"sv{sv}-dark", "dark", sv=sv)
    for mode in ["dark", "light"]:
        case(f"defaults-{mode}", mode, config=None)
    for mode in ["dark", "light"]:
        case(f"pre-kdeglobals-{mode}", mode, home={".config/kdeglobals": KDEGLOBALS})
    case("pre-konsolerc-nodefault", "dark", home={".config/konsolerc": "[MainWindow]\nMenuBar=Disabled\n\n[Desktop Entry]\nFoo=bar\n"})
    case("pre-konsolerc-nosection", "dark", home={".config/konsolerc": "[MainWindow]\nMenuBar=Disabled\n"})
    case("pre-konsolerc-empty", "dark", home={".config/konsolerc": "[Desktop Entry]\nDefaultProfile=\n\n[MainWindow]\nMenuBar=Disabled\n"})
    case("pre-konsolerc-mine", "dark", home={
        ".config/konsolerc": "[Desktop Entry]\nDefaultProfile=Mine.profile\n\n[MainWindow]\nMenuBar=Disabled\n",
        ".local/share/konsole/Mine.profile": MINE_PROFILE,
    })
    case("pre-konsole-colorscheme", "dark", home={".local/share/konsole/MaterialYou.colorscheme": KONSOLE_COLORSCHEME})
    case("pre-klassy-sbe", "dark", home={
        ".config/klassy/klassyrc": "[TitleBarOpacity]\nActiveTitleBarOpacity=50\nOpaqueMaximizedTitleBars=true\n\n[Windeco]\nButtonSize=Small\n",
        ".config/sierrabreezeenhancedrc": "[Windeco]\nBackgroundOpacity=40\nButtonStyle=macOS\n\n[Common]\nShadowSize=ShadowLarge\n",
    })
    case("pre-klassy-only", "dark", home={".config/klassy/klassyrc": "[TitleBarOpacity]\nActiveTitleBarOpacity=50\n"})
    breezerc = "[Windeco]\nButtonCloseActiveColor=1,2,3\nButtonStyle=macOS\n\n[Common]\nShadowSize=ShadowLarge\n"
    klassyrc = "[WindowOutlineStyle]\nThinWindowOutlineStyleActive=WindowOutlineNone\nThinWindowOutlineThickness=1\n\n[Windeco]\nButtonSize=Small\n"
    kwinrules = "[1]\nDescription=Firefox\nwmclass=firefox\nwmclassmatch=1\n\n[General]\ncount=1\nrules=1\n"
    deco = "sierra_breeze_buttons_color = True\nklassy_windeco_outline = True\ndarker_window_list = kitty org.kde.dolphin\n"
    for mode in ["dark", "light"]:
        case(f"cfg-deco-{mode}", mode, config=conf(extra=deco), home={
            ".config/breezerc": breezerc, ".config/klassy/klassyrc": klassyrc, ".config/kwinrulesrc": kwinrules})
    case("cfg-deco-nofiles", "dark", config=conf(extra=deco))
    case("cfg-deco-nosection", "dark", config=conf(extra=deco), home={
        ".config/breezerc": "[Common]\nShadowSize=ShadowLarge\n", ".config/klassy/klassyrc": "[Windeco]\nButtonSize=Small\n"})
    case("cfg-darker-rerun", "dark", config=conf(extra="darker_window_list = kitty\n"), home={
        ".config/kwinrulesrc": "[1]\nDescription=Darker Titlebar - generated by kde-material-you-colors, do not rename\n"
        "decocolor=MaterialYouDark_darker_titlebar\ndecocolorrule=2\nwmclass=(foot)\nwmclassmatch=3\n\n[General]\ncount=1\nrules=1\n"})
    case("cfg-titlebar-opacity", "dark", config=conf(extra="titlebar_opacity = 70\ntitlebar_opacity_dark = 55\n"), home={
        ".config/klassy/klassyrc": "[TitleBarOpacity]\nActiveTitleBarOpacity=50\n",
        ".config/sierrabreezeenhancedrc": "[Windeco]\nBackgroundOpacity=40\n"})
    case("pre-klassy-nosection", "dark", home={".config/klassy/klassyrc": "[Windeco]\nButtonSize=Small\n"})
    case("cfg-titlebar-opacity-nosection", "dark", config=conf(extra="titlebar_opacity = 70\ntitlebar_opacity_dark = 55\n"), home={
        ".config/klassy/klassyrc": "[Windeco]\nButtonSize=Small\n",
        ".config/sierrabreezeenhancedrc": "[Common]\nShadowSize=ShadowLarge\n"})
    opac = ("toolbar_opacity = 80\ntoolbar_opacity_dark = 60\nkonsole_opacity = 90\nkonsole_opacity_dark = 70\n"
            "konsole_blur = True\nlight_blend_multiplier = 2\ndark_blend_multiplier = 0.5\n")
    for mode in ["dark", "light"]:
        case(f"cfg-opacity-blend-{mode}", mode, config=conf(extra=opac))
    case("cfg-multipliers", "dark", config=conf(replace={"chroma_multiplier": "chroma_multiplier = 1.5", "tone_multiplier": "tone_multiplier = 0.8"}))
    case("cfg-ncolor3", "dark", config=conf(replace={"ncolor": "ncolor = 3"}))
    for mode in ["dark", "light"]:
        case(f"cfg-no-icons-{mode}", mode, config=conf(drop=("iconslight", "iconsdark")))
    case("cfg-no-custom-colors", "dark", config=conf(drop=("custom_colors_list",)))
    pywal = {"pywal": "pywal = True"}
    for mode in ["dark", "light"]:
        case(f"pywal-{mode}", mode, config=conf(replace=pywal))
    case("pywal-nocustom-light", "light", config=conf(drop=("custom_colors_list",), replace=pywal))
    case("pywal-light-on-dark", "dark", config=conf(replace=pywal, extra="pywal_light = True\n"))
    case("pywal-dark-on-light", "light", config=conf(drop=("custom_colors_list",), replace=pywal, extra="pywal_light = False\n"))
    case("pywal-follows-scheme", "dark", config=conf(replace=pywal, extra="pywal_light = True\npywal_follows_scheme = True\n"))
    case("cfg-pywal-light-only", "dark", config=conf(extra="pywal_light = True\n"))
    case("pywal-user-templates", "dark", seed="#B3261E", config=conf(replace=pywal), home={
        ".config/wal/templates/custom.txt": USER_TEMPLATE,
        ".config/wal/templates/colors": "overridden {color1} {foreground.strip}\n",
    })
    case("cfg-icons-missing", "dark", config=conf(replace={"iconsdark": "iconsdark = no-such-theme"}))
    # Plasma's accent colour (kdeglobals [General] AccentColor) on top of the scheme.
    accents = [
        ("3daee9-dark", "dark", "#5E7A91", "61,174,233"),
        ("3daee9-light", "light", "#5E7A91", "61,174,233"),
        ("low-dark", "dark", "#5E7A91", "40,40,48"),
        ("low-light", "light", "#5E7A91", "245,240,250"),
        ("mid-light", "light", "#B3261E", "128,128,128"),
        ("red-dark", "dark", "#00FF00", "255,0,0"),
        ("hex-light", "light", "#7D5260", "#e95420"),
        ("alpha-dark", "dark", "#7D5260", "233,84,32,128"),
        ("transparent-dark", "dark", "#5E7A91", "0,0,0,0"),
    ]
    for name, mode, seed, accent in accents:
        kg = KDEGLOBALS.replace("[General]\n", f"[General]\nAccentColor={accent}\n")
        case(f"accent-{name}", mode, seed=seed, home={".config/kdeglobals": kg})


def plasma_cases():
    """tests/kde/plasma/cases: plasma-apply-colorscheme alone (plasma.sh) on accent settings of
    the scheme that kde-material-you-colors never writes."""
    root = os.path.join(PLASMA, "cases")
    if os.path.isdir(root):
        shutil.rmtree(root)
    cases = [
        ("md-tint-default", "MaterialYouDark", ["TintFactor=0.15"], "61,174,233"),
        ("md-tint-low", "MaterialYouDark", ["TintFactor=0.4"], "40,40,48"),
        ("ml-tint-red", "MaterialYouLight", ["TintFactor=0.25"], "255,0,0"),
        ("ml-tint-alpha", "MaterialYouLight", ["TintFactor=0.15"], "233,84,32,128"),
        ("md-titlebar", "MaterialYouDark", ["TitlebarIsAccentColored=true"], "61,174,233"),
        ("ml-titlebar-low", "MaterialYouLight", ["TitlebarIsAccentColored=true"], "245,240,250"),
        ("md-legacy-titlebar", "MaterialYouDark", ["accentActiveTitlebar=true"], "233,84,32"),
        ("md-titlebar-off", "MaterialYouDark", ["TitlebarIsAccentColored=false", "accentActiveTitlebar=true"], "233,84,32"),
        ("ml-titlebar-tint", "MaterialYouLight", ["TintFactor=0.3", "TitlebarIsAccentColored=true"], "#e95420"),
        ("bd-plain", "BreezeDark", [], "61,174,233"),
        ("bd-tint", "BreezeDark", ["TintFactor=0.2"], "146,110,228"),
        ("bl-titlebar", "BreezeLight", ["TitlebarIsAccentColored=true"], "0,150,136"),
        ("bl-low", "BreezeLight", [], "250,250,250"),
        ("bc-tint-titlebar", "BreezeClassic", ["TintFactor=0.5", "TitlebarIsAccentColored=true"], "200,30,90"),
    ]
    for name, scheme, extra, accent in cases:
        d = mk(os.path.join(root, name))
        with open(os.path.join(PLASMA, "schemes", scheme + ".colors"), encoding="utf-8") as f:
            text = f.read()
        text = text.replace("[General]\n", "[General]\n" + "".join(l + "\n" for l in extra), 1)
        text = text.replace(f"ColorScheme={scheme}\n", "ColorScheme=Test\n")
        with open(os.path.join(d, "Test.colors"), "w", encoding="utf-8") as f:
            f.write(text)
        with open(os.path.join(d, "kdeglobals"), "w") as f:
            f.write(KDEGLOBALS.replace("[General]\n", f"[General]\nAccentColor={accent}\n"))


main()
plasma_cases()
