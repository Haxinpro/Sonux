use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

const PACK_VERSION: u32 = 1;
const MAX_PACK_FILES: usize = 32;
const MAX_DIRECTORY_ENTRIES: usize = 128;
const MAX_PACK_BYTES: u64 = 256 * 1024;
const MAX_TRANSLATIONS: usize = 2_000;
const MAX_NAME_CHARACTERS: usize = 80;
const EXAMPLE_FILE_NAME: &str = "custom-example.json";
const EXAMPLE_SOURCE: &[u8] = include_bytes!("../../src/locales/custom-example.json");

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LanguagePackCatalog {
    pub bundled: Vec<LanguagePack>,
    pub custom: Vec<LanguagePack>,
    pub warnings: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LanguagePack {
    pub version: u32,
    pub locale: String,
    pub name: String,
    #[serde(rename = "nativeName")]
    pub native_name: String,
    pub direction: TextDirection,
    #[serde(default)]
    pub translations: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TextDirection {
    Ltr,
    Rtl,
}

pub fn catalog() -> LanguagePackCatalog {
    let (custom, warnings) = language_directory()
        .map(|directory| load_from(&directory))
        .unwrap_or_else(|| {
            (
                Vec::new(),
                vec!["Custom language-pack directory is unavailable.".to_string()],
            )
        });
    LanguagePackCatalog {
        bundled: vec![LanguagePack {
            version: PACK_VERSION,
            locale: "en".to_string(),
            name: "English".to_string(),
            native_name: "English".to_string(),
            direction: TextDirection::Ltr,
            translations: BTreeMap::new(),
        }],
        custom,
        warnings,
        location: location_label(),
    }
}

pub fn open_location() -> Result<(), String> {
    let _write = crate::persistence::begin_config_write().map_err(|error| error.to_string())?;
    let directory = language_directory()
        .ok_or_else(|| "Custom language-pack directory is unavailable.".to_string())?;
    let held = open_language_directory_at(&directory, true)?
        .ok_or_else(|| "Custom language-pack directory is unavailable.".to_string())?;
    seed_example_at(&held)?;
    Command::new("xdg-open")
        .arg(&directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Unable to open {}: {error}", directory.display()))
}

fn load_from(directory: &Path) -> (Vec<LanguagePack>, Vec<String>) {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return (
                Vec::new(),
                vec!["Custom language-pack location must be a real directory.".to_string()],
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), Vec::new())
        }
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("Unable to inspect custom language packs: {error}")],
            )
        }
        Ok(_) => {}
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("Unable to read custom language packs: {error}")],
            )
        }
    };
    let mut paths = Vec::new();
    let mut directory_limited = false;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            directory_limited = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut warnings = Vec::new();
    if directory_limited {
        warnings.push(format!(
            "Only the first {MAX_DIRECTORY_ENTRIES} entries in the custom language-pack directory were scanned."
        ));
    }
    if paths.len() > MAX_PACK_FILES {
        warnings.push(format!(
            "Only the first {MAX_PACK_FILES} custom language-pack files were considered."
        ));
        paths.truncate(MAX_PACK_FILES);
    }
    let mut packs = Vec::new();
    let mut locales = BTreeSet::from(["en".to_string()]);
    for path in paths {
        let file_name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => {
                warnings.push("A language pack with a non-UTF-8 filename was ignored.".to_string());
                continue;
            }
        };
        match read_pack(&path) {
            Ok(pack) => {
                if file_name == EXAMPLE_FILE_NAME && pack.locale.eq_ignore_ascii_case("zz-example")
                {
                    continue;
                }
                if !locales.insert(pack.locale.to_ascii_lowercase()) {
                    warnings.push(format!(
                        "{file_name}: locale '{}' is already provided; this file was ignored.",
                        pack.locale
                    ));
                    continue;
                }
                packs.push(pack);
            }
            Err(error) => warnings.push(format!("{file_name}: {error}")),
        }
    }
    (packs, warnings)
}

fn read_pack(path: &Path) -> Result<LanguagePack, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_PACK_BYTES {
        return Err("Pack must be a regular JSON file no larger than 256 KiB.".to_string());
    }
    let mut source = Vec::new();
    file.take(MAX_PACK_BYTES + 1)
        .read_to_end(&mut source)
        .map_err(|error| error.to_string())?;
    if source.len() as u64 > MAX_PACK_BYTES {
        return Err("Pack must be a regular JSON file no larger than 256 KiB.".to_string());
    }
    let pack: LanguagePack =
        serde_json::from_slice(&source).map_err(|error| format!("Invalid JSON: {error}"))?;
    validate(&pack)?;
    Ok(pack)
}

fn validate(pack: &LanguagePack) -> Result<(), String> {
    if pack.version != PACK_VERSION {
        return Err("Unsupported language-pack version.".to_string());
    }
    if !valid_locale(&pack.locale)
        || pack
            .locale
            .split('-')
            .next()
            .is_some_and(|language| language.eq_ignore_ascii_case("en"))
    {
        return Err(
            "Locale must be a unique BCP-47-style tag and cannot replace English.".to_string(),
        );
    }
    validate_label("Language name", &pack.name)?;
    validate_label("Native language name", &pack.native_name)?;
    if pack.translations.len() > MAX_TRANSLATIONS {
        return Err(format!(
            "Language packs may contain at most {MAX_TRANSLATIONS} translations."
        ));
    }
    Ok(())
}

fn valid_locale(locale: &str) -> bool {
    let mut parts = locale.split('-');
    let Some(language) = parts.next() else {
        return false;
    };
    (2..=3).contains(&language.len())
        && language.bytes().all(|byte| byte.is_ascii_alphabetic())
        && parts.all(|part| {
            (2..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

fn validate_label(field: &str, value: &str) -> Result<(), String> {
    if value.trim() != value
        || value.is_empty()
        || value.chars().count() > MAX_NAME_CHARACTERS
        || value.chars().any(is_disallowed_control)
        || contains_markup(value)
    {
        Err(format!("{field} must be 1-{MAX_NAME_CHARACTERS} characters without markup, surrounding whitespace, or control characters."))
    } else {
        Ok(())
    }
}

fn is_disallowed_control(character: char) -> bool {
    matches!(character as u32, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)
}

fn contains_markup(value: &str) -> bool {
    value
        .as_bytes()
        .windows(2)
        .any(|window| window[0] == b'<' && (window[1] == b'/' || window[1].is_ascii_alphabetic()))
}

fn language_directory() -> Option<PathBuf> {
    dirs::config_dir().map(|root| root.join("sonux").join("locales"))
}

fn location_label() -> String {
    let Some(config_root) = dirs::config_dir() else {
        return "Unavailable".to_string();
    };
    if dirs::home_dir().map(|home| home.join(".config")).as_ref() == Some(&config_root) {
        "~/.config/sonux/locales".to_string()
    } else {
        "$XDG_CONFIG_HOME/sonux/locales".to_string()
    }
}

fn open_language_directory_at(root: &Path, create: bool) -> Result<Option<File>, String> {
    let open = || {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)
    };
    let directory = match open() {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = root
                .parent()
                .ok_or_else(|| "The language-pack directory has no parent.".to_string())?;
            match fs::symlink_metadata(parent) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(
                        "The Sonux configuration location must be a real directory.".to_string()
                    );
                }
                Err(parent_error) if parent_error.kind() == std::io::ErrorKind::NotFound => {
                    fs::DirBuilder::new()
                        .mode(0o700)
                        .create(parent)
                        .map_err(|create_error| {
                            format!("Unable to prepare the settings directory: {create_error}")
                        })?;
                }
                Err(parent_error) => {
                    return Err(format!(
                        "Unable to inspect the settings directory: {parent_error}"
                    ))
                }
                Ok(_) => {}
            }
            match fs::DirBuilder::new().mode(0o700).create(root) {
                Ok(()) => {}
                Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(create_error) => {
                    return Err(format!(
                        "Unable to create the language-pack directory: {create_error}"
                    ))
                }
            }
            open().map_err(|open_error| {
                format!("Unable to open the language-pack directory safely: {open_error}")
            })?
        }
        Err(error) => {
            return Err(format!(
                "Unable to open the language-pack directory safely: {error}"
            ))
        }
    };
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(format!(
            "Unable to secure the language-pack directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Some(directory))
}

fn seed_example_at(directory: &File) -> Result<(), String> {
    let path = format!(
        "/proc/self/fd/{}/{}",
        directory.as_raw_fd(),
        EXAMPLE_FILE_NAME
    );
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    match options.open(&path) {
        Ok(mut file) => file
            .write_all(EXAMPLE_SOURCE)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("Unable to create {EXAMPLE_FILE_NAME}: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(format!("Unable to create {EXAMPLE_FILE_NAME}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sonux-language-packs-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn write_pack(root: &Path, name: &str, locale: &str, translations: serde_json::Value) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join(name),
            serde_json::to_vec(&serde_json::json!({
                "version": 1, "locale": locale, "name": locale, "nativeName": locale,
                "direction": "ltr", "translations": translations
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn loads_partial_unicode_packs_in_filename_order() {
        let root = temporary_root("valid");
        write_pack(
            &root,
            "b.json",
            "de-DE",
            serde_json::json!({"settings.language.title": "Sprache"}),
        );
        write_pack(
            &root,
            "a.json",
            "fi",
            serde_json::json!({"settings.language.title": "Kieli"}),
        );
        let (packs, warnings) = load_from(&root);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            packs
                .iter()
                .map(|pack| pack.locale.as_str())
                .collect::<Vec<_>>(),
            vec!["fi", "de-DE"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_files_warn_without_blocking_valid_packs() {
        let root = temporary_root("invalid");
        write_pack(&root, "a.json", "fi", serde_json::json!({}));
        write_pack(&root, "b.json", "fi", serde_json::json!({}));
        write_pack(&root, "c.json", "en", serde_json::json!({}));
        fs::write(root.join("d.json"), b"not json").unwrap();
        symlink(root.join("a.json"), root.join("e.json")).unwrap();
        let (packs, warnings) = load_from(&root);
        assert_eq!(packs.len(), 1);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validation_enforces_schema_and_bounds() {
        let valid: LanguagePack = serde_json::from_value(serde_json::json!({
            "version": 1,
            "locale": "zz-Example",
            "name": "Example",
            "nativeName": "Example",
            "direction": "ltr",
            "translations": {"settings.language.title": "Language"}
        }))
        .unwrap();
        validate(&valid).unwrap();

        let unknown = serde_json::from_value::<LanguagePack>(serde_json::json!({
            "version": 1, "locale": "fi", "name": "Suomi", "nativeName": "Suomi",
            "direction": "ltr", "translations": {}, "extra": true
        }));
        assert!(unknown.is_err());
        let mut invalid = valid.clone();
        invalid.locale = "bad_locale".to_string();
        assert!(validate(&invalid).is_err());
        invalid = valid.clone();
        invalid.locale = "en-US".to_string();
        assert!(validate(&invalid).is_err());
        invalid = valid;
        invalid.translations.insert(
            "Bad Key".to_string(),
            serde_json::json!({"frontend": "will ignore this entry"}),
        );
        validate(&invalid).unwrap();
    }

    #[test]
    fn fifo_pack_is_rejected_without_blocking() {
        let root = temporary_root("fifo");
        fs::create_dir_all(&root).unwrap();
        let fifo = root.join("blocked.json");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        let (packs, warnings) = load_from(&root);
        assert!(packs.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("regular JSON file"), "{warnings:?}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loading_is_bounded_by_file_count_and_file_size() {
        let root = temporary_root("bounds");
        for index in 0..=MAX_PACK_FILES {
            write_pack(
                &root,
                &format!("{index:02}.json"),
                &format!("zz-{index:02}"),
                serde_json::json!({}),
            );
        }
        fs::write(
            root.join("00.json"),
            vec![b' '; MAX_PACK_BYTES as usize + 1],
        )
        .unwrap();
        let (packs, warnings) = load_from(&root);
        assert_eq!(packs.len(), MAX_PACK_FILES - 1);
        assert!(warnings.iter().any(|warning| warning.contains("first 32")));
        assert!(warnings.iter().any(|warning| warning.contains("256 KiB")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_scanning_is_bounded() {
        let root = temporary_root("directory-bound");
        fs::create_dir_all(&root).unwrap();
        for index in 0..=MAX_DIRECTORY_ENTRIES {
            fs::write(root.join(format!("entry-{index:03}.txt")), b"").unwrap();
        }
        let (_, warnings) = load_from(&root);
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("first 128 entries")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn seeds_private_example_once_and_hides_the_template() {
        let parent = temporary_root("seed");
        fs::create_dir(&parent).unwrap();
        let root = parent.join("locales");
        let directory = open_language_directory_at(&root, true).unwrap().unwrap();
        seed_example_at(&directory).unwrap();
        let example = root.join(EXAMPLE_FILE_NAME);
        assert_eq!(fs::read(&example).unwrap(), EXAMPLE_SOURCE);
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&example).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(load_from(&root).0.is_empty());
        fs::write(&example, b"personal translation").unwrap();
        seed_example_at(&directory).unwrap();
        assert_eq!(fs::read(&example).unwrap(), b"personal translation");
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn refuses_symbolic_link_roots() {
        let root = temporary_root("symlink-root");
        let outside = root.join("outside");
        let linked = root.join("linked");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, &linked).unwrap();
        assert!(open_language_directory_at(&linked, true).is_err());
        assert_eq!(load_from(&linked).1.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
