//! User-customisable theme and interface settings.
//!
//! The core owns these values so every host (Android, the web simulator in
//! `design/ui-simulator`) persists and validates the same schema. Hosts render
//! the look; the core only stores, clamps and derives plain numbers from it.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const BLUR_RANGE: (u8, u8) = (0, 60);
pub const GLASS_OPACITY_RANGE: (u8, u8) = (4, 85);
pub const CORNER_RADIUS_RANGE: (u8, u8) = (6, 38);
pub const FONT_SCALE_RANGE: (u8, u8) = (85, 130);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Wallpaper {
    #[default]
    Aurora,
    Sunset,
    Ocean,
    Forest,
    Mono,
    /// Colours follow the artwork of the current track.
    Album,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Cozy,
    Roomy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoverShape {
    Square,
    #[default]
    Rounded,
    Circle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListStyle {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NavStyle {
    #[default]
    Floating,
    Docked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPreset {
    AuroraGlass,
    Sakura,
    Midnight,
    Minimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub theme: ThemeMode,
    /// Accent colour as `#rrggbb`.
    pub accent: String,
    pub wallpaper: Wallpaper,
    pub blur_px: u8,
    pub glass_opacity_percent: u8,
    pub corner_radius_px: u8,
    pub font_scale_percent: u8,
    pub density: Density,
    pub cover_shape: CoverShape,
    pub list_style: ListStyle,
    pub nav_style: NavStyle,
    pub edge_glow: bool,
    pub animations: bool,
    pub visualizer: bool,
    pub spin_circular_cover: bool,
    pub show_duration: bool,
    pub show_artists_tab: bool,
    pub show_genres_tab: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            accent: "#3e91ff".to_owned(),
            wallpaper: Wallpaper::Aurora,
            blur_px: 28,
            glass_opacity_percent: 22,
            corner_radius_px: 24,
            font_scale_percent: 100,
            density: Density::Cozy,
            cover_shape: CoverShape::Rounded,
            list_style: ListStyle::List,
            nav_style: NavStyle::Floating,
            edge_glow: true,
            animations: true,
            visualizer: true,
            spin_circular_cover: false,
            show_duration: true,
            show_artists_tab: true,
            show_genres_tab: true,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UiSettingsError {
    #[error("accent must look like #rrggbb")]
    InvalidAccent,
    #[error("{0} is outside its allowed range")]
    OutOfRange(&'static str),
}

fn in_range(value: u8, (min, max): (u8, u8)) -> bool {
    (min..=max).contains(&value)
}

impl UiSettings {
    pub fn preset(preset: UiPreset) -> Self {
        let base = Self::default();
        match preset {
            UiPreset::AuroraGlass => base,
            UiPreset::Sakura => Self {
                theme: ThemeMode::Light,
                accent: "#ff5d8f".to_owned(),
                wallpaper: Wallpaper::Sunset,
                blur_px: 30,
                glass_opacity_percent: 50,
                corner_radius_px: 28,
                edge_glow: false,
                ..base
            },
            UiPreset::Midnight => Self {
                accent: "#8b5cf6".to_owned(),
                wallpaper: Wallpaper::Ocean,
                blur_px: 36,
                glass_opacity_percent: 30,
                corner_radius_px: 20,
                ..base
            },
            UiPreset::Minimal => Self {
                theme: ThemeMode::System,
                accent: "#2fd08a".to_owned(),
                wallpaper: Wallpaper::Mono,
                blur_px: 44,
                glass_opacity_percent: 40,
                corner_radius_px: 12,
                edge_glow: false,
                ..base
            },
        }
    }

    pub fn validate(&self) -> Result<(), UiSettingsError> {
        if self.accent_rgb().is_none() {
            return Err(UiSettingsError::InvalidAccent);
        }
        let checks = [
            (self.blur_px, BLUR_RANGE, "blur"),
            (
                self.glass_opacity_percent,
                GLASS_OPACITY_RANGE,
                "glass opacity",
            ),
            (self.corner_radius_px, CORNER_RADIUS_RANGE, "corner radius"),
            (self.font_scale_percent, FONT_SCALE_RANGE, "font scale"),
        ];
        for (value, range, name) in checks {
            if !in_range(value, range) {
                return Err(UiSettingsError::OutOfRange(name));
            }
        }
        Ok(())
    }

    /// Clamps numeric fields and replaces an unusable accent with the default,
    /// so settings loaded from an older or hand-edited file are always safe to apply.
    pub fn sanitized(mut self) -> Self {
        self.blur_px = self.blur_px.clamp(BLUR_RANGE.0, BLUR_RANGE.1);
        self.glass_opacity_percent = self
            .glass_opacity_percent
            .clamp(GLASS_OPACITY_RANGE.0, GLASS_OPACITY_RANGE.1);
        self.corner_radius_px = self
            .corner_radius_px
            .clamp(CORNER_RADIUS_RANGE.0, CORNER_RADIUS_RANGE.1);
        self.font_scale_percent = self
            .font_scale_percent
            .clamp(FONT_SCALE_RANGE.0, FONT_SCALE_RANGE.1);
        if self.accent_rgb().is_none() {
            self.accent = Self::default().accent;
        }
        self
    }

    pub fn accent_rgb(&self) -> Option<(u8, u8, u8)> {
        let hex = self.accent.strip_prefix('#')?;
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some((channel(0)?, channel(2)?, channel(4)?))
    }

    /// True when text drawn on the accent colour should be dark for legibility.
    pub fn accent_needs_dark_text(&self) -> bool {
        self.accent_rgb().is_some_and(|(r, g, b)| {
            0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 165.0
        })
    }

    /// Glass panel opacity as a 0.0..=1.0 fraction.
    pub fn glass_opacity(&self) -> f32 {
        f32::from(self.glass_opacity_percent) / 100.0
    }

    pub fn font_scale(&self) -> f32 {
        f32::from(self.font_scale_percent) / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(UiSettings::default().validate(), Ok(()));
    }

    #[test]
    fn presets_are_valid_and_distinct() {
        let all = [
            UiPreset::AuroraGlass,
            UiPreset::Sakura,
            UiPreset::Midnight,
            UiPreset::Minimal,
        ]
        .map(UiSettings::preset);
        for s in &all {
            assert_eq!(s.validate(), Ok(()));
        }
        assert_ne!(all[0], all[1]);
        assert_ne!(all[2], all[3]);
    }

    #[test]
    fn rejects_bad_accent_and_ranges() {
        let s = UiSettings {
            accent: "blue".to_owned(),
            ..UiSettings::default()
        };
        assert_eq!(s.validate(), Err(UiSettingsError::InvalidAccent));
        let s = UiSettings {
            blur_px: 90,
            ..UiSettings::default()
        };
        assert_eq!(s.validate(), Err(UiSettingsError::OutOfRange("blur")));
    }

    #[test]
    fn sanitize_clamps_and_repairs() {
        let s = UiSettings {
            blur_px: 200,
            glass_opacity_percent: 0,
            corner_radius_px: 1,
            font_scale_percent: 250,
            accent: "#12".to_owned(),
            ..UiSettings::default()
        }
        .sanitized();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.blur_px, 60);
        assert_eq!(s.glass_opacity_percent, 4);
        assert_eq!(s.accent, "#3e91ff");
    }

    #[test]
    fn accent_parsing_and_text_contrast() {
        let s = UiSettings::default();
        assert_eq!(s.accent_rgb(), Some((0x3e, 0x91, 0xff)));
        assert!(!s.accent_needs_dark_text());
        let light = UiSettings {
            accent: "#f5e8a0".to_owned(),
            ..s
        };
        assert!(light.accent_needs_dark_text());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // `#[serde(default)]` lets older saved settings keep loading as fields are added.
        assert_eq!(UiSettings::default().glass_opacity(), 0.22);
    }
}
