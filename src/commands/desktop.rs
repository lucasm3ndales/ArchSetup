use super::pkg;
use crate::adapters::desktop::LinuxDesktop;
use crate::adapters::linux::LinuxInspector;
use crate::application::doctor;
use crate::project;
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

#[derive(Debug, Default, Eq, PartialEq)]
struct Options {
    profile: Option<String>,
    dry_run: bool,
}

fn parse_args(args: &[OsString]) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].to_string_lossy().as_ref() {
            "--dry-run" => options.dry_run = true,
            "--profile" => {
                index += 1;
                let profile = args
                    .get(index)
                    .ok_or("Missing name after --profile.")?
                    .to_string_lossy();
                if !project::valid_profile_name(&profile) {
                    return Err("Unknown or invalid profile".to_owned());
                }
                options.profile = Some(profile.into_owned());
            }
            "-h" | "--help" => return Ok(None),
            _ => {
                return Err("desktop accepts only --profile NAME and --dry-run".to_owned());
            }
        }
        index += 1;
    }
    Ok(Some(options))
}

pub fn help() -> i32 {
    println!("Usage: archsetup desktop [--profile NAME] [--dry-run]\n");
    println!("Configures Xfce and the X11 touchpad; adjusts Ly if enabled.");
    println!("Existing settings are skipped; sudo is used only for missing system changes.");
    println!("Run as a regular user in an Xfce X11 session.");
    println!("The optional profile applies only compatible hardware settings.");
    0
}

pub fn run(root: &Path, args: &[OsString]) -> i32 {
    let options = match parse_args(args) {
        Ok(Some(options)) => options,
        Ok(None) => return help(),
        Err(error) => {
            eprintln!("Error: {error}");
            return 2;
        }
    };
    let profile_data = if let Some(profile) = &options.profile {
        if !root.join("profiles").join(profile).is_dir() {
            eprintln!("Error: Unknown or invalid profile: {profile}");
            return 1;
        }
        match doctor::read_profile(&LinuxInspector, root, profile) {
            Ok(data) => Some(data),
            Err(error) => {
                eprintln!("Error: {error}");
                return 1;
            }
        }
    } else {
        None
    };
    let linux = match LinuxDesktop::new(root) {
        Ok(linux) => linux,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    if let Err(error) = linux.check_xfce_environment() {
        eprintln!("Error: {error}");
        return 1;
    }
    println!(
        "==> {} desktop",
        if options.dry_run {
            "Preview"
        } else {
            "Configuration"
        }
    );
    println!("Profile: {}", options.profile.as_deref().unwrap_or("none"));
    println!(
        "  Xfce: Nordic, Zafiro Dark icons, Hackneyed cursor, dark 40px panel, Arch-branded Whisker Menu toggled by Super, icon-only tasks (Docklike hover previews when installed), native workspaces, native expandable tray for Wi-Fi and Bluetooth, PulseAudio control beside the tray, crisp white user icon, larger menus and clock, and volume/media keys."
    );
    if linux.uses_ly() {
        println!("  Login: DOOM fire animation and date/time in Ly; Xfce handles session locking.");
    } else {
        println!("  Login: current display manager preserved; no greeter changes.");
    }
    println!("  X11 touchpad: natural scrolling.");
    if profile_data
        .as_ref()
        .is_some_and(|data| data.touchpad_tapping)
    {
        println!("  X11 touchpad: tap-to-click enabled by this profile.");
    }
    if options.dry_run {
        match linux.missing_packages() {
            Some(missing) if !missing.is_empty() => {
                println!(
                    "Missing packages: {}\nDependency: archsetup pkg desktop theme.",
                    missing.join(" ")
                );
            }
            Some(_) => {
                println!("Required packages from desktop and theme are already installed.")
            }
            None => println!("Required packages not checked (pacman unavailable)."),
        }
        println!("Preview complete. No changes made.");
        return 0;
    }
    if let Err(error) = linux.preflight(options.profile.as_deref()) {
        eprintln!("Error: {error}");
        return 1;
    }
    if let Some(missing) = linux.missing_packages() {
        if !missing.is_empty() {
            println!("Missing required packages: {}", missing.join(" "));
            println!("Dependency: archsetup pkg desktop theme.");
            if !io::stdin().is_terminal() {
                eprintln!(
                    "Error: Run archsetup pkg desktop theme in an interactive terminal and try again."
                );
                return 1;
            }
            print!("Run archsetup pkg desktop theme now? [y/N] ");
            if let Err(error) = io::stdout().flush() {
                eprintln!("Error: {error}");
                return 1;
            }
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer).is_err() {
                eprintln!("Error: Could not read the response.");
                return 1;
            }
            if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
                eprintln!(
                    "Error: dependency was not installed. Install the packages and try again."
                );
                return 1;
            }
            let status = pkg::run(root, &["desktop".into(), "theme".into()]);
            if status != 0 {
                eprintln!("Error: desktop and theme installation failed (exit code {status}).");
                return status;
            }
        }
    }
    if let Err(error) = linux.preflight_assets() {
        eprintln!("Error: {error}");
        return 1;
    }
    if let Err(error) = linux.apply(options.profile.as_deref()) {
        eprintln!("Error: {error}");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_desktop_options() {
        assert_eq!(parse_args(&[]), Ok(Some(Options::default())));
        let options = parse_args(&["--profile".into(), "acer".into(), "--dry-run".into()])
            .unwrap()
            .unwrap();
        assert_eq!(options.profile.as_deref(), Some("acer"));
        assert!(options.dry_run);
        assert!(parse_args(&["--xfce-only".into()]).is_err());
        assert!(parse_args(&["--profile".into()]).is_err());
        assert!(parse_args(&["theme".into()]).is_err());
    }
}
