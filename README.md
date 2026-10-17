# ArchSetup

ArchSetup is a post-installation tool for Arch Linux. It installs packages by category, configures an Xfce desktop, and supports hardware-specific settings through profiles.

ArchSetup starts with an already installed system. It does not install Arch Linux or configure disks, encryption, bootloaders, or snapshots.

## Before you start

- Use Arch Linux with internet access and `sudo`.
- Run commands as a regular user. Do not put `sudo` before `archsetup` or `./install.sh`.
- To use `desktop` or `all`, install Xfce and log into an Xfce X11 session first. ArchSetup configures Xfce but does not install it.

## Installation

Install Git, clone the repository, and run the installer:

```bash
sudo pacman -Syu --needed git
git clone https://github.com/lucasm3ndales/ArchLinuxSetup.git
cd ArchLinuxSetup
./install.sh
```

The installer prepares only missing prerequisites, builds the tool, and makes the `archsetup` command available. Repeating it skips the package update when prerequisites are installed. Open a new terminal and check the installation:

```bash
archsetup doctor
```

Keep the repository in place: the installed command points to its build directory.

## First use

Check the system and preview a complete setup:

```bash
archsetup doctor
archsetup all --dry-run
```

To install the default package categories and configure Xfce, run:

```bash
archsetup all
```

`all` runs `aur`, `pkg`, and `desktop`, in that order. An Xfce X11 session is required before it starts. To install selected categories instead, use `pkg`:

```bash
archsetup pkg base development --dry-run
archsetup pkg base development
archsetup pkg gaming
```

You can run categories separately or repeat a command. Already installed packages are skipped. `pkg` updates the system only when it needs to install missing official packages; use `pacman -Syu` separately for routine updates.

## Commands

| Command | Purpose |
| --- | --- |
| `archsetup doctor` | Check the system and report warnings without changing it. |
| `archsetup profiles` | List available profiles. |
| `archsetup aur` | Install the `paru` AUR helper. |
| `archsetup pkg [CATEGORY...]` | Install official and AUR packages from the selected categories. |
| `archsetup desktop` | Configure Xfce, its panel, and the touchpad. |
| `archsetup all` | Run `aur`, `pkg`, and `desktop`. |
| `archsetup restore` | Restore settings from ArchSetup backups. |
| `archsetup link` | Create or update the `archsetup` command; the installer already does this. |
| `archsetup unlink` | Remove the installed command without undoing the settings. |
| `archsetup help [COMMAND]` | Show general or command-specific help. |

If `pkg` needs AUR packages and no helper is available, it offers to run `aur` first. If `desktop` is missing required packages, it can offer to install the `desktop` and `theme` categories.

## Options

| Option | Commands | Effect |
| --- | --- | --- |
| `--dry-run` | `link`, `unlink`, `restore`, `aur`, `pkg`, `desktop`, `all` | Preview changes without modifying the system. |
| `--profile NAME` | `doctor`, `pkg`, `desktop`, `all` | Explicitly select a profile. |
| `--verbose` | `doctor` | Show more detailed diagnostics. |
| `--packages` | `restore` | Also remove packages recorded as newly installed by ArchSetup. |
| `--help` or `-h` | Commands | Show help for a command. |

## Package categories

With no category argument, `archsetup pkg` selects `base development apps theme desktop`. `gaming` and `emulation` must be requested by name. `all` uses the same default categories.

| Category | Purpose |
| --- | --- |
| `base` | General command-line tools. |
| `development` | Development and container tools. |
| `apps` | Everyday applications. |
| `theme` | Fonts, theme, icons, and cursors. |
| `desktop` | Xfce utilities including Docklike window previews, Whisker Menu, the Xfce PulseAudio plugin for volume and media keys, Blueman Bluetooth management, and `ksuperkey`; keyboard LED indicators are no longer installed. |
| `gaming` | Games and related tools. |
| `emulation` | Emulators. |

Each category can include packages from the [official repositories](packages/official/) and the [AUR](packages/aur/). Review those files for the exact package list before installing.

`archsetup pkg desktop` installs desktop utilities; `archsetup desktop` applies the configuration. Neither installs Xfce. Enable `[multilib]` in pacman before installing the `gaming` category.

## Profiles

A profile adds packages or settings for a specific machine. Without `--profile`, ArchSetup uses only shared package lists. The `generic` profile explicitly makes the same choice. Hardware profiles apply only when selected and when the machine matches.

```bash
archsetup profiles
archsetup doctor
archsetup pkg --profile NAME --dry-run
archsetup pkg --profile NAME
```

`doctor` may suggest a compatible profile but never selects one automatically. `--profile NAME` also works with `desktop` and `all`. See the available [profiles](profiles/).

A profile is a TOML file at `profiles/NAME/profile.toml`. It has a required `name` and `description`; hardware profiles also use `match_vendor` and `match_dmi`. Optional `[packages.official]` and `[packages.aur]` tables map category names to package arrays. The optional `[desktop]` table supports `touchpad_tapping = true`. Unknown fields are rejected. See the [Acer profile](profiles/acer-aspire-vx5-591g/profile.toml) for an example.

## Desktop configuration

`archsetup desktop` configures Xfce with Nordic, Zafiro Dark icons, Hackneyed cursors, and a compact 40-pixel dark top panel. Whisker Menu keeps the Arch logo in the filled button at the left edge and lets the Super key open or close the menu, including when it was opened by clicking the button. Open applications appear as icons; when Docklike is installed, hovering over an icon shows its windows with previews, including a single open window. The native tasklist remains the fallback until Docklike is installed. The panel uses compact native workspace previews and preserves Xfce's workspace names and count. The session menu uses a crisp white vector user icon based on Zafiro Dark, while the menus, calendar, and clock use larger text and spacing. Invisible spacing separates workspaces, status icons, session menu, and clock. The old keyboard LED indicator and Verve search plugin are omitted. Blueman provides a Bluetooth manager and tray applet after installation; its package supplies Xfce autostart. The command also sets shortcuts and touchpad behavior. If Ly is enabled, it sets the DOOM fire animation and displays the date and time on the login screen. Xfce's screensaver handles inactivity locks and preserves the running session; Ly is used for new logins. Other display managers are left unchanged. Repeating `desktop` skips matching Xfce settings, avoids a panel restart, and only requests `sudo` if a system file needs to change.

The native Xfce status tray sits between the workspace previews and the user menu. Its built-in arrow expands hidden Wi-Fi, Bluetooth, ChatGPT, and other application indicators along the panel. New indicators are hidden by default, including NetworkManager's changing Wi-Fi label. The Xfce PulseAudio plugin sits immediately beside the tray arrow: panel plugins cannot be placed inside the native status tray. The tray shows apps that provide a status icon, not every running process.

The Xfce PulseAudio plugin supplies the visible volume control and handles the notebook's volume Fn keys and MPRIS media keys through PipeWire's PulseAudio-compatible service. ArchSetup disables the old Pasystray autostart and stops its running icon to prevent duplicate audio controls. It removes only its own older `pactl` and `playerctl` shortcuts to avoid conflicting handlers.

Run it inside an Xfce X11 session:

```bash
archsetup desktop --dry-run
archsetup desktop
```

ArchSetup backs up settings before changing them so they can be restored.

## Restore or unlink

To preview and restore previous settings, leave the graphical Xfce session and use a TTY:

```bash
archsetup restore --dry-run
archsetup restore
```

To include packages, review the preview before removing them:

```bash
archsetup restore --packages --dry-run
archsetup restore --packages
```

`--packages` considers only packages recorded as new by ArchSetup. It does not reverse updates to existing packages or restore the entire system. To remove only the installed command and keep the settings, use `archsetup unlink`.

## Update ArchSetup

From the repository directory:

```bash
git pull
./install.sh
```

The installer updates the system, rebuilds the tool, and keeps the installed command pointed at the current build. Use `archsetup help` or `archsetup help pkg` to check the available syntax.
