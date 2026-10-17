use crate::application::package_state::valid_package_name;
use crate::application::profile;
use std::collections::HashSet;
use std::path::Path;

pub trait PackageSource {
    fn read_manifest(&self, path: &Path) -> Result<Option<String>, String>;
    fn pacman_available(&self) -> bool;
    fn installed(&self, package: &str) -> bool;
}

pub fn missing_packages<'a>(
    packages: &'a [String],
    installed: impl Fn(&str) -> bool,
) -> Vec<&'a str> {
    packages
        .iter()
        .filter(|name| !installed(name))
        .map(String::as_str)
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Category {
    Base,
    Development,
    Apps,
    Theme,
    Desktop,
    Gaming,
    Emulation,
}

impl Category {
    pub const DEFAULT: [Self; 5] = [
        Self::Base,
        Self::Development,
        Self::Apps,
        Self::Theme,
        Self::Desktop,
    ];

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "base" => Self::Base,
            "development" => Self::Development,
            "apps" => Self::Apps,
            "theme" => Self::Theme,
            "desktop" => Self::Desktop,
            "gaming" => Self::Gaming,
            "emulation" => Self::Emulation,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Development => "development",
            Self::Apps => "apps",
            Self::Theme => "theme",
            Self::Desktop => "desktop",
            Self::Gaming => "gaming",
            Self::Emulation => "emulation",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustProvider {
    None,
    Rust,
    Rustup,
}

pub const NVIDIA_580XX_CONFLICTS: &[&str] = &[
    "nvidia",
    "nvidia-dkms",
    "nvidia-lts",
    "nvidia-open",
    "nvidia-open-dkms",
    "nvidia-open-lts",
    "nvidia-utils",
    "lib32-nvidia-utils",
];

#[derive(Debug, Eq, PartialEq)]
pub struct Selection {
    pub categories: Vec<Category>,
    pub official: Vec<String>,
    pub aur: Vec<String>,
    pub rust_provider: RustProvider,
}

impl Selection {
    pub fn includes(&self, category: Category) -> bool {
        self.categories.contains(&category)
    }
}

pub fn select(
    source: &impl PackageSource,
    root: &Path,
    categories: &[Category],
    profile: Option<&str>,
) -> Result<Selection, String> {
    let profile_data = profile
        .map(|name| {
            let path = root.join("profiles").join(name).join("profile.toml");
            let content = source
                .read_manifest(&path)?
                .ok_or_else(|| format!("Profile {name} has no profile.toml"))?;
            profile::parse(name, &content)
        })
        .transpose()?;
    let mut selection = Selection {
        categories: Vec::new(),
        official: Vec::new(),
        aur: Vec::new(),
        rust_provider: RustProvider::None,
    };
    let mut seen_categories = HashSet::new();
    let mut seen_official = HashSet::new();
    let mut seen_aur = HashSet::new();
    for category in categories {
        if !seen_categories.insert(*category) {
            continue;
        }
        selection.categories.push(*category);
        for kind in ["official", "aur"] {
            let shared = root
                .join("packages")
                .join(kind)
                .join(format!("{}.txt", category.name()));
            add_manifest(
                source,
                &shared,
                kind,
                &mut selection,
                &mut seen_official,
                &mut seen_aur,
            )?;
            if let Some(profile_data) = &profile_data {
                add_packages(
                    profile_data.packages(kind, *category),
                    kind,
                    &mut selection,
                    &mut seen_official,
                    &mut seen_aur,
                );
            }
        }
        if source.pacman_available()
            && *category == Category::Gaming
            && selection.aur.iter().any(|name| name == "nvidia-580xx-dkms")
        {
            for (kernel, headers) in [
                ("linux", "linux-headers"),
                ("linux-lts", "linux-lts-headers"),
                ("linux-zen", "linux-zen-headers"),
                ("linux-hardened", "linux-hardened-headers"),
            ] {
                if source.installed(kernel) && seen_official.insert(headers.to_owned()) {
                    selection.official.push(headers.to_owned());
                }
            }
        }
    }
    if let Some(conflict) = selection
        .aur
        .iter()
        .find(|name| seen_official.contains(name.as_str()))
    {
        return Err(format!(
            "Package {conflict} appears in both official and AUR lists. Choose one source."
        ));
    }
    if seen_official.contains("rustup") {
        if source.pacman_available() && source.installed("rustup") {
            selection.rust_provider = RustProvider::Rustup;
        } else if source.pacman_available() && source.installed("rust") {
            selection.rust_provider = RustProvider::Rust;
            selection.official.retain(|name| name != "rustup");
        } else {
            selection.rust_provider = RustProvider::Rustup;
        }
    }
    Ok(selection)
}

fn add_manifest(
    source: &impl PackageSource,
    path: &Path,
    kind: &str,
    selection: &mut Selection,
    official: &mut HashSet<String>,
    aur: &mut HashSet<String>,
) -> Result<(), String> {
    let Some(content) = source.read_manifest(path)? else {
        return Ok(());
    };
    let mut packages = Vec::new();
    for package in content.lines() {
        if package.is_empty() || package.starts_with('#') {
            continue;
        }
        if !valid_package_name(package) {
            return Err(format!("Invalid package in {}: {package}", path.display()));
        }
        packages.push(package.to_owned());
    }
    add_packages(&packages, kind, selection, official, aur);
    Ok(())
}

fn add_packages(
    packages: &[String],
    kind: &str,
    selection: &mut Selection,
    official: &mut HashSet<String>,
    aur: &mut HashSet<String>,
) {
    for package in packages {
        if kind == "official" {
            if official.insert(package.clone()) {
                selection.official.push(package.clone());
            }
        } else if aur.insert(package.clone()) {
            selection.aur.push(package.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_installation_has_no_missing_packages() {
        let selected = vec!["git".to_owned(), "blueman".to_owned()];
        assert!(missing_packages(&selected, |_| true).is_empty());
        assert_eq!(
            missing_packages(&selected, |name| name == "git"),
            vec!["blueman"]
        );
    }
    use std::collections::HashMap;
    use std::path::PathBuf;

    struct FakeSource {
        files: HashMap<PathBuf, String>,
        installed: HashSet<String>,
        pacman: bool,
    }
    impl PackageSource for FakeSource {
        fn read_manifest(&self, path: &Path) -> Result<Option<String>, String> {
            Ok(self.files.get(path).cloned())
        }
        fn pacman_available(&self) -> bool {
            self.pacman
        }
        fn installed(&self, package: &str) -> bool {
            self.installed.contains(package)
        }
    }

    #[test]
    fn defaults_deduplicate_and_keep_profile_packages() {
        let root = Path::new("/repo");
        let files = HashMap::from([
            (
                root.join("packages/official/base.txt"),
                "git\n# comment\ngit\n".to_owned(),
            ),
            (
                root.join("packages/official/apps.txt"),
                "discord\n".to_owned(),
            ),
            (root.join("packages/aur/apps.txt"), "brave-bin\n".to_owned()),
            (
                root.join("profiles/acer/profile.toml"),
                "name = \"acer\"\ndescription = \"Acer\"\nmatch_vendor = \"Acer\"\nmatch_dmi = \"VX5\"\n[packages.official]\nbase = [\"linux-headers\"]\n".to_owned(),
            ),
        ]);
        let source = FakeSource {
            files,
            installed: HashSet::new(),
            pacman: false,
        };
        let result = select(
            &source,
            root,
            &[Category::Base, Category::Apps, Category::Base],
            Some("acer"),
        )
        .unwrap();
        assert_eq!(result.categories, vec![Category::Base, Category::Apps]);
        assert_eq!(result.official, vec!["git", "linux-headers", "discord"]);
        assert_eq!(result.aur, vec!["brave-bin"]);
    }

    #[test]
    fn base_does_not_install_headers_without_dkms() {
        let source = FakeSource {
            files: HashMap::new(),
            installed: HashSet::from(["linux-zen".to_owned(), "linux-lts".to_owned()]),
            pacman: true,
        };
        let result = select(&source, Path::new("/repo"), &[Category::Base], None).unwrap();
        assert!(result.official.is_empty());
    }

    #[test]
    fn nvidia_dkms_selects_headers_even_when_only_gaming_is_requested() {
        let root = Path::new("/repo");
        let source = FakeSource {
            files: HashMap::from([(
                root.join("profiles/acer/profile.toml"),
                "name = \"acer\"\ndescription = \"Acer\"\nmatch_vendor = \"Acer\"\nmatch_dmi = \"VX5\"\n[packages.aur]\ngaming = [\"nvidia-580xx-dkms\"]\n".to_owned(),
            )]),
            installed: HashSet::from(["linux".to_owned(), "linux-zen".to_owned()]),
            pacman: true,
        };
        let result = select(&source, root, &[Category::Gaming], Some("acer")).unwrap();
        assert_eq!(result.official, vec!["linux-headers", "linux-zen-headers"]);
        assert_eq!(result.aur, vec!["nvidia-580xx-dkms"]);
    }

    #[test]
    fn generic_profile_adds_no_packages() {
        let root = Path::new("/repo");
        let source = FakeSource {
            files: HashMap::from([
                (root.join("packages/official/base.txt"), "git\n".to_owned()),
                (
                    root.join("profiles/generic/profile.toml"),
                    "name = \"generic\"\ndescription = \"No extras\"\n".to_owned(),
                ),
            ]),
            installed: HashSet::new(),
            pacman: false,
        };
        let normal = select(&source, root, &[Category::Base], None).unwrap();
        let generic = select(&source, root, &[Category::Base], Some("generic")).unwrap();
        assert_eq!(normal.official, generic.official);
        assert_eq!(normal.aur, generic.aur);
    }

    #[test]
    fn avoids_rust_conflict_and_rejects_invalid_manifests() {
        let root = Path::new("/repo");
        let path = root.join("packages/official/development.txt");
        let mut source = FakeSource {
            files: HashMap::from([(path.clone(), "rustup\ndocker\n".to_owned())]),
            installed: HashSet::from(["rust".to_owned()]),
            pacman: true,
        };
        let result = select(&source, root, &[Category::Development], None).unwrap();
        assert_eq!(result.official, vec!["docker"]);
        assert_eq!(result.rust_provider, RustProvider::Rust);
        source.files.insert(path, "bad package\n".to_owned());
        assert!(select(&source, root, &[Category::Development], None).is_err());
    }

    #[test]
    fn rejects_cross_origin_duplicates() {
        let root = Path::new("/repo");
        let source = FakeSource {
            files: HashMap::from([
                (
                    root.join("packages/official/apps.txt"),
                    "brave-bin\n".to_owned(),
                ),
                (root.join("packages/aur/apps.txt"), "brave-bin\n".to_owned()),
            ]),
            installed: HashSet::new(),
            pacman: false,
        };
        assert!(select(&source, root, &[Category::Apps], None).is_err());
    }

    #[test]
    fn repository_default_uses_current_package_categories() {
        struct RepositorySource;
        impl PackageSource for RepositorySource {
            fn read_manifest(&self, path: &Path) -> Result<Option<String>, String> {
                match std::fs::read_to_string(path) {
                    Ok(content) => Ok(Some(content)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error.to_string()),
                }
            }
            fn pacman_available(&self) -> bool {
                false
            }
            fn installed(&self, _: &str) -> bool {
                false
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let default = select(&RepositorySource, root, &Category::DEFAULT, None).unwrap();
        assert_eq!(default.official.len(), 37);
        assert_eq!(default.aur.len(), 9);
        let development = select(&RepositorySource, root, &[Category::Development], None).unwrap();
        assert_eq!(development.aur, ["visual-studio-code-bin"]);
        let apps = select(&RepositorySource, root, &[Category::Apps], None).unwrap();
        assert_eq!(
            apps.official,
            ["discord", "telegram-desktop", "qbittorrent"]
        );
        assert_eq!(apps.aur, ["brave-bin", "spotify", "chatgpt-desktop"]);
        let theme = select(&RepositorySource, root, &[Category::Theme], None).unwrap();
        assert_eq!(theme.official, ["inter-font", "ttf-fira-code"]);
        assert_eq!(
            theme.aur,
            [
                "nordic-theme",
                "zafiro-icon-theme",
                "xcursor-hackneyed-light",
                "xcursor-hackneyed-dark"
            ]
        );
        let desktop = select(&RepositorySource, root, &[Category::Desktop], None).unwrap();
        assert_eq!(
            desktop.official,
            [
                "xorg-xinput",
                "xorg-xauth",
                "xf86-input-libinput",
                "alsa-utils",
                "xdg-desktop-portal-gtk",
                "xfce4-docklike-plugin",
                "xfce4-whiskermenu-plugin",
                "xfce4-pulseaudio-plugin",
                "blueman"
            ]
        );
        assert_eq!(desktop.aur, ["ksuperkey"]);
    }
}
