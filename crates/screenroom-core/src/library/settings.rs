use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

use super::Library;

const AUTOPLAY_NEXT: &str = "autoplayNext";
const AUTO_SKIP_INTRO: &str = "autoSkipIntro";
const REDUCED_MOTION: &str = "reducedMotion";
const SUBTITLE_LANGUAGE: &str = "subtitleLanguage";
const AUDIO_LANGUAGE: &str = "audioLanguage";
const SUBTITLE_SCALE: &str = "subtitleScalePercent";
const SUBTITLE_POSITION: &str = "subtitlePositionPercent";
const DEFAULT_PROFILE: &str = "defaultProfileId";
const VOLUME_PERCENT: &str = "volumePercent";
const CONVERSION_FOLDER: &str = "conversionFolder";

const RETIRED_KEYS: &[&str] = &["hdrExtended"];

const SUBTITLE_SCALE_NORMAL: u16 = 100;

const SUBTITLE_POSITION_BOTTOM: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub autoplay_next: bool,

    pub auto_skip_intro: bool,

    pub reduced_motion: bool,

    pub subtitle_language: Option<String>,

    pub audio_language: Option<String>,

    pub subtitle_scale_percent: u16,

    pub subtitle_position_percent: u16,

    pub volume_percent: u16,

    pub conversion_folder: Option<String>,

    pub default_profile: Option<String>,
}

impl Preferences {
    pub const SUBTITLE_SCALE_MIN_PERCENT: u16 = 50;

    pub const SUBTITLE_SCALE_MAX_PERCENT: u16 = 300;

    pub const SUBTITLE_POSITION_MIN_PERCENT: u16 = 0;

    pub const SUBTITLE_POSITION_MAX_PERCENT: u16 = 100;
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            autoplay_next: true,
            auto_skip_intro: false,
            reduced_motion: false,
            subtitle_language: None,
            audio_language: None,
            subtitle_scale_percent: SUBTITLE_SCALE_NORMAL,
            subtitle_position_percent: SUBTITLE_POSITION_BOTTOM,
            volume_percent: 100,
            conversion_folder: None,
            default_profile: None,
        }
    }
}

impl Library {
    pub fn preferences(&self) -> Result<Preferences> {
        let connection = self.connection();
        let mut statement = connection.prepare("SELECT key, value FROM settings")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut preferences = Preferences::default();
        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                AUTOPLAY_NEXT => preferences.autoplay_next = value == "true",
                AUTO_SKIP_INTRO => preferences.auto_skip_intro = value == "true",
                REDUCED_MOTION => preferences.reduced_motion = value == "true",
                SUBTITLE_LANGUAGE => preferences.subtitle_language = text(&value),
                AUDIO_LANGUAGE => preferences.audio_language = text(&value),
                SUBTITLE_SCALE => {
                    preferences.subtitle_scale_percent = percent(
                        &value,
                        Preferences::SUBTITLE_SCALE_MIN_PERCENT,
                        Preferences::SUBTITLE_SCALE_MAX_PERCENT,
                        SUBTITLE_SCALE_NORMAL,
                    )
                }
                SUBTITLE_POSITION => {
                    preferences.subtitle_position_percent = percent(
                        &value,
                        Preferences::SUBTITLE_POSITION_MIN_PERCENT,
                        Preferences::SUBTITLE_POSITION_MAX_PERCENT,
                        SUBTITLE_POSITION_BOTTOM,
                    )
                }
                VOLUME_PERCENT => {
                    preferences.volume_percent = percent(&value, 0, 100, 100);
                }
                CONVERSION_FOLDER => preferences.conversion_folder = text(&value),
                DEFAULT_PROFILE => preferences.default_profile = text(&value),

                _ => {}
            }
        }
        Ok(preferences)
    }

    pub fn set_preferences(&self, preferences: &Preferences) -> Result<()> {
        let subtitle = language(preferences.subtitle_language.as_deref())?;
        let audio = language(preferences.audio_language.as_deref())?;

        let scale = within(
            preferences.subtitle_scale_percent,
            Preferences::SUBTITLE_SCALE_MIN_PERCENT,
            Preferences::SUBTITLE_SCALE_MAX_PERCENT,
            "subtitle_scale_out_of_range",
            "Subtitle size must be between 50% and 300%.",
        )?;
        let position = within(
            preferences.subtitle_position_percent,
            Preferences::SUBTITLE_POSITION_MIN_PERCENT,
            Preferences::SUBTITLE_POSITION_MAX_PERCENT,
            "subtitle_position_out_of_range",
            "Subtitle position must be between 0% and 100% of the screen height.",
        )?;
        let volume = within(
            preferences.volume_percent,
            0,
            100,
            "volume_out_of_range",
            "Volume must be between 0% and 100%.",
        )?;
        let default_profile = preferences
            .default_profile
            .as_deref()
            .map(|value| super::validated(value, 120).map(str::to_string))
            .transpose()?;
        let conversion_folder = preferences
            .conversion_folder
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| super::validated(value, 1_000).map(str::to_string))
            .transpose()?;

        if let Some(profile_id) = &default_profile {
            let known: bool = self.connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM profiles WHERE id = ?1)",
                [profile_id],
                |row| row.get(0),
            )?;
            if !known {
                return Err(CoreError::library(
                    "profile_missing",
                    "That profile is no longer in the library.",
                ));
            }
        }

        let mut connection = self.connection();
        let tx = connection.transaction()?;
        write(
            &tx,
            AUTOPLAY_NEXT,
            if preferences.autoplay_next {
                "true"
            } else {
                "false"
            },
        )?;
        write(
            &tx,
            AUTO_SKIP_INTRO,
            if preferences.auto_skip_intro {
                "true"
            } else {
                "false"
            },
        )?;
        write(
            &tx,
            REDUCED_MOTION,
            if preferences.reduced_motion {
                "true"
            } else {
                "false"
            },
        )?;
        write_optional(&tx, SUBTITLE_LANGUAGE, subtitle.as_deref())?;
        write_optional(&tx, AUDIO_LANGUAGE, audio.as_deref())?;
        write(&tx, SUBTITLE_SCALE, &scale.to_string())?;
        write(&tx, SUBTITLE_POSITION, &position.to_string())?;
        write(&tx, VOLUME_PERCENT, &volume.to_string())?;
        write_optional(&tx, CONVERSION_FOLDER, conversion_folder.as_deref())?;
        write_optional(&tx, DEFAULT_PROFILE, default_profile.as_deref())?;
        for retired in RETIRED_KEYS {
            tx.execute("DELETE FROM settings WHERE key = ?1", params![retired])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let key = super::validated(key, 120)?;
        self.connection().execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn percent(value: &str, min: u16, max: u16, fallback: u16) -> u16 {
    match value.trim().parse::<u16>() {
        Ok(parsed) => parsed.clamp(min, max),
        Err(_) => fallback,
    }
}

fn within(value: u16, min: u16, max: u16, code: &'static str, message: &str) -> Result<u16> {
    if (min..=max).contains(&value) {
        return Ok(value);
    }
    Err(CoreError::library(code, message))
}

fn text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn language(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > 35
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CoreError::library(
            "invalid_language",
            "Use a language tag such as \"en\" or \"pt-BR\".",
        ));
    }
    Ok(Some(trimmed.to_string()))
}

fn write(tx: &Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn write_optional(tx: &Transaction<'_>, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => write(tx, key, value),
        None => {
            tx.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            Ok(())
        }
    }
}
