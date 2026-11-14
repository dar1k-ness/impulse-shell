<div align="center">
    <h1>【 rusty-impulse 】</h1>
    <h3>end-4's illogical-impulse without Quickshell and without Qt</h3>
</div>

<div align="center">
    <h2>• overview •</h2>
    <h3></h3>
</div>

<details open>
  <summary>What this is/isn't</summary>

  - The graphical shell of [end_4's Hyprland dotfiles](https://github.com/end-4/dots-hyprland) (illogical-impulse): bar, sidebars, overview, lock screen, notifications, settings
  - One Rust binary, `rusty-impulse`, that reads the shell's QML (`qml`) and draws it itself. It links no Qt, no KDE and no Quickshell
  - Meant to look and behave as the original does. Where it does not: [docs/original-differences.md](docs/original-differences.md), [docs/quickshell-differences.md](docs/quickshell-differences.md), [docs/qt-differences.md](docs/qt-differences.md)
  - NOT the dotfiles: Hyprland's configuration, fonts and the other programs stay those of the original's setup
  - NOT a system setup script: `install.sh` installs the packages the build needs and nothing else. No graphic drivers, no zram setup, etc.
  - NOT a mirror: the QML is this project's own source now, nothing is taken from the original's updates

</details>

<details>
  <summary>Notable features</summary>

  - **Overview**: Shows open apps with live previews
  - **AI**: Gemini, Ollama, and more
  - **QoL**: screen translation, anti-flashbang, Google Lens
  - **Material themes**: Choose your wallpaper, done, enjoy
  - **No JavaScript at run time**: the JavaScript in the QML is compiled to Rust when the shell is built ([docs/aot.md](docs/aot.md))
  - **Installs with a way back**: the previous release is kept, `tools/install.sh --rollback` returns to it

</details>

<details>
  <summary>Installation</summary>

   - Have the original's dotfiles installed first: see [its wiki](https://ii.clsty.link/en/ii-qs/01setup/)
   - On Arch Linux, clone this repo and run `./install.sh`. That one command installs the packages the build needs, builds, installs to `~/.local`, brings your settings over from the original, makes Hyprland start this shell and starts it in the session you are in
     - If the shell does not come up, your files are put back and the shell that ran is started again
     - Nothing under the original's names is changed or removed. `tools/carry-over.sh --dots old` puts your files back
     - `tools/install.sh --rollback` goes back to the release the last install replaced
     - The same steps one by one, and other systems: [docs/install.md](docs/install.md), [docs/build.md](docs/build.md)
   - Hyprland starts the shell as `rusty-impulse -c rusty-impulse`
   - A binary runs only the QML tree it was built from. After the tree changes, install again
   - **Keybinds**: Those of the original. Important ones:
     - `Super`+`/` = keybind list
     - `Super`+`Enter` = terminal

</details>

<details>
  <summary>Software overview</summary>

  | Software | Purpose |
  | ------------- | ------------- |
  | [Hyprland](https://github.com/hyprwm/hyprland) | The compositor (manages and renders windows) |
  | `rusty-impulse` | This project: the status bar, sidebars, etc. In place of Quickshell and Qt |
  | Others | See the original's [deps-info.md](https://github.com/end-4/dots-hyprland/blob/main/sdata/deps-info.md) |

</details>

<details>
  <summary>Development</summary>

  | Topic | Where |
  | ------------- | ------------- |
  | What is in which directory | [docs/layout.md](docs/layout.md) |
  | Building, and what makes a build reproducible | [docs/build.md](docs/build.md) |
  | Installing, rolling back, uninstalling | [docs/install.md](docs/install.md) |
  | Tests, the gate (`tools/ci/gate --workspace`), log and debug switches | [docs/testing.md](docs/testing.md), [docs/ci.md](docs/ci.md), [docs/env.md](docs/env.md) |
  | How the JavaScript becomes Rust | [docs/aot.md](docs/aot.md), [docs/native-types.md](docs/native-types.md) |
  | Clocks, threads, measurements | [docs/clocks.md](docs/clocks.md), [docs/threads.md](docs/threads.md), [docs/benchmarks.md](docs/benchmarks.md) |
  | Code taken from other projects | [docs/third-party.md](docs/third-party.md) |

  Branches: `main` is the stable line. `impulse-next` holds postponed features.

</details>

<div align="center">
    <h2>• screenshots •</h2>
    <h3></h3>
</div>

Widget system: its own, in Rust | The look is the original's, and so are these screenshots

| AI, settings app | Some widgets |
|:---|:---------------|
| <img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/5d4e7d07-d0b4-4406-a4c9-ed7ba90e3fe4" /> | <img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/6a32395f-9437-4192-8faf-2951a9e84cbe" /> |
| Window management | wow look its orange |
| <img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/c51bed8b-3670-4d4c-9074-873be224fb8e" /> | <img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/98703a66-0743-439f-a721-cef7afa6ab95" /> |

<div align="center">
    <h2>• thank you •</h2>
    <h3></h3>
</div>

 - [@end-4](https://github.com/end-4) for illogical-impulse: the look, the QML and the scripts of this shell are his, and this page follows the one of his repository
 - Everyone [thanked there](https://github.com/end-4/dots-hyprland#-thank-you-): [@clsty](https://github.com/clsty), [@midn8hustlr](https://github.com/midn8hustlr), [@outfoxxed](https://github.com/outfoxxed/) and the others
 - Quickshell and [Qt](https://www.qt.io/): what this shell is compared with, test by test

<div align="center">
    <h2>• inspirations/copying •</h2>
    <h3></h3>
</div>

 - Inspiration: illogical-impulse of [end-4](https://github.com/end-4), and through it Material Design 3
 - Copying: Absolutely, feel free. Just follow the license and it's all good
 - License: GPL-3.0-only, the text is in [LICENSE](LICENSE). The original is under the GPL 3 too; what is taken from where is in [docs/third-party.md](docs/third-party.md)
