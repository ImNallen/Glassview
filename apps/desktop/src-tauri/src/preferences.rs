use crate::{keys::KeyMode, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{io::Write, path::Path, sync::LazyLock};
use tauri::Manager;

const FILE_NAME: &str = "preferences.json";
const BACKUP_NAME: &str = "preferences.json.bak";
const FORMAT_VERSION: u64 = 1;
const INVALID: &str = "Invalid preferences";

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(try_from = "String")]
pub(crate) struct HexColor(String);
impl TryFrom<String> for HexColor {
    type Error = &'static str;
    fn try_from(color: String) -> std::result::Result<Self, Self::Error> {
        let valid = color.len() == 7
            && color.starts_with('#')
            && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit);
        valid.then_some(Self(color)).ok_or(INVALID)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct RippleColors {
    pub(crate) left: HexColor,
    pub(crate) right: HexColor,
    pub(crate) middle: HexColor,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(try_from = "u8")]
pub(crate) struct RippleSize(u8);
impl TryFrom<u8> for RippleSize {
    type Error = &'static str;
    fn try_from(size: u8) -> std::result::Result<Self, Self::Error> {
        (24..=160)
            .contains(&size)
            .then_some(Self(size))
            .ok_or(INVALID)
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(try_from = "u16")]
pub(crate) struct FadeMs(u16);
impl TryFrom<u16> for FadeMs {
    type Error = &'static str;
    fn try_from(ms: u16) -> std::result::Result<Self, Self::Error> {
        (500..=5000)
            .contains(&ms)
            .then_some(Self(ms))
            .ok_or(INVALID)
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PillPosition {
    BottomCenter,
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PillSize {
    Small,
    Medium,
    Large,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preferences {
    pub(crate) ripple_colors: RippleColors,
    pub(crate) ripple_size: RippleSize,
    pub(crate) halo: bool,
    pub(crate) pill_position: PillPosition,
    pub(crate) pill_size: PillSize,
    pub(crate) fade_ms: FadeMs,
    pub(crate) keys: KeyMode,
    pub(crate) onboarded: bool,
}
const DEFAULTS_JSON: &str = include_str!("../../src/contract/preference-defaults.json");
static DEFAULTS: LazyLock<Preferences> = LazyLock::new(|| {
    serde_json::from_str(DEFAULTS_JSON).expect("preference-defaults.json holds valid preferences")
});
impl Default for Preferences {
    fn default() -> Self {
        DEFAULTS.clone()
    }
}
impl Preferences {
    pub(crate) fn parse(saved: Value) -> serde_json::Result<Self> {
        let Value::Object(fields) = saved else {
            return serde_json::from_value(saved);
        };
        let mut merged = serde_json::to_value(Self::default())?;
        for (field, value) in fields {
            merged[field] = value;
        }
        serde_json::from_value(merged)
    }
    pub(crate) fn load(app: &tauri::AppHandle) -> (Self, Option<String>) {
        match app.path().app_config_dir() {
            Ok(dir) => load_from(&dir),
            Err(error) => {
                log::error!("Could not find the preferences folder: {error}");
                (Self::default(), None)
            }
        }
    }

    pub(crate) fn save(&self, app: &tauri::AppHandle) -> Result<()> {
        let dir = app.path().app_config_dir()?;
        self.save_to(&dir)
    }

    fn save_to(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut saved = serde_json::to_value(self)?;
        saved["version"] = FORMAT_VERSION.into();
        let bytes = serde_json::to_vec_pretty(&saved)?;
        Ok(write_atomically(&dir.join(FILE_NAME), &bytes)?)
    }
}

fn load_from(dir: &Path) -> (Preferences, Option<String>) {
    const SOME_RESET: &str = "Some settings could not be read and were reset to their defaults.";
    const ALL_RESET: &str = "Your settings could not be read, so Glassview is using the defaults.";
    let path = dir.join(FILE_NAME);
    let backup = dir.join(BACKUP_NAME);
    let (preferences, problem, backed_up) = match std::fs::read(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Preferences::default(), None)
        }
        Err(error) => {
            log::error!("Could not read {}: {error}", path.display());
            (
                Preferences::default(),
                ALL_RESET,
                std::fs::rename(&path, &backup),
            )
        }
        Ok(bytes) => {
            let (preferences, problem) = match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(saved)) => {
                    if let Some(preferences) = parse(&saved) {
                        return (preferences, None);
                    }
                    let (preferences, reset) = recover(saved);
                    log::warn!("Reset unreadable preferences: {reset:?}");
                    (preferences, SOME_RESET)
                }
                _ => {
                    log::warn!(
                        "Reset all preferences: {} is not a JSON object",
                        path.display()
                    );
                    (Preferences::default(), ALL_RESET)
                }
            };
            (preferences, problem, write_atomically(&backup, &bytes))
        }
    };
    if let Err(error) = backed_up {
        log::error!("Could not back up {}: {error}", path.display());
        return (preferences, Some(problem.into()));
    }
    if let Err(error) = preferences.save_to(dir) {
        log::error!("Could not save recovered preferences: {error}");
    }
    (
        preferences,
        Some(format!(
            "{problem} The original file was kept as {BACKUP_NAME}."
        )),
    )
}

fn recover(saved: Map<String, Value>) -> (Preferences, Vec<String>) {
    let Ok(Value::Object(mut kept)) = serde_json::to_value(Preferences::default()) else {
        return (
            Preferences::default(),
            saved.into_iter().map(|(field, _)| field).collect(),
        );
    };
    let mut reset = Vec::new();
    for (field, value) in saved {
        let previous = kept.insert(field.clone(), value);
        if parse(&kept).is_none() {
            match previous {
                Some(previous) => kept.insert(field.clone(), previous),
                None => kept.remove(&field),
            };
            reset.push(field);
        }
    }
    (parse(&kept).unwrap_or_default(), reset)
}

fn parse(fields: &Map<String, Value>) -> Option<Preferences> {
    Preferences::parse(Value::Object(fields.clone())).ok()
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let written = std::fs::File::create(&temporary).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let result = written.and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const CUSTOM_JSON: &str = r##"{"rippleColors":{"left":"#123aBC","right":"#000000","middle":"#ffffff"},"rippleSize":24,"halo":true,"pillPosition":"top-right","pillSize":"large","fadeMs":5000,"keys":"all","onboarded":true}"##;
    fn with(field: &str, value: Value) -> serde_json::Result<Preferences> {
        let mut fields = serde_json::to_value(Preferences::default()).unwrap();
        fields[field] = value;
        serde_json::from_value(fields)
    }
    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glassview-preferences-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let defaults = Preferences::default();
        assert_eq!(
            defaults.keys,
            KeyMode::Shortcuts,
            "typed text is hidden by default"
        );
        assert_eq!(defaults.fade_ms, FadeMs(1500));
        assert_eq!(defaults.ripple_size, RippleSize(56));
        assert_eq!(defaults.pill_position, PillPosition::BottomCenter);
        assert!(!defaults.halo);
        assert!(!defaults.onboarded);
    }

    #[test]
    fn saved_and_sent_preferences_keep_their_exact_json() {
        let default_json = serde_json::to_string(&Preferences::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&default_json).unwrap(),
            serde_json::from_str::<Value>(DEFAULTS_JSON).unwrap()
        );
        for (name, json) in [("default", default_json.as_str()), ("custom", CUSTOM_JSON)] {
            let sent: Preferences = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&sent).unwrap(), json, "{name}");
            let mut file: Value = serde_json::from_str(json).unwrap();
            file["version"] = FORMAT_VERSION.into();
            let stored = serde_json::to_vec_pretty(&file).unwrap();
            let dir = scratch_dir(&format!("exact-{name}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(FILE_NAME), &stored).unwrap();
            let (loaded, warning) = load_from(&dir);
            assert!(warning.is_none(), "{name}");
            loaded.save_to(&dir).unwrap();
            assert_eq!(
                std::fs::read(dir.join(FILE_NAME)).unwrap(),
                stored,
                "{name}"
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn out_of_range_preferences_are_rejected() {
        for (field, value) in [
            ("rippleSize", json!(23)),
            ("rippleSize", json!(161)),
            ("fadeMs", json!(499)),
            ("fadeMs", json!(5001)),
            ("keys", json!("everything")),
            ("pillPosition", json!("center")),
            ("pillSize", json!("huge")),
            ("halo", json!("yes")),
            (
                "rippleColors",
                json!({"left": "#fff", "right": "#000000", "middle": "#000000"}),
            ),
            (
                "rippleColors",
                json!({"left": "#000000", "right": "#000000"}),
            ),
            (
                "rippleColors",
                json!({"left": "#gggggg", "right": "#000000", "middle": "#000000"}),
            ),
        ] {
            assert!(with(field, value.clone()).is_err(), "{field}: {value}");
        }
        for (field, value) in [
            ("rippleSize", json!(24)),
            ("rippleSize", json!(160)),
            ("fadeMs", json!(500)),
            ("fadeMs", json!(5000)),
        ] {
            assert!(with(field, value.clone()).is_ok(), "{field}: {value}");
        }
    }

    #[test]
    fn missing_preferences_load_defaults_without_a_warning() {
        let dir = scratch_dir("missing");
        let (preferences, warning) = load_from(&dir);
        assert_eq!(preferences, Preferences::default());
        assert!(warning.is_none());
    }

    #[test]
    fn saved_preferences_replace_the_file_and_load_back() {
        let dir = scratch_dir("saved");
        Preferences::default().save_to(&dir).unwrap();
        let preferences = Preferences {
            keys: KeyMode::All,
            ..Preferences::default()
        };
        preferences.save_to(&dir).unwrap();
        let (loaded, warning) = load_from(&dir);
        assert_eq!(loaded, preferences);
        assert!(warning.is_none());
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files, [FILE_NAME]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn damaged_preferences_are_backed_up_and_repaired_once() {
        let dir = scratch_dir("truncated");
        std::fs::create_dir_all(&dir).unwrap();
        let truncated = br##"{"halo":true,"rippleColors":{"left":"#4f"##;
        std::fs::write(dir.join(FILE_NAME), truncated).unwrap();
        let (preferences, warning) = load_from(&dir);
        assert_eq!(preferences, Preferences::default());
        assert_eq!(
            warning.unwrap(),
            "Your settings could not be read, so Glassview is using the defaults. \
             The original file was kept as preferences.json.bak."
        );
        assert_eq!(std::fs::read(dir.join(BACKUP_NAME)).unwrap(), truncated);
        let (reloaded, warning) = load_from(&dir);
        assert_eq!(reloaded, preferences);
        assert!(warning.is_none(), "the next launch starts clean");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn one_invalid_field_resets_only_that_field() {
        let dir = scratch_dir("field");
        std::fs::create_dir_all(&dir).unwrap();
        let saved = r##"{"rippleSize":999,"halo":true,"pillPosition":"top-left",
            "fadeMs":3000,"keys":"sideways","onboarded":true}"##;
        std::fs::write(dir.join(FILE_NAME), saved).unwrap();
        let (preferences, warning) = load_from(&dir);
        assert!(warning
            .unwrap()
            .starts_with("Some settings could not be read"));
        assert_eq!(
            preferences,
            Preferences {
                halo: true,
                pill_position: PillPosition::TopLeft,
                fade_ms: FadeMs(3000),
                onboarded: true,
                ..Preferences::default()
            }
        );
        let (_, reset) = recover(serde_json::from_str(saved).unwrap());
        assert_eq!(reset, ["keys", "rippleSize"]);
        let (reloaded, warning) = load_from(&dir);
        assert_eq!(reloaded, preferences);
        assert!(warning.is_none(), "the recovered fields were saved");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_preferences_are_moved_aside_before_defaults_are_saved() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch_dir("unreadable");
        let custom = Preferences {
            halo: true,
            ..Preferences::default()
        };
        custom.save_to(&dir).unwrap();
        let original = std::fs::read(dir.join(FILE_NAME)).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&path).is_ok() {
            // Running as root, which can read the file anyway.
            return;
        }
        let (preferences, warning) = load_from(&dir);
        assert_eq!(preferences, Preferences::default());
        assert!(warning.unwrap().contains(BACKUP_NAME));
        let backup = dir.join(BACKUP_NAME);
        std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
