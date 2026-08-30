use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "Mouseless";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub key: String,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    #[serde(default)]
    pub win: bool,
}

impl Hotkey {
    pub fn default_overlay() -> Hotkey {
        // macOS default is Option+U; Option maps to Alt on Linux.
        Hotkey {
            key: "U".to_string(),
            control: false,
            alt: true,
            shift: false,
            win: false,
        }
    }

    pub fn display_name(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.control {
            parts.push("Control");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.win {
            parts.push("Super");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(self.key.as_str());
        parts.join("+")
    }

    pub fn matches(&self, label: &str, control: bool, alt: bool, shift: bool, win: bool) -> bool {
        Self::normalized_key(label) == Self::normalized_key(&self.key)
            && control == self.control
            && alt == self.alt
            && shift == self.shift
            && win == self.win
    }

    pub fn normalized(&self) -> Hotkey {
        let mut copy = self.clone();
        copy.key = Self::normalized_key(&copy.key);
        if !copy.control && !copy.alt && !copy.shift && !copy.win {
            copy.alt = true;
        }
        copy
    }

    pub fn normalized_key(value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.chars().count() == 1 {
            return trimmed.to_uppercase();
        }
        match trimmed.to_lowercase().as_str() {
            "esc" => "Escape".to_string(),
            "return" | "enter" => "Enter".to_string(),
            "left" => "ArrowLeft".to_string(),
            "right" => "ArrowRight".to_string(),
            "up" => "ArrowUp".to_string(),
            "down" => "ArrowDown".to_string(),
            _ => {
                let mut chars = trimmed.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub grid_rows: i32,
    pub grid_columns: i32,
    pub overlay_opacity: f64,
    pub continuous_mode: bool,
    pub free_mode_step: f64,
    pub scroll_step: i32,
    pub overlay_hotkey: Hotkey,
    pub quit_grid_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            grid_rows: 5,
            grid_columns: 5,
            overlay_opacity: 0.72,
            continuous_mode: false,
            free_mode_step: 26.0,
            scroll_step: 18,
            overlay_hotkey: Hotkey::default_overlay(),
            quit_grid_key: "Q".to_string(),
        }
    }
}

impl Settings {
    pub fn clamped(&self) -> Settings {
        let mut copy = self.clone();
        copy.grid_rows = copy.grid_rows.clamp(3, 5);
        copy.grid_columns = copy.grid_columns.clamp(3, 5);
        copy.overlay_opacity = copy.overlay_opacity.clamp(0.25, 0.95);
        copy.free_mode_step = copy.free_mode_step.clamp(6.0, 90.0);
        copy.overlay_hotkey = copy.overlay_hotkey.normalized();
        copy.quit_grid_key = Hotkey::normalized_key(&copy.quit_grid_key);
        if copy.quit_grid_key.is_empty() {
            copy.quit_grid_key = "Q".to_string();
        }
        copy
    }
}

/// Loads and persists settings from `$XDG_CONFIG_HOME/Mouseless/config.json`.
pub struct SettingsStore {
    pub settings: Settings,
    path: std::path::PathBuf,
}

fn config_dir() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join(APP_NAME);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".config").join(APP_NAME)
}

impl SettingsStore {
    pub fn new() -> Self {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");

        let settings = if let Ok(data) = std::fs::read(&path) {
            serde_json::from_slice::<Settings>(&data)
                .map(|s| s.clamped())
                .unwrap_or_default()
        } else {
            Settings::default()
        };

        let store = SettingsStore { settings, path };
        if !store.path.exists() {
            store.save();
        }
        store
    }

    pub fn save(&self) {
        if let Ok(data) = serde_json::to_vec_pretty(&self.settings) {
            let _ = std::fs::write(&self.path, data);
        }
    }

    pub fn reload(&mut self) {
        if let Ok(data) = std::fs::read(&self.path) {
            if let Ok(s) = serde_json::from_slice::<Settings>(&data) {
                self.settings = s.clamped();
            }
        }
    }
}
