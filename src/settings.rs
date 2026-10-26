use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::log;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Reminder {
    pub id: u32,
    pub due_unix: u64,
    pub label: String,
    pub repeat_daily: bool,
    #[serde(default = "default_reminder_enabled")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Todo {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub id: u32,
    pub text: String,
    pub created_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub radius_dip: f32,
    pub wander: bool,
    pub wander_speed: f32,
    pub lens_strength: f32,
    pub accent: String,
    pub custom_area: Option<[i32; 4]>,
    pub fullscreen_hide: bool,
    pub paused: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub reminders: Vec<Reminder>,
    pub reminder_sound: bool,
    pub todos: Vec<Todo>,
    pub notes: Vec<Note>,
    pub focus_until_unix: Option<u64>,
    pub focus_minutes: u32,
    pub focus_reminder_id: Option<u32>,
    pub intro_seen: bool,
    pub fun_enabled: bool,
    pub fun_interval_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            radius_dip: 52.0,
            wander: true,
            wander_speed: 22.0,
            lens_strength: 0.60,
            accent: "#FFE8D2".to_owned(),
            custom_area: None,
            fullscreen_hide: true,
            paused: false,
            x: None,
            y: None,
            reminders: Vec::new(),
            reminder_sound: true,
            todos: Vec::new(),
            notes: Vec::new(),
            focus_until_unix: None,
            focus_minutes: 25,
            focus_reminder_id: None,
            intro_seen: false,
            fun_enabled: false,
            fun_interval_seconds: 90,
        }
    }
}

impl Settings {
    pub fn clamp(&mut self) {
        self.radius_dip = self.radius_dip.clamp(28.0, 92.0);
        self.wander_speed = self.wander_speed.clamp(0.0, 120.0);
        self.lens_strength = self.lens_strength.clamp(0.0, 2.0);
        self.focus_minutes = self.focus_minutes.clamp(1, 180);
        self.fun_interval_seconds = self.fun_interval_seconds.clamp(30, 300);
        if parse_accent_hex(&self.accent).is_none() {
            self.accent = "#FFE8D2".to_owned();
        }
    }

    pub fn accent_linear(&self) -> [f32; 3] {
        parse_accent_hex(&self.accent).unwrap_or([1.0, 0.79, 0.62])
    }
}

pub fn parse_accent_hex(value: &str) -> Option<[f32; 3]> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&value[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&value[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&value[4..6], 16).ok()? as f32 / 255.0;
    let to_linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Some([to_linear(r), to_linear(g), to_linear(b)])
}

pub fn path() -> PathBuf {
    log::data_dir().join("settings.json")
}

pub fn load() -> Settings {
    let path = path();
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(mut settings) => {
                settings.clamp();
                settings
            }
            Err(error) => {
                log::warn(&format!("settings parse failed: {error}"));
                let backup = path.with_extension("json.bad");
                let _ = fs::copy(&path, &backup);
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings) {
    let mut settings = settings.clone();
    settings.clamp();
    let path = path();
    let tmp = path.with_extension("json.tmp");
    let result = serde_json::to_vec_pretty(&settings)
        .map_err(|e| e.to_string())
        .and_then(|bytes| fs::write(&tmp, bytes).map_err(|e| e.to_string()))
        .and_then(|_| fs::rename(&tmp, &path).map_err(|e| e.to_string()));
    if let Err(error) = result {
        log::error(&format!("settings save failed: {error}"));
    }
}

fn default_reminder_enabled() -> bool {
    true
}
