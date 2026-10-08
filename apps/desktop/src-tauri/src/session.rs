use crate::preferences::Preferences;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Action {
    Toggle,
    Enable,
    Disable,
    OpenSettings,
    CloseSettings,
    OpenGithub,
    RequestKeyAccess,
    Relaunch,
    DismissError,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum KeyAccess {
    Granted,
    #[cfg(any(target_os = "macos", test))]
    Denied,
    #[cfg(any(target_os = "macos", test))]
    Unknown,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    pub(crate) enabled: bool,
    pub(crate) preferences: Preferences,
    pub(crate) key_access: KeyAccess,
    /// Access was granted while running, which some macOS versions only honor after a relaunch.
    pub(crate) granted_while_running: bool,
    pub(crate) settings_open: bool,
    pub(crate) shortcut: &'static str,
    pub(crate) shortcut_unavailable: bool,
    pub(crate) error: Option<String>,
}

impl Session {
    pub(crate) fn new(preferences: Preferences, key_access: KeyAccess) -> Self {
        Self {
            enabled: true,
            preferences,
            key_access,
            granted_while_running: false,
            settings_open: false,
            shortcut: if cfg!(target_os = "macos") {
                "⌥⇧⌘K"
            } else {
                "Ctrl+Alt+Shift+K"
            },
            shortcut_unavailable: false,
            error: None,
        }
    }
    pub(crate) fn transition(&mut self, action: Action) -> bool {
        match action {
            Action::Toggle => self.enabled = !self.enabled,
            Action::Enable => self.enabled = true,
            Action::Disable => self.enabled = false,
            Action::OpenSettings => self.settings_open = true,
            Action::CloseSettings => {
                self.settings_open = false;
                let first_close = !self.preferences.onboarded;
                self.preferences.onboarded = true;
                return first_close;
            }
            Action::DismissError => self.error = None,
            Action::OpenGithub | Action::RequestKeyAccess | Action::Relaunch | Action::Quit => {}
        }
        false
    }
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn set_key_access(&mut self, access: KeyAccess) {
        if self.key_access != KeyAccess::Granted && access == KeyAccess::Granted {
            self.granted_while_running = true;
        }
        self.key_access = access;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session::new(Preferences::default(), KeyAccess::Unknown)
    }

    #[test]
    fn toggling_flips_enabled_from_every_source() {
        let mut s = session();
        assert!(s.enabled, "Glassview starts on");
        assert!(!s.transition(Action::Toggle));
        assert!(!s.enabled);
        s.transition(Action::Toggle);
        assert!(s.enabled);
        s.transition(Action::Disable);
        s.transition(Action::Disable);
        assert!(!s.enabled);
        s.transition(Action::Enable);
        assert!(s.enabled);
    }

    #[test]
    fn closing_settings_the_first_time_marks_onboarding_done_and_saves_once() {
        let mut s = session();
        s.transition(Action::OpenSettings);
        assert!(s.settings_open);
        assert!(s.transition(Action::CloseSettings));
        assert!(!s.settings_open);
        assert!(s.preferences.onboarded);
        s.transition(Action::OpenSettings);
        assert!(!s.transition(Action::CloseSettings), "already saved");
    }

    #[test]
    fn a_grant_while_running_is_remembered_for_the_relaunch_hint() {
        let mut s = session();
        s.set_key_access(KeyAccess::Denied);
        assert!(!s.granted_while_running);
        s.set_key_access(KeyAccess::Granted);
        assert!(s.granted_while_running);
        let at_launch = Session::new(Preferences::default(), KeyAccess::Granted);
        assert!(!at_launch.granted_while_running);
    }

    #[test]
    fn actions_and_access_keep_their_wire_names() {
        let parse = |name: &str| serde_json::from_value::<Action>(name.into()).unwrap();
        assert_eq!(parse("request-key-access"), Action::RequestKeyAccess);
        assert_eq!(parse("open-settings"), Action::OpenSettings);
        assert_eq!(parse("open-github"), Action::OpenGithub);
        assert!(serde_json::from_value::<Action>("show".into()).is_err());
        assert_eq!(serde_json::to_value(KeyAccess::Unknown).unwrap(), "unknown");
        let json = serde_json::to_value(session()).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "enabled",
                "error",
                "grantedWhileRunning",
                "keyAccess",
                "preferences",
                "settingsOpen",
                "shortcut",
                "shortcutUnavailable"
            ]
        );
    }
}
