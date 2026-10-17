mod root_files;
mod xfce;

use crate::adapters::linux::LinuxInspector;
use crate::application::{desktop, doctor};
use crate::system;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct LinuxDesktop {
    root: PathBuf,
    home: PathBuf,
    state_dir: PathBuf,
    config_dir: PathBuf,
}

impl LinuxDesktop {
    pub fn new(root: &Path) -> Result<Self, String> {
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or("HOME is not set")?;
        let state_home = env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"));
        let config_dir = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Ok(Self {
            root: root.to_path_buf(),
            home,
            state_dir: state_home.join("archlinuxsetup"),
            config_dir,
        })
    }

    pub fn pacman_available(&self) -> bool {
        system::command_exists("pacman")
    }

    pub fn missing_packages(&self) -> Option<Vec<&'static str>> {
        self.pacman_available().then(|| {
            desktop::REQUIRED_PACKAGES
                .iter()
                .copied()
                .filter(|name| !system::installed(name))
                .collect()
        })
    }

    pub fn check_xfce_environment(&self) -> Result<(), String> {
        let session_type = env::var("XDG_SESSION_TYPE").ok();
        let current_desktop = env::var("XDG_CURRENT_DESKTOP").ok();
        desktop::check_xfce(desktop::XfceFacts {
            session_type: session_type.as_deref(),
            current_desktop: current_desktop.as_deref(),
            display_available: env::var_os("DISPLAY").is_some_and(|value| !value.is_empty()),
            xfce_session_installed: system::installed("xfce4-session"),
            xfce_panel_installed: system::installed("xfce4-panel"),
        })
    }

    pub fn uses_ly(&self) -> bool {
        success("systemctl", &["is-enabled", "--quiet", "ly.service"]).unwrap_or(false)
            || (1..=6).any(|tty| {
                ["ly", "ly-kmsconvt"].iter().any(|name| {
                    let service = format!("{name}@tty{tty}.service");
                    success("systemctl", &["is-enabled", "--quiet", &service]).unwrap_or(false)
                })
            })
    }

    pub fn preflight(&self, profile: Option<&str>) -> Result<(), String> {
        if system::effective_uid()? == 0 {
            return Err("Run as a regular user, without sudo.".to_owned());
        }
        let os_release = fs::read_to_string("/etc/os-release")
            .map_err(|_| "Could not identify the distribution.".to_owned())?;
        if !os_release
            .lines()
            .any(|line| line == "ID=arch" || line == "ID=\"arch\"")
        {
            return Err("Desktop configuration can only run on Arch Linux.".to_owned());
        }
        self.check_xfce_environment()?;
        for name in [
            "sudo",
            "pacman",
            "xfconf-query",
            "xfce4-panel",
            "pgrep",
            "sha256sum",
            "date",
            "mv",
            "cp",
            "cmp",
            "install",
        ] {
            if !system::command_exists(name) {
                return Err(format!("Required command missing: {name}"));
            }
        }
        if let Some(profile) = profile {
            doctor::check_profile(&LinuxInspector, &self.root, profile)
                .map_err(|error| format!("Cannot apply this profile: {error}"))?;
        }
        if !system::installed("xorg-server") {
            return Err(
                "xorg-server is missing; set up an Xfce X11 session before applying changes."
                    .to_owned(),
            );
        }
        if self.uses_ly() {
            if !system::command_exists("ly-dm") {
                return Err("Ly is enabled, but ly-dm was not found.".to_owned());
            }
            if !Path::new("/etc/ly/config.ini").is_file() {
                return Err("Ly is enabled, but /etc/ly/config.ini is missing.".to_owned());
            }
        }
        for path in [
            "/usr/share/xfce4/panel/plugins/whiskermenu.desktop",
            "/usr/share/pixmaps/archlinux-logo.svg",
        ] {
            if !Path::new(path).is_file() {
                return Err(format!("Required file missing: {path}"));
            }
        }
        let alt = maybe_output(
            "xfconf-query",
            &[
                "-c",
                "xfce4-keyboard-shortcuts",
                "-p",
                "/commands/custom/<Alt>F1",
            ],
        );
        if !alt.as_deref().is_none_or(|value| {
            value.is_empty()
                || value == "xfce4-popup-applicationsmenu"
                || value == "xfce4-popup-whiskermenu"
        }) {
            return Err(
                "Alt+F1 already runs another command; review the Xfce shortcut before applying changes."
                    .to_owned(),
            );
        }
        let super_key = maybe_output(
            "xfconf-query",
            &[
                "-c",
                "xfce4-keyboard-shortcuts",
                "-p",
                "/commands/custom/Super_L",
            ],
        );
        if super_key.as_deref().is_some_and(|value| !value.is_empty()) {
            return Err(
                "Super_L already has an Xfce shortcut; remove the conflict before applying changes."
                    .to_owned(),
            );
        }
        let mut root_paths = vec![
            "/etc/X11/xorg.conf.d/90-archlinuxsetup-touchpad.conf",
            "/etc/X11/xorg.conf.d/91-archlinuxsetup-acer-tapping.conf",
        ];
        if self.uses_ly() {
            root_paths.push("/etc/ly/config.ini");
        }
        for path in root_paths {
            let path = Path::new(path);
            match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(format!(
                        "Will not replace a symbolic link: {}",
                        path.display()
                    ));
                }
                Ok(metadata) if !metadata.is_file() => {
                    return Err(format!("Path is not a regular file: {}", path.display()));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("could not inspect {}: {error}", path.display()));
                }
            }
        }
        for directory in [
            self.home.join(".icons"),
            self.home.join(".icons/default"),
            self.config_dir.join("gtk-3.0"),
            self.config_dir.join("archsetup"),
        ] {
            match fs::symlink_metadata(&directory) {
                Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                    return Err(format!("Invalid cursor directory: {}", directory.display()));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "could not inspect {}: {error}",
                        directory.display()
                    ));
                }
            }
        }
        for config_file in [
            self.config_dir.join("autostart/ksuperkey.desktop"),
            self.config_dir.join("autostart/pasystray.desktop"),
            self.home.join(".Xresources"),
            self.home.join(".icons/default/index.theme"),
            self.config_dir.join("gtk-3.0/gtk.css"),
            self.config_dir.join("archsetup/panel-user.svg"),
        ] {
            match fs::symlink_metadata(&config_file) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(format!(
                        "Configuration is not a regular file: {}",
                        config_file.display()
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "could not inspect {}: {error}",
                        config_file.display()
                    ));
                }
            }
            if config_file.is_file()
                && config_file != self.config_dir.join("autostart/ksuperkey.desktop")
            {
                fs::read_to_string(&config_file).map_err(|error| {
                    format!("could not read {}: {error}", config_file.display())
                })?;
            }
        }
        if self.uses_ly() {
            fs::read_to_string("/etc/ly/config.ini")
                .map_err(|_| "Could not read the existing Ly configuration.".to_owned())?;
        }
        self.panel_plan()?;
        Ok(())
    }

    pub fn preflight_assets(&self) -> Result<(), String> {
        if !system::command_exists("ksuperkey") {
            return Err("ksuperkey was not found; run archsetup pkg desktop.".to_owned());
        }
        if let Some(missing) = self.missing_packages() {
            if !missing.is_empty() {
                return Err(format!("Missing required packages: {}", missing.join(" ")));
            }
        }
        for (path, directory) in [
            ("/usr/share/themes/Nordic/gtk-3.0", true),
            ("/usr/share/themes/Nordic/xfwm4", true),
            ("/usr/share/icons/zafiro-dark/index.theme", false),
            ("/usr/share/icons/Hackneyed/cursors", true),
        ] {
            let present = if directory {
                Path::new(path).is_dir()
            } else {
                Path::new(path).is_file()
            };
            if !present {
                return Err(format!(
                    "Theme, icons or cursor missing: {path}. Run archsetup pkg theme."
                ));
            }
        }
        let plan = self.panel_plan()?;
        if !system::command_exists(plan.menu_command) {
            return Err(format!("Menu command missing: {}", plan.menu_command));
        }
        Ok(())
    }

    pub fn apply(&self, profile: Option<&str>) -> Result<(), String> {
        let tapping = profile
            .map(|name| {
                doctor::read_profile(&LinuxInspector, &self.root, name)
                    .map(|data| data.touchpad_tapping)
            })
            .transpose()?
            .unwrap_or(false);
        println!("\n==> Configuring Xfce");
        self.configure_xfce()?;
        let ly_needed = self.uses_ly()
            && fs::read_to_string("/etc/ly/config.ini")
                .map(|current| desktop::render_ly(&current) != current)
                .unwrap_or(true);
        let natural = Path::new("/etc/X11/xorg.conf.d/90-archlinuxsetup-touchpad.conf");
        let natural_needed = fs::read_to_string(natural)
            .map(|current| current != desktop::NATURAL_SCROLL)
            .unwrap_or(true);
        let tapping_path = Path::new("/etc/X11/xorg.conf.d/91-archlinuxsetup-acer-tapping.conf");
        let tapping_needed = tapping
            && fs::read_to_string(tapping_path)
                .map(|current| current != desktop::TAPPING)
                .unwrap_or(true);
        if ly_needed || natural_needed || tapping_needed {
            run("sudo", &["-v"])?;
        }
        if ly_needed {
            println!("\n==> Configuring the Ly login screen");
            self.configure_ly()?;
        } else {
            println!("\n==> Login: already configured or current display manager preserved.");
        }
        if natural_needed || tapping_needed {
            println!("\n==> Configuring the touchpad");
            self.configure_touchpad(tapping)?;
        } else {
            println!("\n==> Touchpad is already configured.");
        }
        println!("\nComplete: desktop configured.");
        Ok(())
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    io::stdout().flush().map_err(|error| error.to_string())?;
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} failed (exit code {}).",
            status.code().unwrap_or(1)
        ))
    }
}

fn capture(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed (exit code {}).",
            output.status.code().unwrap_or(1)
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("invalid output from {program}"))
}

fn maybe_output(program: &str, args: &[&str]) -> Option<String> {
    capture(program, args)
        .ok()
        .map(|value| value.trim_end().to_owned())
}

fn success(program: &str, args: &[&str]) -> Result<bool, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    Ok(output.status.success())
}
