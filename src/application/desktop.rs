use std::collections::HashMap;

pub const REQUIRED_PACKAGES: &[&str] = &[
    "xf86-input-libinput",
    "inter-font",
    "ttf-fira-code",
    "ksuperkey",
    "xfce4-whiskermenu-plugin",
    "nordic-theme",
    "zafiro-icon-theme",
    "xcursor-hackneyed-light",
    "xcursor-hackneyed-dark",
    "xfce4-pulseaudio-plugin",
];

#[derive(Clone, Copy)]
pub struct XfceFacts<'a> {
    pub session_type: Option<&'a str>,
    pub current_desktop: Option<&'a str>,
    pub display_available: bool,
    pub xfce_session_installed: bool,
    pub xfce_panel_installed: bool,
}

pub fn check_xfce(facts: XfceFacts<'_>) -> Result<(), String> {
    let mut missing = Vec::new();
    if !facts.xfce_session_installed {
        missing.push("xfce4-session");
    }
    if !facts.xfce_panel_installed {
        missing.push("xfce4-panel");
    }
    if !missing.is_empty() {
        return Err(format!(
            "Xfce is not installed (missing {}). Install it before running desktop or all.",
            missing.join(" and ")
        ));
    }
    if facts.session_type != Some("x11") || !facts.display_available {
        return Err("Start an Xfce X11 session before running desktop or all.".to_owned());
    }
    if !facts.current_desktop.is_some_and(|desktop| {
        desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("xfce"))
    }) {
        return Err(
            "The current session is not Xfce. Log into Xfce before running desktop or all."
                .to_owned(),
        );
    }
    Ok(())
}

pub const AUTOSTART: &str = "[Desktop Entry]\nType=Application\nName=ArchLinuxSetup Super key\nComment=Opens the application menu when pressing the Super key\nExec=ksuperkey\nOnlyShowIn=XFCE;\nTerminal=false\n";
pub const PASYSTRAY_DISABLED: &str =
    "[Desktop Entry]\nType=Application\nName=PulseAudio System Tray\nHidden=true\n";
pub const RETIRED_MEDIA_SHORTCUTS: &[(&str, &str)] = &[
    (
        "XF86AudioRaiseVolume",
        "pactl set-sink-volume @DEFAULT_SINK@ +5%",
    ),
    (
        "XF86AudioLowerVolume",
        "pactl set-sink-volume @DEFAULT_SINK@ -5%",
    ),
    ("XF86AudioMute", "pactl set-sink-mute @DEFAULT_SINK@ toggle"),
    (
        "XF86AudioMicMute",
        "pactl set-source-mute @DEFAULT_SOURCE@ toggle",
    ),
    ("XF86AudioPlay", "playerctl play-pause"),
    ("XF86AudioNext", "playerctl next"),
    ("XF86AudioPrev", "playerctl previous"),
    ("XF86AudioStop", "playerctl stop"),
];
pub const NATURAL_SCROLL: &str = "Section \"InputClass\"\n    Identifier \"ArchLinuxSetup touchpad natural scroll\"\n    MatchIsTouchpad \"on\"\n    MatchDevicePath \"/dev/input/event*\"\n    Driver \"libinput\"\n    Option \"NaturalScrolling\" \"true\"\nEndSection\n";
pub const TAPPING: &str = "Section \"InputClass\"\n    Identifier \"ArchLinuxSetup Acer touchpad tap-to-click\"\n    MatchIsTouchpad \"on\"\n    MatchDevicePath \"/dev/input/event*\"\n    Driver \"libinput\"\n    Option \"Tapping\" \"true\"\nEndSection\n";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Panel {
    pub id: u32,
    pub position: String,
    pub plugins: Vec<u32>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PanelPlan {
    pub top_id: u32,
    pub remove_panels: Vec<u32>,
    pub menu_id: u32,
    pub menu_type: &'static str,
    pub menu_command: &'static str,
    pub windowlist_id: u32,
    pub windowlist_type: &'static str,
    pub spacer_id: u32,
    pub pager_id: u32,
    pub systray_id: u32,
    pub pulseaudio_id: u32,
    pub retired_tray_popup_id: Option<u32>,
    pub pager_gap_id: u32,
    pub status_gap_id: u32,
    pub session_gap_id: u32,
    pub actions_id: u32,
    pub clock_id: u32,
    pub legacy_genmon_id: Option<u32>,
    pub remove_legacy_marker: bool,
    pub remove_kbdleds_marker: bool,
    pub plugin_order: Vec<u32>,
}

pub fn plan_panels(
    panels: &[Panel],
    plugin_types: &HashMap<u32, String>,
    max_plugin_id: u32,
    legacy_keylock: Option<u32>,
    recorded_kbdleds: Option<u32>,
    docklike_available: bool,
    whisker_available: bool,
) -> Result<PanelPlan, String> {
    let top = panels
        .iter()
        .find(|panel| panel.position.starts_with("p=6;"))
        .or_else(|| {
            panels
                .iter()
                .find(|panel| !panel.position.starts_with("p=10;"))
        })
        .or_else(|| panels.first())
        .ok_or("No existing panel was found.")?;
    let remove_panels = panels
        .iter()
        .filter(|panel| panel.id != top.id)
        .map(|panel| panel.id)
        .collect();
    let mut next_id = max_plugin_id;
    let kind = |id: &u32| plugin_types.get(id).map(String::as_str).unwrap_or("");
    let mut allocate = |name: &str| -> Result<u32, String> {
        if let Some(id) = top.plugins.iter().find(|id| kind(id) == name) {
            return Ok(*id);
        }
        next_id = next_id.checked_add(1).ok_or("Xfce plugin limit exceeded")?;
        Ok(next_id)
    };
    let existing_whisker = top
        .plugins
        .iter()
        .find(|id| kind(id) == "whiskermenu")
        .copied();
    let (menu_id, menu_type, menu_command) = if let Some(id) = existing_whisker {
        (id, "whiskermenu", "xfce4-popup-whiskermenu")
    } else if whisker_available {
        let existing_applications = top
            .plugins
            .iter()
            .find(|id| kind(id) == "applicationsmenu")
            .copied();
        let id = if let Some(id) = existing_applications {
            id
        } else {
            allocate("whiskermenu")?
        };
        (id, "whiskermenu", "xfce4-popup-whiskermenu")
    } else {
        (
            allocate("applicationsmenu")?,
            "applicationsmenu",
            "xfce4-popup-applicationsmenu",
        )
    };
    let windowlist_type = if docklike_available {
        "docklike"
    } else {
        "tasklist"
    };
    let windowlist_id = allocate(windowlist_type)?;
    let pager_id = allocate("pager")?;
    let systray_id = allocate("systray")?;
    let pulseaudio_id = allocate("pulseaudio")?;
    let actions_id = allocate("actions")?;
    let clock_id = allocate("clock")?;
    drop(allocate);
    let retired_tray_popup_id = top
        .plugins
        .iter()
        .find(|id| kind(id) == "archsetup-tray-popup")
        .copied();
    let legacy_genmon_id =
        legacy_keylock.filter(|id| top.plugins.contains(id) && kind(id) == "genmon");
    let separators = top
        .plugins
        .iter()
        .filter(|id| kind(id) == "separator")
        .copied()
        .collect::<Vec<_>>();
    let mut claim_separator = |index: usize| -> Result<u32, String> {
        if let Some(id) = separators.get(index) {
            return Ok(*id);
        }
        next_id = next_id.checked_add(1).ok_or("Xfce plugin limit exceeded")?;
        Ok(next_id)
    };
    let spacer_id = claim_separator(0)?;
    let pager_gap_id = claim_separator(1)?;
    let status_gap_id = claim_separator(2)?;
    let session_gap_id = claim_separator(3)?;
    let mut plugin_order = vec![menu_id, windowlist_id];
    let mut status = Vec::new();
    for id in &top.plugins {
        if [
            menu_id,
            windowlist_id,
            spacer_id,
            pager_id,
            systray_id,
            pulseaudio_id,
            pager_gap_id,
            status_gap_id,
            session_gap_id,
            actions_id,
            clock_id,
        ]
        .contains(id)
            || Some(*id) == legacy_genmon_id
            || Some(*id) == retired_tray_popup_id
        {
            continue;
        }
        match kind(id) {
            "applicationsmenu" | "whiskermenu" | "tasklist" | "docklike" | "clock"
            | "separator" | "kbdleds" | "verve" => {}
            "systray" | "statusnotifier" | "pulseaudio" | "notification-plugin" => status.push(*id),
            _ => plugin_order.push(*id),
        }
    }
    plugin_order.extend([spacer_id, pager_id, pager_gap_id]);
    if !status.contains(&systray_id) {
        status.insert(0, systray_id);
    }
    if !status.contains(&pulseaudio_id) {
        status.push(pulseaudio_id);
    }
    plugin_order.extend(status);
    plugin_order.extend([status_gap_id, actions_id, session_gap_id, clock_id]);
    Ok(PanelPlan {
        top_id: top.id,
        remove_panels,
        menu_id,
        menu_type,
        menu_command,
        windowlist_id,
        windowlist_type,
        spacer_id,
        pager_id,
        systray_id,
        pulseaudio_id,
        retired_tray_popup_id,
        pager_gap_id,
        status_gap_id,
        session_gap_id,
        actions_id,
        clock_id,
        legacy_genmon_id,
        remove_legacy_marker: legacy_keylock.is_some(),
        remove_kbdleds_marker: recorded_kbdleds.is_some(),
        plugin_order,
    })
}

const LY_SETTINGS: &[(&str, &str)] = &[("animation", "doom"), ("clock", "%d/%m/%Y %H:%M")];

pub fn render_ly(existing: &str) -> String {
    let mut output = String::new();
    let mut seen = Vec::new();
    for line in existing.lines() {
        let setting = line.trim_start().split_once('=').map(|(key, _)| key.trim());
        if let Some((key, value)) = LY_SETTINGS.iter().find(|(key, _)| Some(*key) == setting) {
            if !seen.contains(key) {
                output.push_str(&format!("{key} = {value}\n"));
                seen.push(*key);
            }
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    for (key, value) in LY_SETTINGS {
        if !seen.contains(key) {
            output.push_str(&format!("{key} = {value}\n"));
        }
    }
    output
}

pub fn render_xresources(existing: &str) -> String {
    let mut output = String::new();
    for line in existing.lines() {
        if line
            .trim_start()
            .split_once(':')
            .is_some_and(|(key, _)| key.trim() == "Xcursor.theme")
        {
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("Xcursor.theme: Hackneyed\n");
    output
}

const PANEL_CSS_START: &str = "/* ArchSetup panel: begin */";
const PANEL_CSS_END: &str = "/* ArchSetup panel: end */";

// Scaled from Zafiro Dark's im-user outline and drawn at the panel's native size.
pub const PANEL_USER_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"32\" viewBox=\"0 0 32 32\"><circle cx=\"16\" cy=\"10\" r=\"5.5\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"2.5\"/><path d=\"M5.5 27c0-5 4.7-8.5 10.5-8.5S26.5 22 26.5 27Z\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"2.5\" stroke-linejoin=\"round\"/></svg>\n";
pub fn render_panel_user_svg(_existing: &str) -> String {
    PANEL_USER_SVG.to_owned()
}

pub fn render_panel_css(existing: &str) -> String {
    let mut output = if let Some(start) = existing.find(PANEL_CSS_START) {
        if let Some(end_offset) = existing[start..].find(PANEL_CSS_END) {
            let end = start + end_offset + PANEL_CSS_END.len();
            format!(
                "{}{}",
                &existing[..start],
                existing[end..].trim_start_matches('\n')
            )
        } else {
            existing.to_owned()
        }
    } else {
        existing.to_owned()
    };
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("/* ArchSetup panel: begin */\n");
    output.push_str("#applicationmenu-button, #whiskermenu-button { min-width: 48px; min-height: 38px; padding: 0; border-radius: 0; background-color: rgba(255, 255, 255, 0.09); }\n");
    output.push_str("#actions-button { min-width: 42px; min-height: 38px; padding: 0; background-image: url(\"../archsetup/panel-user.svg\"); background-repeat: no-repeat; background-position: center; background-size: 30px 30px; }\n");
    output.push_str("menu { font-family: Inter; font-size: 12pt; }\nmenu menuitem { min-height: 30px; padding: 5px 12px; }\nmenu menuitem image { min-width: 22px; min-height: 22px; }\n");
    output.push_str("calendar { font-family: Inter; font-size: 12pt; padding: 10px; }\ncalendar button { min-width: 28px; min-height: 28px; }\n");
    output.push_str("/* ArchSetup panel: end */\n");
    output
}

pub fn render_docklike_rc(existing: &str) -> String {
    const SETTINGS: &[(&str, &str)] = &[
        ("showPreviews", "true"),
        ("noWindowsListIfSingle", "false"),
        ("onlyDisplayVisible", "true"),
        ("indicatorColorFromTheme", "true"),
    ];
    let mut output = String::new();
    let mut in_user = false;
    let mut saw_user = false;
    let mut seen = Vec::new();
    for line in existing.lines() {
        if line.starts_with('[') {
            if in_user {
                for (key, value) in SETTINGS {
                    if !seen.contains(key) {
                        output.push_str(&format!("{key}={value}\n"));
                    }
                }
            }
            in_user = line.trim() == "[user]";
            saw_user |= in_user;
        }
        if in_user {
            if let Some((key, _)) = line.split_once('=') {
                let key = key.trim();
                if let Some((setting, value)) = SETTINGS.iter().find(|(setting, _)| *setting == key)
                {
                    if !seen.contains(setting) {
                        output.push_str(&format!("{setting}={value}\n"));
                        seen.push(*setting);
                    }
                    continue;
                }
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    if !saw_user {
        output.push_str("[user]\n");
    }
    if in_user || !saw_user {
        for (key, value) in SETTINGS {
            if !seen.contains(key) {
                output.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    output
}

pub fn render_cursor_index(existing: &str) -> String {
    let mut output = String::new();
    let mut in_icon_theme = false;
    let mut found = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_icon_theme {
                output.push_str("Inherits=Hackneyed\n");
            }
            in_icon_theme = trimmed == "[Icon Theme]";
            found |= in_icon_theme;
            output.push_str(line);
            output.push('\n');
        } else if in_icon_theme
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "Inherits")
        {
            continue;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if in_icon_theme {
        output.push_str("Inherits=Hackneyed\n");
    }
    if !found {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("[Icon Theme]\nInherits=Hackneyed\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xfce_requires_installation_and_active_x11_session() {
        let ready = XfceFacts {
            session_type: Some("x11"),
            current_desktop: Some("XFCE"),
            display_available: true,
            xfce_session_installed: true,
            xfce_panel_installed: true,
        };
        assert!(check_xfce(ready).is_ok());
        assert!(
            check_xfce(XfceFacts {
                xfce_panel_installed: false,
                ..ready
            })
            .unwrap_err()
            .contains("xfce4-panel")
        );
        assert!(
            check_xfce(XfceFacts {
                current_desktop: Some("KDE"),
                ..ready
            })
            .unwrap_err()
            .contains("is not Xfce")
        );
        assert!(
            check_xfce(XfceFacts {
                session_type: Some("wayland"),
                ..ready
            })
            .unwrap_err()
            .contains("X11")
        );
        assert!(
            check_xfce(XfceFacts {
                display_available: false,
                ..ready
            })
            .is_err()
        );
        assert!(
            check_xfce(XfceFacts {
                current_desktop: Some("NOTXFCE"),
                ..ready
            })
            .is_err()
        );
    }

    #[test]
    fn selects_top_panel_and_reuses_plugins() {
        let panels = vec![
            Panel {
                id: 1,
                position: "p=6;x=0;y=0".into(),
                plugins: vec![1, 2, 3, 4, 5, 6, 7],
            },
            Panel {
                id: 2,
                position: "p=10;x=0;y=0".into(),
                plugins: vec![],
            },
        ];
        let types = HashMap::from([
            (1, "applicationsmenu".into()),
            (2, "verve".into()),
            (3, "tasklist".into()),
            (4, "pager".into()),
            (5, "kbdleds".into()),
            (6, "systray".into()),
            (7, "clock".into()),
        ]);
        let plan = plan_panels(&panels, &types, 7, None, Some(5), false, false).unwrap();
        assert_eq!(plan.top_id, 1);
        assert_eq!(plan.remove_panels, vec![2]);
        assert_eq!(plan.spacer_id, 10);
        assert_eq!(plan.actions_id, 9);
        assert!(plan.remove_kbdleds_marker);
        assert_eq!(plan.plugin_order, vec![1, 3, 10, 4, 11, 6, 8, 12, 9, 13, 7]);
    }

    #[test]
    fn keeps_one_expanding_spacer_without_discarding_other_plugins() {
        let panels = vec![Panel {
            id: 1,
            position: "p=6;x=0;y=0".into(),
            plugins: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        }];
        let types = HashMap::from([
            (1, "applicationsmenu".into()),
            (2, "tasklist".into()),
            (3, "separator".into()),
            (4, "separator".into()),
            (5, "launcher".into()),
            (6, "pager".into()),
            (7, "separator".into()),
            (8, "kbdleds".into()),
            (9, "systray".into()),
            (10, "clock".into()),
        ]);
        let plan = plan_panels(&panels, &types, 10, None, Some(8), false, false).unwrap();
        assert_eq!(plan.spacer_id, 3);
        assert_eq!(plan.actions_id, 12);
        assert_eq!(
            plan.plugin_order,
            vec![1, 2, 5, 3, 6, 4, 9, 11, 7, 12, 13, 10]
        );
        let mut repeat_types = types.clone();
        repeat_types.insert(11, "pulseaudio".into());
        repeat_types.insert(12, "actions".into());
        repeat_types.insert(13, "separator".into());
        let repeat = plan_panels(
            &[Panel {
                plugins: plan.plugin_order.clone(),
                ..panels[0].clone()
            }],
            &repeat_types,
            13,
            None,
            Some(8),
            false,
            false,
        )
        .unwrap();
        assert_eq!(repeat.plugin_order, plan.plugin_order);
    }

    #[test]
    fn retires_only_archsetup_popup_and_places_status_between_pager_and_user() {
        let panels = vec![Panel {
            id: 1,
            position: "p=6;x=0;y=0".into(),
            plugins: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        }];
        let types = HashMap::from([
            (1, "applicationsmenu".into()),
            (2, "docklike".into()),
            (3, "separator".into()),
            (4, "pager".into()),
            (5, "systray".into()),
            (6, "archsetup-tray-popup".into()),
            (7, "pulseaudio".into()),
            (8, "separator".into()),
            (9, "actions".into()),
            (10, "clock".into()),
        ]);
        let plan = plan_panels(&panels, &types, 10, None, None, true, false).unwrap();
        assert_eq!(plan.retired_tray_popup_id, Some(6));
        assert_eq!(plan.pulseaudio_id, 7);
        assert!(!plan.plugin_order.contains(&6));
        assert!(plan.plugin_order.contains(&7));
        let pager = plan.plugin_order.iter().position(|id| *id == 4).unwrap();
        let wifi_bluetooth = plan.plugin_order.iter().position(|id| *id == 5).unwrap();
        let user = plan.plugin_order.iter().position(|id| *id == 9).unwrap();
        assert!(pager < wifi_bluetooth && wifi_bluetooth < user);
    }

    #[test]
    fn uses_docklike_for_hover_previews_when_available() {
        let panels = vec![Panel {
            id: 1,
            position: "p=6;x=0;y=0".into(),
            plugins: vec![1, 2, 3, 4, 5],
        }];
        let types = HashMap::from([
            (1, "applicationsmenu".into()),
            (2, "tasklist".into()),
            (3, "pager".into()),
            (4, "actions".into()),
            (5, "clock".into()),
        ]);
        let plan = plan_panels(&panels, &types, 5, None, None, true, false).unwrap();
        assert_eq!(plan.windowlist_type, "docklike");
        assert_eq!(plan.windowlist_id, 6);
        assert!(!plan.plugin_order.contains(&2));
        assert_eq!(plan.plugin_order, vec![1, 6, 9, 3, 10, 7, 8, 11, 4, 12, 5]);
        assert_eq!(plan.systray_id, 7);
        assert_eq!(plan.pulseaudio_id, 8);
    }

    #[test]
    fn reuses_applications_button_when_switching_to_whisker() {
        let panels = vec![Panel {
            id: 1,
            position: "p=6;x=0;y=0".into(),
            plugins: vec![1, 2, 3],
        }];
        let types = HashMap::from([
            (1, "applicationsmenu".into()),
            (2, "tasklist".into()),
            (3, "clock".into()),
        ]);
        let plan = plan_panels(&panels, &types, 3, None, None, false, true).unwrap();
        assert_eq!(plan.menu_id, 1);
        assert_eq!(plan.menu_type, "whiskermenu");
        assert_eq!(plan.menu_command, "xfce4-popup-whiskermenu");
        assert_eq!(plan.plugin_order.iter().filter(|id| **id == 1).count(), 1);
    }

    #[test]
    fn panel_css_and_docklike_settings_preserve_unrelated_content() {
        let css = render_panel_css("button { color: red; }\n");
        assert!(css.starts_with("button { color: red; }\n"));
        assert!(css.contains("panel-user.svg"));
        assert_eq!(render_panel_css(&css), css);
        let old_css = css.replace(
            "menu { font-family",
            "#systray-6 > box > button { opacity: 0; }\nmenu { font-family",
        );
        assert_eq!(render_panel_css(&old_css), css);
        let rc =
            render_docklike_rc("[user]\npinned=foo;\nshowPreviews=false\n[other]\nkey=value\n");
        assert!(rc.contains("pinned=foo;\n"));
        assert!(rc.contains("showPreviews=true\n"));
        assert!(rc.contains("[other]\nkey=value\n"));
        assert_eq!(render_docklike_rc(&rc), rc);
    }

    #[test]
    fn renders_ly_settings_without_changing_other_options() {
        let original = "# animation = none\nanimation = none\nclock = null\nfont = foo\n";
        let updated = render_ly(original);
        assert!(updated.contains("# animation = none\n"));
        assert!(updated.contains("animation = doom\n"));
        assert!(updated.contains("clock = %d/%m/%Y %H:%M\n"));
        assert!(updated.contains("font = foo\n"));
        assert_eq!(render_ly(&updated), updated);
    }

    #[test]
    fn cursor_files_keep_other_settings_and_remain_stable() {
        let resources = render_xresources("Xft.dpi: 120\nXcursor.theme: Old\n");
        assert_eq!(resources, "Xft.dpi: 120\nXcursor.theme: Hackneyed\n");
        assert_eq!(render_xresources(&resources), resources);

        let index =
            render_cursor_index("[Icon Theme]\nName=Custom\nInherits=Old\n[Other]\nFoo=bar\n");
        assert_eq!(
            index,
            "[Icon Theme]\nName=Custom\nInherits=Hackneyed\n[Other]\nFoo=bar\n"
        );
        assert_eq!(render_cursor_index(&index), index);
        assert_eq!(
            render_cursor_index(""),
            "[Icon Theme]\nInherits=Hackneyed\n"
        );
    }

    #[test]
    fn replaces_only_recorded_legacy_genmon_and_allocates_unique_plugins() {
        let panels = vec![Panel {
            id: 9,
            position: "p=6;x=0;y=0".into(),
            plugins: vec![2, 5, 18],
        }];
        let types = HashMap::from([
            (2, "whiskermenu".into()),
            (5, "genmon".into()),
            (18, "systray".into()),
        ]);
        let plan = plan_panels(&panels, &types, 18, Some(5), None, false, false).unwrap();
        assert_eq!(plan.menu_id, 2);
        assert_eq!(plan.menu_command, "xfce4-popup-whiskermenu");
        assert_eq!(plan.legacy_genmon_id, Some(5));
        assert_eq!(
            plan.plugin_order,
            vec![2, 19, 24, 20, 25, 18, 21, 26, 22, 27, 23]
        );
        assert_eq!(
            plan.plugin_order.len(),
            plan.plugin_order
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
    }
}
