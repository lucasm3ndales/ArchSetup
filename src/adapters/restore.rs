use crate::adapters::file_backup;
use crate::application::restore::{self, DockerBefore, RootAction};
use crate::system;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct LinuxRestore {
    home: PathBuf,
    config_home: PathBuf,
    state_dir: PathBuf,
}

struct RootFile {
    path: PathBuf,
    action: RootAction,
}

impl LinuxRestore {
    pub fn new(home: PathBuf, config_home: Option<PathBuf>, state_home: Option<PathBuf>) -> Self {
        let config_home = config_home
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".config"));
        let state_dir = state_home
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".local/state"))
            .join("archlinuxsetup");
        Self {
            home,
            config_home,
            state_dir,
        }
    }

    pub fn backup_base(&self) -> PathBuf {
        self.state_dir.join("backups")
    }

    pub fn oldest_xfce_backup(&self) -> Result<Option<PathBuf>, String> {
        let mut backups = Vec::new();
        for path in directory_entries(&self.backup_base())? {
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("xfce-"))
                && is_real_directory(&path)
            {
                backups.push(path);
            }
        }
        backups.sort();
        Ok(backups.into_iter().next())
    }

    fn oldest_cursor_backup(&self, name: &str) -> Result<Option<PathBuf>, String> {
        let mut backups = Vec::new();
        for path in directory_entries(&self.backup_base())? {
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("xfce-"))
                && is_real_directory(&path)
                && ["original", "created"]
                    .iter()
                    .any(|suffix| path.join(format!("{name}.{suffix}")).is_file())
            {
                backups.push(path);
            }
        }
        backups.sort();
        Ok(backups.into_iter().next())
    }

    fn docklike_backup_names(&self) -> Result<Vec<String>, String> {
        let mut names = BTreeSet::new();
        for backup in directory_entries(&self.backup_base())? {
            if !backup
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("xfce-"))
                || !is_real_directory(&backup)
            {
                continue;
            }
            for file in directory_entries(&backup)? {
                let Some(file_name) = file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                let stem = file_name
                    .strip_suffix(".original")
                    .or_else(|| file_name.strip_suffix(".created"));
                let Some(stem) = stem else { continue };
                let Some(id) = stem
                    .strip_prefix("docklike-")
                    .and_then(|name| name.strip_suffix(".rc"))
                else {
                    continue;
                };
                if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
                    names.insert(stem.to_owned());
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    pub fn installed_packages(&self) -> Result<Vec<String>, String> {
        let manifest = self.manifest();
        require_regular(
            &manifest,
            "The new-package record is missing. Cannot safely determine what to remove.",
        )?;
        if !system::command_exists("pacman") {
            return Err("pacman not found.".to_owned());
        }
        let content = fs::read_to_string(&manifest)
            .map_err(|error| format!("could not read {}: {error}", manifest.display()))?;
        let mut installed = Vec::new();
        for package in restore::parse_packages(&content)? {
            if status(Command::new("pacman").args(["-Qq", &package]))? {
                installed.push(package);
            }
        }
        Ok(installed)
    }

    pub fn manifest(&self) -> PathBuf {
        self.state_dir.join("packages-installed")
    }

    pub fn apply(
        &self,
        xfce_backup: Option<&Path>,
        packages: Option<&[String]>,
    ) -> Result<(), String> {
        if xfce_backup.is_some() {
            self.check_xfce_session()?;
        }
        self.preflight_user_files(xfce_backup)?;
        let docker = self.docker_before()?;
        let root_paths: Vec<_> = restore::ROOT_FILES.iter().map(PathBuf::from).collect();
        for path in &root_paths {
            reject_symlink_or_other(path)?;
        }
        let root_backups: Vec<_> = root_paths
            .iter()
            .map(|path| oldest_root_backup(path))
            .collect::<Result<_, _>>()?;
        let needs_sudo = docker.is_some()
            || packages.is_some_and(|items| !items.is_empty())
            || root_paths.iter().zip(&root_backups).any(|(path, backup)| {
                backup.is_some() || (path.is_file() && self.root_marker(path).is_file())
            });
        if needs_sudo {
            if !system::command_exists("sudo") {
                return Err("sudo not found.".to_owned());
            }
            run(Command::new("sudo").arg("-v"))?;
        }
        let root_files = root_paths
            .into_iter()
            .zip(root_backups)
            .map(|(path, backup)| {
                let target_exists = path.is_file();
                let marker = self.root_marker(&path);
                let marker_exists = marker.is_file();
                let marker_matches = if marker_exists && target_exists {
                    let digest =
                        capture(Command::new("sudo").arg("sha256sum").arg("--").arg(&path))?;
                    let hash = digest
                        .split_whitespace()
                        .next()
                        .ok_or("sha256sum returned no hash")?;
                    let recorded = fs::read_to_string(&marker)
                        .map_err(|error| format!("could not read {}: {error}", marker.display()))?;
                    recorded
                        .lines()
                        .any(|line| line == format!("{hash} {}", path.display()))
                } else {
                    false
                };
                Ok(RootFile {
                    action: restore::root_action(
                        marker_exists,
                        marker_matches,
                        target_exists,
                        backup,
                    ),
                    path,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if let Some(packages) = packages.filter(|items| !items.is_empty()) {
            let mut command = Command::new("pacman");
            command.args(["-R", "--print", "--"]).args(packages);
            if !status(&mut command)? {
                return Err(
                    "Removal would break dependencies. Review the packages before trying again."
                        .to_owned(),
                );
            }
        }

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let restore_backup = self
            .backup_base()
            .join(format!("restore-{stamp}-{}", std::process::id()));
        fs::create_dir_all(&restore_backup)
            .map_err(|error| format!("could not create {}: {error}", restore_backup.display()))?;

        if let Some(xfce_backup) = xfce_backup {
            self.restore_xfce(xfce_backup, &restore_backup)?;
            println!("Xfce restored. The configuration will load in the next session.");
        }
        if self.state_dir.join("browser-before/recorded").is_file() {
            for label in [
                ".config/mimeapps.list",
                ".local/share/applications/mimeapps.list",
            ] {
                self.restore_user_file(
                    &self.home.join(label),
                    &self.state_dir.join("browser-before").join(label),
                    &restore_backup,
                )?;
            }
            println!("Browser associations restored.");
        }
        for file in root_files {
            self.restore_root_file(file, stamp)?;
        }
        if let Some(docker) = docker {
            restore_docker(&docker)?;
            println!("Previous Docker group and service state restored.");
        }
        if let Some(packages) = packages {
            if !packages.is_empty() {
                run(Command::new("sudo")
                    .args(["pacman", "-R", "--noconfirm", "--"])
                    .args(packages))?;
                println!("Tracked packages removed. Untracked dependencies were preserved.");
            }
            let manifest = self.manifest();
            let archived = restore_backup.join("packages-installed");
            file_backup::copy(&manifest, &archived)?;
            File::create(&manifest)
                .map_err(|error| format!("could not clean {}: {error}", manifest.display()))?;
            println!("Active package record archived at {}.", archived.display());
        }
        println!(
            "Complete. Backups preserved at {}.",
            self.backup_base().display()
        );
        Ok(())
    }

    fn preflight_user_files(&self, xfce_backup: Option<&Path>) -> Result<(), String> {
        if let Some(backup) = xfce_backup {
            for channel in restore::XFCE_CHANNELS {
                reject_symlink_or_other(&self.xfce_file(channel))?;
                reject_symlink_or_other(&backup.join(format!("{channel}.xml")))?;
            }
            for (target, source) in [
                (
                    self.config_home.join("autostart/ksuperkey.desktop"),
                    backup.join("ksuperkey.desktop"),
                ),
                (
                    self.config_home.join("autostart/pasystray.desktop"),
                    backup.join("pasystray-autostart.original"),
                ),
                (
                    self.home.join(".local/bin/archlinuxsetup-keylocks"),
                    backup.join("archlinuxsetup-keylocks"),
                ),
            ] {
                reject_symlink_or_other(&target)?;
                reject_symlink_or_other(&source)?;
            }
            if let Some(saved) = self.oldest_cursor_backup("pasystray-autostart")? {
                let target = self.config_home.join("autostart/pasystray.desktop");
                reject_symlink_or_other(&target)?;
                reject_symlink_or_other(&saved.join("pasystray-autostart.original"))?;
                reject_symlink_or_other(&saved.join("pasystray-autostart.created"))?;
            }
            for (target, name) in [
                (self.home.join(".Xresources"), "Xresources"),
                (self.home.join(".icons/default/index.theme"), "cursor-index"),
                (self.config_home.join("gtk-3.0/gtk.css"), "panel-gtk-css"),
                (
                    self.config_home.join("archsetup/panel-user.svg"),
                    "panel-user.svg",
                ),
            ] {
                if let Some(cursor_backup) = self.oldest_cursor_backup(name)? {
                    reject_symlink_or_other(&target)?;
                    reject_symlink_or_other(&cursor_backup.join(format!("{name}.original")))?;
                    reject_symlink_or_other(&cursor_backup.join(format!("{name}.created")))?;
                }
            }
            for name in self.docklike_backup_names()? {
                let target = self.config_home.join("xfce4/panel").join(&name);
                if let Some(saved) = self.oldest_cursor_backup(&name)? {
                    reject_symlink_or_other(&target)?;
                    reject_symlink_or_other(&saved.join(format!("{name}.original")))?;
                    reject_symlink_or_other(&saved.join(format!("{name}.created")))?;
                }
            }
        }
        if self.state_dir.join("browser-before/recorded").is_file() {
            for label in [
                ".config/mimeapps.list",
                ".local/share/applications/mimeapps.list",
            ] {
                reject_symlink_or_other(&self.home.join(label))?;
                reject_symlink_or_other(&self.state_dir.join("browser-before").join(label))?;
            }
        }
        Ok(())
    }

    fn check_xfce_session(&self) -> Result<(), String> {
        if ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Err(
                "Log out of the graphical session and run restore from a TTY to restore Xfce."
                    .to_owned(),
            );
        }
        if system::command_exists("pgrep") {
            let uid = system::effective_uid()?.to_string();
            if status(Command::new("pgrep").args(["-u", &uid, "-x", "xfconfd"]))? {
                return Err("xfconfd is running. End the Xfce session before restoring.".to_owned());
            }
        }
        Ok(())
    }

    fn xfce_file(&self, channel: &str) -> PathBuf {
        self.config_home
            .join("xfce4/xfconf/xfce-perchannel-xml")
            .join(format!("{channel}.xml"))
    }

    fn restore_xfce(&self, backup: &Path, restore_backup: &Path) -> Result<(), String> {
        for channel in restore::XFCE_CHANNELS {
            self.restore_user_file(
                &self.xfce_file(channel),
                &backup.join(format!("{channel}.xml")),
                restore_backup,
            )?;
        }
        let autostart = self.config_home.join("autostart/ksuperkey.desktop");
        let original = backup.join("ksuperkey.desktop");
        if original.is_file() {
            self.restore_user_file(&autostart, &original, restore_backup)?;
        } else if autostart.is_file() {
            let content = fs::read_to_string(&autostart).map_err(|error| error.to_string())?;
            if content
                .lines()
                .any(|line| line == "Name=ArchLinuxSetup Super key")
            {
                self.restore_user_file(&autostart, &original, restore_backup)?;
            }
        }
        if let Some(saved) = self.oldest_cursor_backup("pasystray-autostart")? {
            let target = self.config_home.join("autostart/pasystray.desktop");
            let original = saved.join("pasystray-autostart.original");
            let created = saved.join("pasystray-autostart.created");
            if original.is_file() {
                self.restore_user_file(&target, &original, restore_backup)?;
            } else if created.is_file() && target.is_file() {
                let current = fs::read(&target).map_err(|error| error.to_string())?;
                let expected = fs::read(&created).map_err(|error| error.to_string())?;
                if current == expected {
                    self.restore_user_file(&target, &original, restore_backup)?;
                } else {
                    println!("Pasystray autostart changed after installation; file preserved.");
                }
            }
        }
        let keylocks = backup.join("archlinuxsetup-keylocks");
        if keylocks.is_file() {
            self.restore_user_file(
                &self.home.join(".local/bin/archlinuxsetup-keylocks"),
                &keylocks,
                restore_backup,
            )?;
        }
        for (target, name) in [
            (self.home.join(".Xresources"), "Xresources"),
            (self.home.join(".icons/default/index.theme"), "cursor-index"),
            (self.config_home.join("gtk-3.0/gtk.css"), "panel-gtk-css"),
            (
                self.config_home.join("archsetup/panel-user.svg"),
                "panel-user.svg",
            ),
        ] {
            if let Some(cursor_backup) = self.oldest_cursor_backup(name)? {
                let original = cursor_backup.join(format!("{name}.original"));
                let created = cursor_backup.join(format!("{name}.created"));
                if original.is_file() {
                    self.restore_user_file(&target, &original, restore_backup)?;
                } else if created.is_file() && target.is_file() {
                    let current = fs::read(&target).map_err(|error| error.to_string())?;
                    let expected = fs::read(&created).map_err(|error| error.to_string())?;
                    if current == expected {
                        self.restore_user_file(&target, &original, restore_backup)?;
                    } else {
                        println!(
                            "Cursor: {} changed after installation; file preserved.",
                            target.display()
                        );
                    }
                }
            }
        }
        for name in self.docklike_backup_names()? {
            let target = self.config_home.join("xfce4/panel").join(&name);
            if let Some(saved) = self.oldest_cursor_backup(&name)? {
                let original = saved.join(format!("{name}.original"));
                let created = saved.join(format!("{name}.created"));
                if original.is_file() {
                    self.restore_user_file(&target, &original, restore_backup)?;
                } else if created.is_file() && target.is_file() {
                    let current = fs::read(&target).map_err(|error| error.to_string())?;
                    let expected = fs::read(&created).map_err(|error| error.to_string())?;
                    if current == expected {
                        self.restore_user_file(&target, &original, restore_backup)?;
                    } else {
                        println!(
                            "Docklike: {} changed after installation; file preserved.",
                            target.display()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn restore_user_file(
        &self,
        target: &Path,
        original: &Path,
        restore_backup: &Path,
    ) -> Result<(), String> {
        reject_symlink_or_other(target)?;
        if target.is_file() {
            let relative = target
                .strip_prefix(&self.home)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| {
                    PathBuf::from("external").join(target.strip_prefix("/").unwrap_or(target))
                });
            let archived = restore_backup.join(relative);
            fs::create_dir_all(archived.parent().ok_or("invalid backup path")?)
                .map_err(|error| format!("could not create backup directory: {error}"))?;
            file_backup::copy(target, &archived)?;
        }
        if original.is_file() {
            fs::create_dir_all(target.parent().ok_or("invalid destination path")?)
                .map_err(|error| format!("could not create configuration directory: {error}"))?;
            file_backup::copy(original, target)?;
        } else if target.is_file() {
            fs::remove_file(target)
                .map_err(|error| format!("could not remove {}: {error}", target.display()))?;
        }
        Ok(())
    }

    fn root_marker(&self, path: &Path) -> PathBuf {
        self.state_dir
            .join("root-created")
            .join(path.file_name().expect("root file has a name"))
    }

    fn restore_root_file(&self, file: RootFile, stamp: u128) -> Result<(), String> {
        let target = &file.path;
        reject_symlink_or_other(target)?;
        match file.action {
            RootAction::RemoveOwned => {
                backup_root_current(target, stamp)?;
                run(Command::new("sudo")
                    .arg("rm")
                    .arg("-f")
                    .arg("--")
                    .arg(target))?;
                println!("Removed file created by ArchSetup: {}", target.display());
            }
            RootAction::Restore(backup) => {
                if target.is_file() {
                    backup_root_current(target, stamp)?;
                }
                run(Command::new("sudo")
                    .arg("cp")
                    .arg("-p")
                    .arg("--")
                    .arg(&backup)
                    .arg(target))?;
                println!("Restored: {}", target.display());
            }
            RootAction::PreserveChanged => println!(
                "Preserved because it changed after ArchSetup: {}",
                target.display()
            ),
            RootAction::PreserveUnknown => println!(
                "Preserved because no previous backup was verified: {}",
                target.display()
            ),
            RootAction::None => {}
        }
        Ok(())
    }

    fn docker_before(&self) -> Result<Option<DockerBefore>, String> {
        let path = self.state_dir.join("docker-before");
        if !path.exists() {
            return Ok(None);
        }
        require_regular(&path, "Invalid previous Docker state record.")?;
        let user = capture(Command::new("id").arg("-un"))?;
        let content = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        restore::parse_docker_before(&content, user.trim()).map(Some)
    }
}

fn restore_docker(before: &DockerBefore) -> Result<(), String> {
    let groups = capture(Command::new("id").arg("-nG"))?;
    let current = groups.split_whitespace().any(|group| group == "docker");
    if !before.in_group && current {
        run(Command::new("sudo").args(["gpasswd", "-d", &before.user, "docker"]))?;
    } else if before.in_group && !current {
        run(Command::new("sudo").args(["usermod", "-aG", "docker", &before.user]))?;
    }
    run(Command::new("sudo").args([
        "systemctl",
        if before.enabled { "enable" } else { "disable" },
        "docker.service",
    ]))?;
    run(Command::new("sudo").args([
        "systemctl",
        if before.active { "start" } else { "stop" },
        "docker.service",
    ]))?;
    Ok(())
}

fn backup_root_current(target: &Path, stamp: u128) -> Result<(), String> {
    let archive = PathBuf::from(format!(
        "{}.archsetup.pre-restore-{stamp}",
        target.display()
    ));
    run(Command::new("sudo")
        .arg("cp")
        .arg("-p")
        .arg("--")
        .arg(target)
        .arg(archive))
}

fn oldest_root_backup(target: &Path) -> Result<Option<PathBuf>, String> {
    let parent = target.parent().ok_or("root file has no parent directory")?;
    let name = target
        .file_name()
        .ok_or("root file has no name")?
        .to_string_lossy();
    let mut paths = Vec::new();
    for path in directory_entries(parent)? {
        let Some(candidate) = path.file_name() else {
            continue;
        };
        let candidate = candidate.to_string_lossy();
        if (candidate.starts_with(&format!("{name}.archsetup.bak-"))
            || candidate.starts_with(&format!("{name}.archlinuxsetup.bak-")))
            && is_regular_file(&path)
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths.into_iter().next())
}

fn directory_entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!("could not list {}: {error}", dir.display()));
        }
    };
    entries
        .map(|entry| {
            entry
                .map(|item| item.path())
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn reject_symlink_or_other(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("Unexpected symbolic link: {}", path.display()))
        }
        Ok(metadata) if !metadata.is_file() => {
            Err(format!("Path is not a regular file: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not verify {}: {error}", path.display())),
    }
}

fn require_regular(path: &Path, message: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        _ => Err(message.to_owned()),
    }
}

fn run(command: &mut Command) -> Result<(), String> {
    io::stdout().flush().map_err(|error| error.to_string())?;
    let status = command
        .status()
        .map_err(|error| format!("could not run {:?}: {error}", command))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "command failed: {:?} (exit code {})",
            command,
            status.code().unwrap_or(1)
        ))
    }
}

fn status(command: &mut Command) -> Result<bool, String> {
    command
        .output()
        .map(|output| output.status.success())
        .map_err(|error| format!("could not run {:?}: {error}", command))
}

fn capture(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("could not run {:?}: {error}", command))?;
    if !output.status.success() {
        return Err(format!("command failed: {:?}", command));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("invalid output from {:?}", command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn chooses_first_real_xfce_backup_and_root_backup() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = std::env::temp_dir().join(format!("archsetup-restore-test-{unique}"));
        let state_home = temp.join("state");
        let restore = LinuxRestore::new(temp.clone(), None, Some(state_home));
        let backups = restore.backup_base();
        fs::create_dir_all(backups.join("xfce-002")).unwrap();
        fs::create_dir_all(backups.join("xfce-001")).unwrap();
        symlink(backups.join("xfce-001"), backups.join("xfce-000")).unwrap();
        assert_eq!(
            restore.oldest_xfce_backup().unwrap(),
            Some(backups.join("xfce-001"))
        );
        fs::write(backups.join("xfce-002/Xresources.created"), "cursor").unwrap();
        assert_eq!(
            restore.oldest_cursor_backup("Xresources").unwrap(),
            Some(backups.join("xfce-002"))
        );

        let root = temp.join("greeter.conf");
        fs::write(temp.join("greeter.conf.archsetup.bak-002"), "second").unwrap();
        fs::write(temp.join("greeter.conf.archlinuxsetup.bak-001"), "first").unwrap();
        symlink(
            temp.join("greeter.conf.archlinuxsetup.bak-001"),
            temp.join("greeter.conf.archsetup.bak-000"),
        )
        .unwrap();
        assert_eq!(
            oldest_root_backup(&root).unwrap(),
            Some(temp.join("greeter.conf.archlinuxsetup.bak-001"))
        );
        fs::remove_dir_all(temp).unwrap();
    }
}
