//! Desktop-entry based icon and name resolution - the same mechanism app
//! launchers use. Parses .desktop files once (Name/Icon/Exec/
//! StartupWMClass), matches streams against them, and resolves icon names
//! to actual files across the freedesktop icon dirs (user, system,
//! Flatpak exports). Results are cached per identity.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
struct DesktopEntry {
    /// Desktop-file id: the file stem, lowercased (e.g. "org.kde.dolphin",
    /// "spotify"). What systemd scopes and flatpak ids point at.
    id: String,
    /// Display name, e.g. "Spotify".
    name: String,
    name_lower: String,
    icon: Option<String>,
    /// Basename of the Exec command, lowercased.
    exec_base: Option<String>,
    /// Steam app id extracted from a `steam://run(gameid)/...` launcher.
    /// This distinguishes games whose desktop entries all execute `steam`.
    steam_app_id: Option<String>,
    wm_class_lower: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolved {
    /// Absolute path to an icon file, ready for the asset protocol.
    pub icon_path: Option<String>,
    /// Polished display name from the desktop entry, when matched.
    pub display_name: Option<String>,
    /// Stable desktop-file id (without `.desktop`) used to group the raw
    /// PipeWire identities that belong to one installed application.
    pub desktop_id: Option<String>,
}

struct Resolver {
    desktops: Vec<DesktopEntry>,
    cache: HashMap<String, Resolved>,
}

static RESOLVER: OnceLock<Mutex<Resolver>> = OnceLock::new();

fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::data_dir() {
        dirs.push(home.join("applications"));
        dirs.push(home.join("flatpak/exports/share/applications"));
    }
    dirs.push(PathBuf::from("/usr/share/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs
}

/// Every installed icon theme directory (hicolor first, then whatever
/// themes the distro/user installed - Papirus, Adwaita, breeze, …).
/// Many apps only ship icons inside a theme, so hicolor alone misses them.
fn icon_theme_dirs() -> &'static [PathBuf] {
    // The theme set is stable for the process lifetime; scanning the icon
    // roots once avoids re-walking them for every resolve cache miss
    // (icon_name_to_path tries several candidate names per stream).
    static THEMES: OnceLock<Vec<PathBuf>> = OnceLock::new();
    THEMES.get_or_init(|| {
        let mut roots = Vec::new();
        if let Some(data) = dirs::data_dir() {
            roots.push(data.join("icons"));
            roots.push(data.join("flatpak/exports/share/icons"));
        }
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));

        let mut themes = Vec::new();
        for root in roots {
            // hicolor is the freedesktop fallback theme - search it first.
            let hicolor = root.join("hicolor");
            if hicolor.is_dir() {
                themes.push(hicolor);
            }
            if let Ok(read) = fs::read_dir(&root) {
                for entry in read.flatten() {
                    let path = entry.path();
                    if path.is_dir() && path.file_name().is_some_and(|n| n != "hicolor") {
                        themes.push(path);
                    }
                }
            }
        }
        themes
    })
}

fn parse_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let raw = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut icon, mut exec, mut wm_class, mut no_display) =
        (None::<String>, None, None, None, false);
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "Name" if name.is_none() => name = Some(value.to_string()),
                "Icon" => icon = Some(value.to_string()),
                "Exec" => exec = Some(value.to_string()),
                "StartupWMClass" => wm_class = Some(value.to_string()),
                "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }
    if no_display {
        return None;
    }
    let name = name?;
    let steam_app_id = exec.as_deref().and_then(steam_app_id_from_exec);
    let exec_base = exec.and_then(|e| {
        let first = e.split_whitespace().next()?;
        Path::new(first)
            .file_name()
            .map(|f| f.to_string_lossy().to_lowercase())
    });
    Some(DesktopEntry {
        id: path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default(),
        name_lower: name.to_lowercase(),
        name,
        icon,
        exec_base,
        steam_app_id,
        wm_class_lower: wm_class.map(|w| w.to_lowercase()),
    })
}

fn steam_app_id_from_exec(exec: &str) -> Option<String> {
    exec.split_whitespace().find_map(|argument| {
        let suffix = argument
            .strip_prefix("steam://rungameid/")
            .or_else(|| argument.strip_prefix("steam://run/"))?;
        let app_id: String = suffix.chars().take_while(char::is_ascii_digit).collect();
        (!app_id.is_empty()).then_some(app_id)
    })
}

/// Desktop-id candidates for a live process, most reliable first. Linux
/// binaries don't embed icons - the icon belongs to the app's .desktop
/// entry, so identifying a stream's icon means mapping PID → desktop id
/// through the fingerprints the system leaves on the process.
fn desktop_id_candidates(pid: u32) -> Vec<String> {
    let mut out = Vec::new();

    // 1. systemd app units: desktop launchers run apps in cgroups named
    //    app[-<launcher>]-<DesktopID>-<rand>.scope or
    //    app-<DesktopID>@<uuid>.service (e.g. app-discord@1a2b….service).
    if let Ok(cgroup) = fs::read_to_string(format!("/proc/{pid}/cgroup")) {
        if let Some(unit) = cgroup
            .lines()
            .filter_map(|l| l.rsplit('/').next())
            .find(|seg| {
                seg.starts_with("app-") && (seg.ends_with(".scope") || seg.ends_with(".service"))
            })
        {
            let token = unit
                .trim_start_matches("app-")
                .trim_end_matches(".scope")
                .trim_end_matches(".service");
            // Drop the instance suffix: @uuid, or a trailing -random part.
            let token = match token.split_once('@') {
                Some((before, _)) => before,
                None => match token.rfind('-') {
                    Some(i) if token[i + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => {
                        &token[..i]
                    }
                    _ => token,
                },
            };
            // systemd escapes '-' inside unit names as \x2d.
            let token = token.replace("\\x2d", "-").to_lowercase();
            if !token.is_empty() {
                out.push(token.clone());
                // And without a launcher prefix (app-gnome-spotify-…).
                if let Some((_, rest)) = token.split_once('-') {
                    out.push(rest.to_string());
                }
            }
        }
    }

    // 2. Flatpak sandbox: the app id sits at the sandbox root.
    if let Ok(info) = fs::read_to_string(format!("/proc/{pid}/root/.flatpak-info")) {
        if let Some(name) = info.lines().find_map(|l| l.strip_prefix("name=")) {
            out.push(name.trim().to_lowercase());
        }
    }

    // 3. GIO stamps processes launched from a menu/dock with the exact
    //    .desktop file (inherited by children - which is what we want for
    //    audio helper processes).
    if let Ok(environ) = fs::read(format!("/proc/{pid}/environ")) {
        for var in environ.split(|b| *b == 0) {
            if let Some(value) = var.strip_prefix(b"GIO_LAUNCHED_DESKTOP_FILE=".as_slice()) {
                let path = String::from_utf8_lossy(value);
                if let Some(stem) = Path::new(path.as_ref()).file_stem() {
                    out.push(stem.to_string_lossy().to_lowercase());
                }
            }
        }
    }

    out
}

/// The real executable basename - resolves wrapper scripts and symlinks
/// (an "electron" stream whose exe is /opt/Slack/slack, say).
fn exe_basename(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()?
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
}

/// Steam exports the owning game id into native and Proton game processes.
/// It is stronger than an inherited Steam cgroup/GIO launcher identity and
/// maps directly to the app id embedded in generated game desktop entries.
fn process_steam_app_id(pid: u32) -> Option<String> {
    let environ = fs::read(format!("/proc/{pid}/environ")).ok()?;
    for key in [b"SteamAppId=".as_slice(), b"SteamGameId=".as_slice()] {
        for variable in environ.split(|byte| *byte == 0) {
            let Some(value) = variable.strip_prefix(key) else {
                continue;
            };
            if !value.is_empty() && value.iter().all(u8::is_ascii_digit) {
                return Some(String::from_utf8_lossy(value).into_owned());
            }
        }
    }
    None
}

/// Return a match only when the fingerprint identifies exactly one desktop
/// entry. Launchers such as Steam and Wine are shared by many game shortcuts;
/// choosing the first `Exec=steam` entry would assign an arbitrary game's
/// name and icon to every launcher/helper stream.
fn unique_desktop(
    desktops: &[DesktopEntry],
    predicate: impl Fn(&DesktopEntry) -> bool,
) -> Option<&DesktopEntry> {
    let mut matches = desktops.iter().filter(|desktop| predicate(desktop));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn desktop_for_identity<'a>(
    desktops: &'a [DesktopEntry],
    app_lower: &str,
    binary_lower: Option<&str>,
) -> Option<&'a DesktopEntry> {
    unique_desktop(desktops, |desktop| {
        desktop.wm_class_lower.as_deref() == Some(app_lower)
    })
    .or_else(|| unique_desktop(desktops, |desktop| desktop.name_lower == app_lower))
    .or_else(|| {
        let binary = binary_lower?;
        unique_desktop(desktops, |desktop| {
            desktop.exec_base.as_deref() == Some(binary)
        })
    })
    .or_else(|| {
        unique_desktop(desktops, |desktop| {
            desktop.exec_base.as_deref() == Some(app_lower)
        })
    })
}

fn load_desktops() -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();
    for dir in desktop_dirs() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for file in read.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "desktop") {
                if let Some(entry) = parse_desktop_file(&path) {
                    // XDG roots are ordered from the user's overrides to
                    // system fallbacks. The first desktop-file ID shadows
                    // later copies and must count only once for uniqueness.
                    if seen_ids.insert(entry.id.clone()) {
                        entries.push(entry);
                    }
                }
            }
        }
    }
    entries
}

/// Resolve an icon name to a file path across the freedesktop dirs.
fn icon_name_to_path(name: &str) -> Option<String> {
    if name.starts_with('/') && Path::new(name).exists() {
        return Some(name.to_string());
    }
    const SIZES: [&str; 9] = [
        "64x64", "128x128", "256x256", "96x96", "72x72", "48x48", "512x512", "32x32", "24x24",
    ];
    for theme in icon_theme_dirs() {
        for size in SIZES {
            let p = theme.join(size).join("apps").join(format!("{name}.png"));
            if p.exists() {
                return Some(p.to_string_lossy().into_owned());
            }
            // Some themes nest the size the other way around (apps/<size>).
            let p = theme.join("apps").join(size).join(format!("{name}.svg"));
            if p.exists() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
        let svg = theme.join("scalable/apps").join(format!("{name}.svg"));
        if svg.exists() {
            return Some(svg.to_string_lossy().into_owned());
        }
    }
    for ext in ["png", "svg", "xpm"] {
        let p = PathBuf::from("/usr/share/pixmaps").join(format!("{name}.{ext}"));
        if p.exists() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    None
}

fn desktop_for_process<'a>(
    desktops: &'a [DesktopEntry],
    steam_app_id: Option<&str>,
    desktop_ids: &[String],
    exe: Option<&str>,
) -> Option<&'a DesktopEntry> {
    if let Some(steam_app_id) = steam_app_id {
        // Do not fall back to an inherited Steam launcher identity when the
        // process explicitly identifies a game. If no matching shortcut is
        // installed, leaving it unresolved is safer than merging all games.
        return unique_desktop(desktops, |desktop| {
            desktop.steam_app_id.as_deref() == Some(steam_app_id)
        });
    }
    desktops
        .iter()
        .find(|desktop| {
            !desktop.id.is_empty() && desktop_ids.iter().any(|candidate| candidate == &desktop.id)
        })
        .or_else(|| {
            let exe = exe?;
            unique_desktop(desktops, |desktop| {
                desktop.exec_base.as_deref() == Some(exe)
            })
        })
}

fn select_canonical_desktop<'a>(
    steam_app_id: Option<&str>,
    pid_desktop: Option<&'a DesktopEntry>,
    hinted_desktop: Option<&'a DesktopEntry>,
    identity_desktop: Option<&'a DesktopEntry>,
) -> (Option<&'a DesktopEntry>, Option<String>) {
    if let Some(steam_app_id) = steam_app_id {
        // The game id is authoritative and stable even when the user has no
        // generated desktop shortcut. A matching shortcut enriches the name
        // and icon only; inherited Steam identities are never a fallback.
        return (pid_desktop, Some(format!("steam-app:{steam_app_id}")));
    }
    let desktop = pid_desktop.or(hinted_desktop).or(identity_desktop);
    let desktop_id = desktop.map(|entry| entry.id.clone());
    (desktop, desktop_id)
}

/// Resolve the best icon path + display name for a stream.
///
/// `binary` is the process binary when the identity came from it;
/// `icon_hint` is the stream's application.icon-name property.
pub fn resolve(
    app_name: &str,
    binary: Option<&str>,
    icon_hint: Option<&str>,
    pid: Option<u32>,
    desktop_id_hint: Option<&str>,
) -> Resolved {
    // Fingerprint before consulting the cache. Different Steam/Proton games
    // can expose the same helper name and icon, so `pid.is_some()` alone is
    // not a safe cache discriminator.
    let pid_desktop_ids = pid.map(desktop_id_candidates).unwrap_or_default();
    let pid_steam_app_id = pid.and_then(process_steam_app_id);
    let pid_exe = pid.and_then(exe_basename);
    let resolver = RESOLVER.get_or_init(|| {
        Mutex::new(Resolver {
            desktops: load_desktops(),
            cache: HashMap::new(),
        })
    });
    let Ok(mut resolver) = resolver.lock() else {
        return Resolved::default();
    };

    // Cache stable process fingerprints rather than the changing PID itself.
    let key = format!(
        "{app_name}\0{binary:?}\0{icon_hint:?}\0{desktop_id_hint:?}\0{pid_steam_app_id:?}\0{pid_desktop_ids:?}\0{pid_exe:?}"
    );
    if let Some(hit) = resolver.cache.get(&key) {
        return hit.clone();
    }

    let app_lower = app_name.to_lowercase();
    let binary_lower = binary.map(str::to_lowercase);
    let hinted_steam_app_id = desktop_id_hint
        .and_then(|hint| hint.strip_prefix("steam-app:"))
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()));
    let canonical_steam_app_id = pid_steam_app_id.as_deref().or(hinted_steam_app_id);

    // The PID beats name-matching: the process's cgroup scope, flatpak id,
    // or launch environment names its desktop entry exactly, and the real
    // exe path sees through wrapper binaries.
    let hinted_desktop = desktop_id_hint.and_then(|hint| {
        let hint = hint.to_lowercase();
        resolver.desktops.iter().find(|desktop| desktop.id == hint)
    });

    let pid_desktop = desktop_for_process(
        &resolver.desktops,
        canonical_steam_app_id,
        &pid_desktop_ids,
        pid_exe.as_deref(),
    );

    let identity_desktop =
        desktop_for_identity(&resolver.desktops, &app_lower, binary_lower.as_deref());
    let (desktop, desktop_id) = select_canonical_desktop(
        canonical_steam_app_id,
        pid_desktop,
        hinted_desktop,
        identity_desktop,
    );

    // A uniquely/canonically matched desktop entry represents the application
    // better than a helper stream's hint (for example steamwebhelper claiming
    // a Chromium icon). Fall back through the raw hint and identity names.
    let slug = app_lower.replace(' ', "-");
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(d) = desktop {
        if let Some(icon) = d.icon.as_deref() {
            candidates.push(icon);
        }
    }
    if let Some(hint) = icon_hint {
        candidates.push(hint);
    }
    if let Some(b) = binary_lower.as_deref() {
        candidates.push(b);
    }
    candidates.push(&slug);

    let resolved = Resolved {
        icon_path: candidates.iter().find_map(|c| icon_name_to_path(c)),
        display_name: desktop.map(|d| d.name.clone()),
        desktop_id,
    };
    // `/proc` metadata can be briefly unavailable while a process starts or
    // exits. Cache successful live identity enrichment, but let live misses
    // retry instead of freezing an incomplete result for the whole session.
    if pid.is_none() || resolved.desktop_id.is_some() {
        resolver.cache.insert(key, resolved.clone());
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_id_candidates_handles_real_processes() {
        // Our own PID won't be in an app unit, but the call must not
        // panic or error on a live /proc entry.
        let _ = desktop_id_candidates(std::process::id());
        // Nonexistent PID degrades to no candidates.
        assert!(desktop_id_candidates(u32::MAX - 7).is_empty());
    }

    #[test]
    fn parses_minimal_desktop_entry() {
        let dir = std::env::temp_dir().join("sonux-test-desktop");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\nName=Cool App\nExec=/usr/bin/coolapp --flag\nIcon=coolapp\nStartupWMClass=CoolApp\n",
        )
        .expect("writes");
        let entry = parse_desktop_file(&path).expect("parses");
        assert_eq!(entry.name, "Cool App");
        assert_eq!(entry.exec_base.as_deref(), Some("coolapp"));
        assert_eq!(entry.steam_app_id, None);
        assert_eq!(entry.wm_class_lower.as_deref(), Some("coolapp"));
        assert_eq!(entry.icon.as_deref(), Some("coolapp"));
    }

    #[test]
    fn nodisplay_entries_are_skipped() {
        let dir = std::env::temp_dir().join("sonux-test-desktop");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("hidden.desktop");
        fs::write(&path, "[Desktop Entry]\nName=Hidden\nNoDisplay=true\n").expect("writes");
        assert!(parse_desktop_file(&path).is_none());
    }

    fn desktop(id: &str, name: &str, exec_base: &str) -> DesktopEntry {
        DesktopEntry {
            id: id.to_string(),
            name: name.to_string(),
            name_lower: name.to_lowercase(),
            icon: None,
            exec_base: Some(exec_base.to_string()),
            steam_app_id: None,
            wm_class_lower: None,
        }
    }

    fn steam_desktop(id: &str, name: &str, app_id: &str) -> DesktopEntry {
        let mut entry = desktop(id, name, "steam");
        entry.steam_app_id = Some(app_id.to_string());
        entry
    }

    #[test]
    fn steam_shortcuts_keep_their_distinct_game_ids() {
        assert_eq!(
            steam_app_id_from_exec("steam steam://rungameid/431960"),
            Some("431960".to_string())
        );
        assert_eq!(
            steam_app_id_from_exec("steam steam://run/916440 --silent"),
            Some("916440".to_string())
        );
        assert_eq!(steam_app_id_from_exec("steam"), None);

        let desktops = vec![
            steam_desktop("wallpaper engine", "Wallpaper Engine", "431960"),
            steam_desktop("anno 1800", "Anno 1800", "916440"),
        ];
        assert_eq!(
            desktop_for_process(&desktops, Some("431960"), &[], None)
                .expect("Wallpaper Engine")
                .name,
            "Wallpaper Engine"
        );
        assert_eq!(
            desktop_for_process(&desktops, Some("916440"), &[], None)
                .expect("Anno 1800")
                .name,
            "Anno 1800"
        );
        assert!(desktop_for_process(&desktops, Some("999999"), &["steam".into()], None).is_none());

        let steam_launcher = desktop("steam", "Steam", "steam");
        let (matched, canonical_id) = select_canonical_desktop(
            Some("999999"),
            None,
            Some(&steam_launcher),
            Some(&steam_launcher),
        );
        assert!(matched.is_none());
        assert_eq!(canonical_id.as_deref(), Some("steam-app:999999"));
    }

    #[test]
    fn exact_name_beats_shared_launcher_executables() {
        let desktops = vec![
            desktop("wallpaper-engine", "Wallpaper Engine", "steam"),
            desktop("steam", "Steam", "steam"),
            desktop("anno-1800", "Anno 1800", "steam"),
        ];
        let resolved = desktop_for_identity(&desktops, "steam", None).expect("Steam entry");
        assert_eq!(resolved.id, "steam");
    }

    #[test]
    fn ambiguous_launcher_executable_never_selects_an_arbitrary_game() {
        let desktops = vec![
            desktop("wallpaper-engine", "Wallpaper Engine", "steam"),
            desktop("anno-1800", "Anno 1800", "steam"),
        ];
        assert!(desktop_for_identity(&desktops, "unknown helper", Some("steam")).is_none());
    }

    #[test]
    fn unique_executable_remains_a_valid_fallback() {
        let desktops = vec![desktop("org.example.player", "Player", "example-player")];
        let resolved = desktop_for_identity(&desktops, "playback", Some("example-player"))
            .expect("unique executable");
        assert_eq!(resolved.id, "org.example.player");
    }
}
