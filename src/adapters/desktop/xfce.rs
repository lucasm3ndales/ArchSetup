use super::{LinuxDesktop, capture, maybe_output, run, success};
use crate::application::desktop::{self, Panel, PanelPlan};
use crate::system;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

impl LinuxDesktop {
    pub(super) fn xfce_is_current(&self) -> Result<bool, String> {
        let plan = self.panel_plan()?;
        if !plan.remove_panels.is_empty()
            || plan.legacy_genmon_id.is_some()
            || plan.retired_tray_popup_id.is_some()
            || plan.remove_legacy_marker
            || plan.remove_kbdleds_marker
        {
            return Ok(false);
        }
        let panel = format!("/panels/panel-{}", plan.top_id);
        let menu = format!("/plugins/plugin-{}", plan.menu_id);
        let windows = format!("/plugins/plugin-{}", plan.windowlist_id);
        let pager = format!("/plugins/plugin-{}", plan.pager_id);
        let systray = format!("/plugins/plugin-{}", plan.systray_id);
        let audio = format!("/plugins/plugin-{}", plan.pulseaudio_id);
        let actions = format!("/plugins/plugin-{}", plan.actions_id);
        let clock = format!("/plugins/plugin-{}", plan.clock_id);
        let workspace_count = maybe_output(
            "xfconf-query",
            &["-c", "xfwm4", "-p", "/general/workspace_count"],
        )
        .and_then(|value| value.parse::<u32>().ok());
        let pager_rows = if workspace_count.is_some_and(|count| count >= 4) {
            "2"
        } else {
            "1"
        };
        let mut checks = vec![
            ("xsettings", "/Net/ThemeName".to_owned(), "Nordic"),
            ("xsettings", "/Net/IconThemeName".to_owned(), "zafiro-dark"),
            ("xsettings", "/Gtk/CursorThemeName".to_owned(), "Hackneyed"),
            ("xsettings", "/Gtk/CursorThemeSize".to_owned(), "24"),
            ("xsettings", "/Gtk/FontName".to_owned(), "Inter 10"),
            ("xfwm4", "/general/theme".to_owned(), "Nordic"),
            (
                "xfwm4",
                "/general/title_font".to_owned(),
                "Inter SemiBold 10",
            ),
            (
                "xfce4-keyboard-shortcuts",
                "/commands/custom/<Alt>F1".to_owned(),
                plan.menu_command,
            ),
            ("xfce4-panel", "/panels/dark-mode".to_owned(), "true"),
            ("xfce4-panel", format!("{panel}/position"), "p=6;x=0;y=0"),
            ("xfce4-panel", format!("{panel}/length"), "100"),
            ("xfce4-panel", format!("{panel}/size"), "40"),
            ("xfce4-panel", format!("{panel}/icon-size"), "28"),
            ("xfce4-panel", format!("{panel}/background-style"), "0"),
            ("xfce4-panel", format!("{panel}/autohide-behavior"), "0"),
            ("xfce4-panel", format!("{panel}/position-locked"), "true"),
            ("xfce4-panel", menu.clone(), plan.menu_type),
            (
                "xfce4-panel",
                format!("{menu}/button-icon"),
                "/usr/share/pixmaps/archlinux-logo.svg",
            ),
            ("xfce4-panel", format!("{menu}/show-button-title"), "false"),
            ("xfce4-panel", windows.clone(), plan.windowlist_type),
            ("xfce4-panel", pager.clone(), "pager"),
            ("xfce4-panel", format!("{pager}/miniature-view"), "true"),
            ("xfce4-panel", format!("{pager}/rows"), pager_rows),
            ("xfce4-panel", systray.clone(), "systray"),
            ("xfce4-panel", format!("{systray}/hide-new-items"), "true"),
            ("xfce4-panel", audio.clone(), "pulseaudio"),
            (
                "xfce4-panel",
                format!("{audio}/enable-keyboard-shortcuts"),
                "true",
            ),
            ("xfce4-panel", format!("{audio}/enable-mpris"), "true"),
            (
                "xfce4-panel",
                format!("{audio}/enable-multimedia-keys"),
                "true",
            ),
            ("xfce4-panel", actions.clone(), "actions"),
            ("xfce4-panel", format!("{actions}/appearance"), "1"),
            ("xfce4-panel", format!("{actions}/button-title"), "3"),
            ("xfce4-panel", format!("{actions}/custom-title"), ""),
            ("xfce4-panel", clock.clone(), "clock"),
            ("xfce4-panel", format!("{clock}/mode"), "2"),
            (
                "xfce4-panel",
                format!("{clock}/digital-format"),
                "%d/%m %H:%M",
            ),
            ("xfce4-panel", format!("{clock}/digital-layout"), "3"),
            (
                "xfce4-panel",
                format!("{clock}/digital-time-format"),
                "%d/%m %H:%M",
            ),
            (
                "xfce4-panel",
                format!("{clock}/digital-time-font"),
                "Inter Medium 11",
            ),
        ];
        if plan.menu_type == "applicationsmenu" {
            checks.push(("xfce4-panel", format!("{menu}/small"), "false"));
        }
        if plan.windowlist_type == "tasklist" {
            checks.extend([
                ("xfce4-panel", format!("{windows}/grouping"), "true"),
                (
                    "xfce4-panel",
                    format!("{windows}/include-all-workspaces"),
                    "false",
                ),
                ("xfce4-panel", format!("{windows}/show-labels"), "false"),
            ]);
        }
        for (channel, property, expected) in checks {
            if maybe_output("xfconf-query", &["-c", channel, "-p", &property]).as_deref()
                != Some(expected)
            {
                return Ok(false);
            }
        }
        for (known, hidden) in [
            ("known-items", "hidden-items"),
            ("known-legacy-items", "hidden-legacy-items"),
        ] {
            let known = read_string_array(&format!("{systray}/{known}"));
            let hidden = read_string_array(&format!("{systray}/{hidden}"));
            if known.iter().any(|name| !hidden.contains(name)) {
                return Ok(false);
            }
        }
        for (key, command) in desktop::RETIRED_MEDIA_SHORTCUTS {
            if maybe_output(
                "xfconf-query",
                &[
                    "-c",
                    "xfce4-keyboard-shortcuts",
                    "-p",
                    &format!("/commands/custom/{key}"),
                ],
            )
            .as_deref()
                == Some(command)
            {
                return Ok(false);
            }
        }
        for (id, expand) in [
            (plan.spacer_id, true),
            (plan.pager_gap_id, false),
            (plan.status_gap_id, false),
            (plan.session_gap_id, false),
        ] {
            let path = format!("/plugins/plugin-{id}");
            for (property, expected) in [
                (path.clone(), "separator"),
                (
                    format!("{path}/expand"),
                    if expand { "true" } else { "false" },
                ),
                (format!("{path}/style"), "0"),
            ] {
                if maybe_output("xfconf-query", &["-c", "xfce4-panel", "-p", &property]).as_deref()
                    != Some(expected)
                {
                    return Ok(false);
                }
            }
        }
        let actual_ids = maybe_output(
            "xfconf-query",
            &["-c", "xfce4-panel", "-p", &format!("{panel}/plugin-ids")],
        )
        .map(|value| parse_ids(&value));
        if actual_ids.as_deref() != Some(plan.plugin_order.as_slice()) {
            return Ok(false);
        }
        for (path, render) in [
            (
                self.home.join(".Xresources"),
                desktop::render_xresources as fn(&str) -> String,
            ),
            (
                self.home.join(".icons/default/index.theme"),
                desktop::render_cursor_index as fn(&str) -> String,
            ),
            (
                self.config_dir.join("archsetup/panel-user.svg"),
                desktop::render_panel_user_svg as fn(&str) -> String,
            ),
        ] {
            let Ok(current) = fs::read_to_string(path) else {
                return Ok(false);
            };
            if render(&current) != current {
                return Ok(false);
            }
        }
        let Ok(css) = fs::read_to_string(self.config_dir.join("gtk-3.0/gtk.css")) else {
            return Ok(false);
        };
        if desktop::render_panel_css(&css) != css {
            return Ok(false);
        }
        if plan.windowlist_type == "docklike" {
            let path = self
                .config_dir
                .join(format!("xfce4/panel/docklike-{}.rc", plan.windowlist_id));
            let Ok(current) = fs::read_to_string(path) else {
                return Ok(false);
            };
            if desktop::render_docklike_rc(&current) != current {
                return Ok(false);
            }
        }
        if fs::read_to_string(self.config_dir.join("autostart/ksuperkey.desktop"))
            .ok()
            .as_deref()
            != Some(desktop::AUTOSTART)
        {
            return Ok(false);
        }
        if fs::read_to_string(self.config_dir.join("autostart/pasystray.desktop"))
            .ok()
            .as_deref()
            != Some(desktop::PASYSTRAY_DISABLED)
        {
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn panel_plan(&self) -> Result<PanelPlan, String> {
        let values =
            capture("xfconf-query", &["-c", "xfce4-panel", "-p", "/panels"]).map_err(|_| {
                "The Xfce panel has no configuration yet. Start a full session first.".to_owned()
            })?;
        let ids = parse_ids(&values);
        if ids.is_empty() {
            return Err("No existing panel was found.".to_owned());
        }
        let mut panels = Vec::new();
        for id in ids {
            let position_path = format!("/panels/panel-{id}/position");
            let plugins_path = format!("/panels/panel-{id}/plugin-ids");
            let position =
                maybe_output("xfconf-query", &["-c", "xfce4-panel", "-p", &position_path])
                    .unwrap_or_default();
            let plugins = maybe_output("xfconf-query", &["-c", "xfce4-panel", "-p", &plugins_path])
                .map(|value| parse_ids(&value))
                .unwrap_or_default();
            panels.push(Panel {
                id,
                position,
                plugins,
            });
        }
        let properties = capture("xfconf-query", &["-c", "xfce4-panel", "-l"])?;
        let mut max_id = 0;
        for line in properties.lines() {
            if let Some(rest) = line.strip_prefix("/plugins/plugin-") {
                let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                if let Ok(id) = rest[..digits].parse::<u32>() {
                    max_id = max_id.max(id);
                }
            }
        }
        let top = panels
            .iter()
            .find(|panel| panel.position.starts_with("p=6;"))
            .or_else(|| {
                panels
                    .iter()
                    .find(|panel| !panel.position.starts_with("p=10;"))
            })
            .or_else(|| panels.first())
            .unwrap();
        max_id = top.plugins.iter().copied().fold(max_id, u32::max);
        let mut plugin_types = HashMap::new();
        for id in &top.plugins {
            let property = format!("/plugins/plugin-{id}");
            let kind = maybe_output("xfconf-query", &["-c", "xfce4-panel", "-p", &property])
                .unwrap_or_default();
            let kind = if kind == "genmon"
                && maybe_output(
                    "xfconf-query",
                    &[
                        "-c",
                        "xfce4-panel",
                        "-p",
                        &format!("/plugins/plugin-{id}/archsetup-tray-popup"),
                    ],
                )
                .as_deref()
                    == Some("true")
            {
                "archsetup-tray-popup".to_owned()
            } else {
                kind
            };
            plugin_types.insert(*id, kind);
        }
        let legacy_path = format!("/panels/panel-{}/archlinuxsetup-keylocks-id", top.id);
        let kbdleds_path = format!("/panels/panel-{}/archlinuxsetup-kbdleds-id", top.id);
        let legacy = read_id(&legacy_path);
        let recorded = read_id(&kbdleds_path);
        let docklike_available =
            Path::new("/usr/share/xfce4/panel/plugins/docklike.desktop").is_file();
        let whisker_available =
            Path::new("/usr/share/xfce4/panel/plugins/whiskermenu.desktop").is_file();
        desktop::plan_panels(
            &panels,
            &plugin_types,
            max_id,
            legacy,
            recorded,
            docklike_available,
            whisker_available,
        )
    }

    pub(super) fn configure_xfce(&self) -> Result<(), String> {
        if self.xfce_is_current()? {
            println!("Xfce is already configured; no panel restart needed.");
            let uid = system::effective_uid()?.to_string();
            if !success("pgrep", &["-u", &uid, "-x", "ksuperkey"])? {
                Command::new("ksuperkey")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|error| format!("could not start ksuperkey: {error}"))?;
            }
            self.stop_pasystray()?;
            return Ok(());
        }
        let plan = self.panel_plan()?;
        println!("\n==> Saving current configuration");
        let backup = self.backup_xfce()?;
        println!("Previous configuration saved to {}", backup.display());
        println!("\n==> Configuring theme and panels");
        let old_keylocks = self.home.join(".local/bin/archlinuxsetup-keylocks");
        if fs::symlink_metadata(&old_keylocks).is_ok() {
            let target = backup.join("archlinuxsetup-keylocks");
            run(
                "mv",
                &[
                    "--",
                    &old_keylocks.to_string_lossy(),
                    &target.to_string_lossy(),
                ],
            )?;
            println!("Old indicator preserved at {}", target.display());
        }
        set_property("xsettings", "/Net/ThemeName", "string", "Nordic")?;
        set_property("xsettings", "/Net/IconThemeName", "string", "zafiro-dark")?;
        set_property("xsettings", "/Gtk/CursorThemeName", "string", "Hackneyed")?;
        set_property("xsettings", "/Gtk/CursorThemeSize", "int", "24")?;
        set_property("xsettings", "/Gtk/FontName", "string", "Inter 10")?;
        set_property("xfwm4", "/general/theme", "string", "Nordic")?;
        self.configure_cursor_files(&backup)?;
        set_property(
            "xfwm4",
            "/general/title_font",
            "string",
            "Inter SemiBold 10",
        )?;
        set_property(
            "xfce4-keyboard-shortcuts",
            "/commands/custom/<Alt>F1",
            "string",
            plan.menu_command,
        )?;
        for (key, command) in desktop::RETIRED_MEDIA_SHORTCUTS {
            let property = format!("/commands/custom/{key}");
            if maybe_output(
                "xfconf-query",
                &["-c", "xfce4-keyboard-shortcuts", "-p", &property],
            )
            .as_deref()
                == Some(command)
            {
                run(
                    "xfconf-query",
                    &["-c", "xfce4-keyboard-shortcuts", "-p", &property, "--reset"],
                )?;
            }
        }

        for id in &plan.remove_panels {
            let path = format!("/panels/panel-{id}");
            run(
                "xfconf-query",
                &["-c", "xfce4-panel", "-p", &path, "--recursive", "--reset"],
            )?;
        }
        if !plan.remove_panels.is_empty() {
            set_array("xfce4-panel", "/panels", "int", &[plan.top_id.to_string()])?;
        }
        let panel = format!("/panels/panel-{}", plan.top_id);
        set_property("xfce4-panel", "/panels/dark-mode", "bool", "true")?;
        set_property(
            "xfce4-panel",
            &format!("{panel}/position"),
            "string",
            "p=6;x=0;y=0",
        )?;
        set_property("xfce4-panel", &format!("{panel}/length"), "uint", "100")?;
        set_property("xfce4-panel", &format!("{panel}/size"), "uint", "40")?;
        set_property("xfce4-panel", &format!("{panel}/icon-size"), "uint", "28")?;
        set_property(
            "xfce4-panel",
            &format!("{panel}/background-style"),
            "uint",
            "0",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{panel}/autohide-behavior"),
            "uint",
            "0",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{panel}/position-locked"),
            "bool",
            "true",
        )?;
        let menu = format!("/plugins/plugin-{}", plan.menu_id);
        set_property("xfce4-panel", &menu, "string", plan.menu_type)?;
        set_property(
            "xfce4-panel",
            &format!("{menu}/button-icon"),
            "string",
            "/usr/share/pixmaps/archlinux-logo.svg",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{menu}/show-button-title"),
            "bool",
            "false",
        )?;
        if plan.menu_type == "applicationsmenu" {
            set_property("xfce4-panel", &format!("{menu}/small"), "bool", "false")?;
        }
        let windowlist = format!("/plugins/plugin-{}", plan.windowlist_id);
        set_property("xfce4-panel", &windowlist, "string", plan.windowlist_type)?;
        if plan.windowlist_type == "tasklist" {
            set_property(
                "xfce4-panel",
                &format!("{windowlist}/grouping"),
                "bool",
                "true",
            )?;
            set_property(
                "xfce4-panel",
                &format!("{windowlist}/include-all-workspaces"),
                "bool",
                "false",
            )?;
            set_property(
                "xfce4-panel",
                &format!("{windowlist}/show-labels"),
                "bool",
                "false",
            )?;
        } else {
            self.configure_docklike_rc(plan.windowlist_id, &backup)?;
        }
        configure_separator(plan.spacer_id, true)?;
        configure_separator(plan.pager_gap_id, false)?;
        configure_separator(plan.status_gap_id, false)?;
        configure_separator(plan.session_gap_id, false)?;
        let pager = format!("/plugins/plugin-{}", plan.pager_id);
        set_property("xfce4-panel", &pager, "string", "pager")?;
        set_property(
            "xfce4-panel",
            &format!("{pager}/miniature-view"),
            "bool",
            "true",
        )?;
        let workspace_count = maybe_output(
            "xfconf-query",
            &["-c", "xfwm4", "-p", "/general/workspace_count"],
        )
        .and_then(|value| value.parse::<u32>().ok());
        let pager_rows = if workspace_count.is_some_and(|count| count >= 4) {
            "2"
        } else {
            "1"
        };
        set_property("xfce4-panel", &format!("{pager}/rows"), "uint", pager_rows)?;
        let systray = format!("/plugins/plugin-{}", plan.systray_id);
        set_property("xfce4-panel", &systray, "string", "systray")?;
        set_property(
            "xfce4-panel",
            &format!("{systray}/hide-new-items"),
            "bool",
            "true",
        )?;
        let audio = format!("/plugins/plugin-{}", plan.pulseaudio_id);
        set_property("xfce4-panel", &audio, "string", "pulseaudio")?;
        for property in [
            "enable-keyboard-shortcuts",
            "enable-mpris",
            "enable-multimedia-keys",
        ] {
            set_property(
                "xfce4-panel",
                &format!("{audio}/{property}"),
                "bool",
                "true",
            )?;
        }
        for (known, hidden) in [
            ("known-items", "hidden-items"),
            ("known-legacy-items", "hidden-legacy-items"),
        ] {
            let known = read_string_array(&format!("{systray}/{known}"));
            set_string_array("xfce4-panel", &format!("{systray}/{hidden}"), &known)?;
        }
        if let Some(id) = plan.legacy_genmon_id {
            let path = format!("/plugins/plugin-{id}");
            run(
                "xfconf-query",
                &["-c", "xfce4-panel", "-p", &path, "--recursive", "--reset"],
            )?;
        }
        if plan.remove_legacy_marker {
            let path = format!("{panel}/archlinuxsetup-keylocks-id");
            run(
                "xfconf-query",
                &["-c", "xfce4-panel", "-p", &path, "--reset"],
            )?;
        }
        if plan.remove_kbdleds_marker {
            let path = format!("{panel}/archlinuxsetup-kbdleds-id");
            run(
                "xfconf-query",
                &["-c", "xfce4-panel", "-p", &path, "--reset"],
            )?;
        }
        let actions = format!("/plugins/plugin-{}", plan.actions_id);
        set_property("xfce4-panel", &actions, "string", "actions")?;
        set_property("xfce4-panel", &format!("{actions}/appearance"), "uint", "1")?;
        set_property(
            "xfce4-panel",
            &format!("{actions}/button-title"),
            "uint",
            "3",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{actions}/custom-title"),
            "string",
            "",
        )?;
        let clock = format!("/plugins/plugin-{}", plan.clock_id);
        set_property("xfce4-panel", &clock, "string", "clock")?;
        set_property("xfce4-panel", &format!("{clock}/mode"), "uint", "2")?;
        set_property(
            "xfce4-panel",
            &format!("{clock}/digital-format"),
            "string",
            "%d/%m %H:%M",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{clock}/digital-layout"),
            "uint",
            "3",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{clock}/digital-time-format"),
            "string",
            "%d/%m %H:%M",
        )?;
        set_property(
            "xfce4-panel",
            &format!("{clock}/digital-time-font"),
            "string",
            "Inter Medium 11",
        )?;
        let ids = plan
            .plugin_order
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>();
        set_array("xfce4-panel", &format!("{panel}/plugin-ids"), "int", &ids)?;
        run("xfce4-panel", &["--restart"])?;
        if let Some(id) = plan.retired_tray_popup_id {
            let property = format!("/plugins/plugin-{id}");
            run(
                "xfconf-query",
                &[
                    "-c",
                    "xfce4-panel",
                    "-p",
                    &property,
                    "--recursive",
                    "--reset",
                ],
            )?;
        }
        println!("\n==> Enabling the Super key in Xfce");
        self.configure_ksuperkey(&backup)?;
        self.disable_pasystray(&backup)?;
        println!(
            "\nComplete: single panel, native workspace previews, current-workspace tasks, session menu and date/time clock configured."
        );
        Ok(())
    }

    fn backup_xfce(&self) -> Result<PathBuf, String> {
        let base = self.state_dir.join("backups");
        fs::create_dir_all(&base)
            .map_err(|error| format!("could not create {}: {error}", base.display()))?;
        let date = capture("date", &["+%Y%m%d-%H%M%S"])?.trim().to_owned();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .subsec_nanos();
        let backup = base.join(format!("xfce-{date}-{}-{nanos:09}", std::process::id()));
        fs::create_dir(&backup)
            .map_err(|error| format!("could not create backup {}: {error}", backup.display()))?;
        for channel in [
            "xsettings",
            "xfwm4",
            "xfce4-panel",
            "xfce4-keyboard-shortcuts",
        ] {
            let listing = capture("xfconf-query", &["-c", channel, "-lv"]).unwrap_or_default();
            fs::write(backup.join(format!("{channel}.txt")), listing)
                .map_err(|error| format!("could not record {channel}: {error}"))?;
            let source = self
                .config_dir
                .join("xfce4/xfconf/xfce-perchannel-xml")
                .join(format!("{channel}.xml"));
            if source.is_file() {
                fs::copy(&source, backup.join(format!("{channel}.xml")))
                    .map_err(|error| format!("could not save {}: {error}", source.display()))?;
            }
        }
        for (source, backup_name, rendered) in [
            (
                self.home.join(".Xresources"),
                "Xresources.original",
                desktop::render_xresources as fn(&str) -> String,
            ),
            (
                self.home.join(".icons/default/index.theme"),
                "cursor-index.original",
                desktop::render_cursor_index as fn(&str) -> String,
            ),
            (
                self.config_dir.join("archsetup/panel-user.svg"),
                "panel-user.svg.original",
                desktop::render_panel_user_svg as fn(&str) -> String,
            ),
            (
                self.config_dir.join("gtk-3.0/gtk.css"),
                "panel-gtk-css.original",
                desktop::render_panel_css as fn(&str) -> String,
            ),
        ] {
            if source.is_file() {
                let current = fs::read_to_string(&source)
                    .map_err(|error| format!("could not read {}: {error}", source.display()))?;
                if rendered(&current) != current {
                    fs::copy(&source, backup.join(backup_name))
                        .map_err(|error| format!("could not save {}: {error}", source.display()))?;
                }
            }
        }
        Ok(backup)
    }

    fn configure_cursor_files(&self, backup: &Path) -> Result<(), String> {
        for (target, rendered, created_name) in [
            (
                self.home.join(".Xresources"),
                desktop::render_xresources as fn(&str) -> String,
                "Xresources.created",
            ),
            (
                self.home.join(".icons/default/index.theme"),
                desktop::render_cursor_index as fn(&str) -> String,
                "cursor-index.created",
            ),
            (
                self.config_dir.join("archsetup/panel-user.svg"),
                desktop::render_panel_user_svg as fn(&str) -> String,
                "panel-user.svg.created",
            ),
            (
                self.config_dir.join("gtk-3.0/gtk.css"),
                desktop::render_panel_css as fn(&str) -> String,
                "panel-gtk-css.created",
            ),
        ] {
            let existed = target.is_file();
            let current = if existed {
                fs::read_to_string(&target)
                    .map_err(|error| format!("could not read {}: {error}", target.display()))?
            } else {
                String::new()
            };
            let updated = rendered(&current);
            if updated != current {
                fs::create_dir_all(target.parent().ok_or("invalid cursor path")?)
                    .map_err(|error| format!("could not create directory: {error}"))?;
                if !existed {
                    fs::write(backup.join(created_name), &updated).map_err(|error| {
                        format!("could not record {}: {error}", target.display())
                    })?;
                }
                fs::write(&target, &updated).map_err(|error| {
                    format!("could not configure {}: {error}", target.display())
                })?;
            }
        }
        let retired_script = self.config_dir.join("archsetup/tray-popup.py");
        if retired_script.is_file() {
            let content = fs::read_to_string(&retired_script)
                .map_err(|error| format!("could not read {}: {error}", retired_script.display()))?;
            if content.contains("Popup for StatusNotifierItem applications in the Xfce panel.")
                && content.contains("archsetup-tray-popup.sock")
            {
                fs::copy(&retired_script, backup.join("tray-popup.py.removed")).map_err(
                    |error| format!("could not save {}: {error}", retired_script.display()),
                )?;
                fs::remove_file(&retired_script).map_err(|error| {
                    format!("could not remove {}: {error}", retired_script.display())
                })?;
            }
        }
        Ok(())
    }

    fn configure_docklike_rc(&self, id: u32, backup: &Path) -> Result<(), String> {
        let target = self
            .config_dir
            .join(format!("xfce4/panel/docklike-{id}.rc"));
        if fs::symlink_metadata(&target)
            .is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
        {
            return Err(format!(
                "Invalid Docklike configuration: {}",
                target.display()
            ));
        }
        let existed = target.is_file();
        let current = if existed {
            fs::read_to_string(&target)
                .map_err(|error| format!("could not read {}: {error}", target.display()))?
        } else {
            String::new()
        };
        let updated = desktop::render_docklike_rc(&current);
        if updated != current {
            fs::create_dir_all(target.parent().ok_or("invalid Docklike path")?)
                .map_err(|error| format!("could not create Docklike directory: {error}"))?;
            let backup_name = if existed {
                format!("docklike-{id}.rc.original")
            } else {
                format!("docklike-{id}.rc.created")
            };
            if existed {
                fs::copy(&target, backup.join(backup_name))
                    .map_err(|error| format!("could not save Docklike configuration: {error}"))?;
            } else {
                fs::write(backup.join(backup_name), &updated)
                    .map_err(|error| format!("could not record Docklike configuration: {error}"))?;
            }
            fs::write(&target, updated)
                .map_err(|error| format!("could not configure Docklike: {error}"))?;
        }
        Ok(())
    }

    fn configure_ksuperkey(&self, backup: &Path) -> Result<(), String> {
        let target = self.config_dir.join("autostart/ksuperkey.desktop");
        if fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!("Unexpected symbolic link: {}", target.display()));
        }
        let current = fs::read_to_string(&target).ok();
        if current.as_deref() != Some(desktop::AUTOSTART) {
            fs::create_dir_all(target.parent().unwrap()).map_err(|error| error.to_string())?;
            if target.is_file() {
                fs::copy(&target, backup.join("ksuperkey.desktop"))
                    .map_err(|error| format!("could not save autostart: {error}"))?;
            }
            fs::write(&target, desktop::AUTOSTART)
                .map_err(|error| format!("could not configure autostart: {error}"))?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o644))
                .map_err(|error| format!("could not set autostart permissions: {error}"))?;
        }
        let uid = system::effective_uid()?.to_string();
        if !success("pgrep", &["-u", &uid, "-x", "ksuperkey"])? {
            Command::new("ksuperkey")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("could not start ksuperkey: {error}"))?;
        }
        Ok(())
    }

    fn disable_pasystray(&self, backup: &Path) -> Result<(), String> {
        let target = self.config_dir.join("autostart/pasystray.desktop");
        if fs::symlink_metadata(&target)
            .is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
        {
            return Err(format!("Unexpected file type: {}", target.display()));
        }
        let current = fs::read_to_string(&target).ok();
        if current.as_deref() != Some(desktop::PASYSTRAY_DISABLED) {
            fs::create_dir_all(target.parent().ok_or("invalid autostart path")?)
                .map_err(|error| format!("could not create autostart directory: {error}"))?;
            if target.is_file() {
                fs::copy(&target, backup.join("pasystray-autostart.original"))
                    .map_err(|error| format!("could not save Pasystray autostart: {error}"))?;
            } else {
                fs::write(
                    backup.join("pasystray-autostart.created"),
                    desktop::PASYSTRAY_DISABLED,
                )
                .map_err(|error| format!("could not record Pasystray autostart: {error}"))?;
            }
            fs::write(&target, desktop::PASYSTRAY_DISABLED)
                .map_err(|error| format!("could not disable Pasystray autostart: {error}"))?;
        }
        self.stop_pasystray()
    }

    fn stop_pasystray(&self) -> Result<(), String> {
        let uid = system::effective_uid()?.to_string();
        if success("pgrep", &["-u", &uid, "-x", "pasystray"])? {
            run("pkill", &["-u", &uid, "-x", "pasystray"])?;
        }
        Ok(())
    }
}

fn parse_ids(output: &str) -> Vec<u32> {
    output
        .lines()
        .filter_map(|line| line.parse::<u32>().ok())
        .collect()
}

fn parse_string_array(output: &str) -> Vec<String> {
    output
        .split_once("\n\n")
        .map(|(_, items)| items.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn read_string_array(property: &str) -> Vec<String> {
    maybe_output("xfconf-query", &["-c", "xfce4-panel", "-p", property])
        .map(|output| parse_string_array(&output))
        .unwrap_or_default()
}

fn set_string_array(channel: &str, property: &str, values: &[String]) -> Result<(), String> {
    let current = maybe_output("xfconf-query", &["-c", channel, "-p", property]);
    if current.as_deref().map(parse_string_array).as_deref() == Some(values) {
        return Ok(());
    }
    if values.is_empty() {
        if current.is_some() {
            return run("xfconf-query", &["-c", channel, "-p", property, "--reset"]);
        }
        return Ok(());
    }
    let mut args = vec![
        "-c".to_owned(),
        channel.to_owned(),
        "-p".to_owned(),
        property.to_owned(),
        "--create".to_owned(),
        "--force-array".to_owned(),
    ];
    for value in values {
        args.extend([
            "--type".to_owned(),
            "string".to_owned(),
            "--set".to_owned(),
            value.clone(),
        ]);
    }
    run(
        "xfconf-query",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

fn read_id(path: &str) -> Option<u32> {
    maybe_output("xfconf-query", &["-c", "xfce4-panel", "-p", path])?
        .parse()
        .ok()
}

fn set_property(channel: &str, property: &str, kind: &str, value: &str) -> Result<(), String> {
    if maybe_output("xfconf-query", &["-c", channel, "-p", property]).as_deref() == Some(value) {
        return Ok(());
    }
    run(
        "xfconf-query",
        &[
            "-c", channel, "-p", property, "--create", "--type", kind, "--set", value,
        ],
    )
}

fn configure_separator(id: u32, expand: bool) -> Result<(), String> {
    let plugin = format!("/plugins/plugin-{id}");
    set_property("xfce4-panel", &plugin, "string", "separator")?;
    set_property(
        "xfce4-panel",
        &format!("{plugin}/expand"),
        "bool",
        if expand { "true" } else { "false" },
    )?;
    set_property("xfce4-panel", &format!("{plugin}/style"), "uint", "0")
}

fn set_array(channel: &str, property: &str, kind: &str, values: &[String]) -> Result<(), String> {
    if maybe_output("xfconf-query", &["-c", channel, "-p", property])
        .map(|value| {
            parse_ids(&value)
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
        })
        .as_deref()
        == Some(values)
    {
        return Ok(());
    }
    let mut args = vec![
        "-c".to_owned(),
        channel.to_owned(),
        "-p".to_owned(),
        property.to_owned(),
        "--create".to_owned(),
        "--force-array".to_owned(),
    ];
    for value in values {
        args.extend([
            "--type".to_owned(),
            kind.to_owned(),
            "--set".to_owned(),
            value.clone(),
        ]);
    }
    let status = Command::new("xfconf-query")
        .args(&args)
        .status()
        .map_err(|error| format!("could not run xfconf-query: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("xfconf-query failed to configure the array".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_only_numeric_panel_ids() {
        assert_eq!(
            parse_ids("Value is an array with 2 items:\n0\n1\n"),
            vec![0, 1]
        );
    }

    #[test]
    fn parses_systray_names_without_the_xfconf_header() {
        assert_eq!(
            parse_string_array(
                "Value is an array with 2 items:\n\nblueman\nnetworkmanager applet\n"
            ),
            vec!["blueman", "networkmanager applet"]
        );
    }
}
