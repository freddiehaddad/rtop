//! Typed enums for closed-set config fields.
//!
//! Each enum follows the same pattern:
//! * `as_str` is the **single source of truth** for the canonical
//!   lowercase string form.
//! * `Display`, `Serialize`, `FromStr`, and `Deserialize` all
//!   delegate to `as_str` / `FromStr` — no string literals
//!   duplicated.
//! * `ALL` exposes every variant in cycle order; `NAMES` is the
//!   matching `&[&str]` for the options menu's `browsable_values`.
//!   A unit test asserts `NAMES[i] == ALL[i].as_str()` so the
//!   two cannot drift.
//!
//! `ProcSort` lives in `crate::collect::process_display` because
//! it owns the sort-dispatch logic; reference it directly there
//! when needed.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

// ---------------------------------------------------------------------------
// TempScale
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
#[error("invalid TempScale value '{0}'")]
pub struct ParseTempScaleError(pub String);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TempScale {
    #[default]
    Celsius,
    Fahrenheit,
    Kelvin,
    Rankine,
}

impl TempScale {
    pub const ALL: &'static [TempScale] = &[
        TempScale::Celsius,
        TempScale::Fahrenheit,
        TempScale::Kelvin,
        TempScale::Rankine,
    ];

    pub const NAMES: &'static [&'static str] = &["celsius", "fahrenheit", "kelvin", "rankine"];

    pub const fn as_str(self) -> &'static str {
        match self {
            TempScale::Celsius => "celsius",
            TempScale::Fahrenheit => "fahrenheit",
            TempScale::Kelvin => "kelvin",
            TempScale::Rankine => "rankine",
        }
    }
}

impl fmt::Display for TempScale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TempScale {
    type Err = ParseTempScaleError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TempScale::ALL
            .iter()
            .copied()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseTempScaleError(s.to_string()))
    }
}

impl Serialize for TempScale {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TempScale {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <String>::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// GraphSymbol
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
#[error("invalid GraphSymbol value '{0}'")]
pub struct ParseGraphSymbolError(pub String);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GraphSymbol {
    /// Sentinel that means "inherit from the global `graph_symbol`".
    /// Only meaningful for per-widget overrides; the global field
    /// itself never carries this value.
    #[default]
    Default,
    Braille,
    Block,
}

impl GraphSymbol {
    pub const ALL: &'static [GraphSymbol] = &[
        GraphSymbol::Default,
        GraphSymbol::Braille,
        GraphSymbol::Block,
    ];

    pub const NAMES: &'static [&'static str] = &["default", "braille", "block"];

    pub const fn as_str(self) -> &'static str {
        match self {
            GraphSymbol::Default => "default",
            GraphSymbol::Braille => "braille",
            GraphSymbol::Block => "block",
        }
    }
}

impl fmt::Display for GraphSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for GraphSymbol {
    type Err = ParseGraphSymbolError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GraphSymbol::ALL
            .iter()
            .copied()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseGraphSymbolError(s.to_string()))
    }
}

impl Serialize for GraphSymbol {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GraphSymbol {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <String>::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// CpuGraphSource
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
#[error("invalid CpuGraphSource value '{0}'")]
pub struct ParseCpuGraphSourceError(pub String);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CpuGraphSource {
    #[default]
    Auto,
    Total,
    User,
    System,
}

impl CpuGraphSource {
    pub const ALL: &'static [CpuGraphSource] = &[
        CpuGraphSource::Auto,
        CpuGraphSource::Total,
        CpuGraphSource::User,
        CpuGraphSource::System,
    ];

    pub const NAMES: &'static [&'static str] = &["auto", "total", "user", "system"];

    pub const fn as_str(self) -> &'static str {
        match self {
            CpuGraphSource::Auto => "auto",
            CpuGraphSource::Total => "total",
            CpuGraphSource::User => "user",
            CpuGraphSource::System => "system",
        }
    }

    /// User-facing graph-overlay label. `Auto` resolves to `"total"`
    /// (the underlying data series does too — see
    /// [`crate::domain::cpu::CpuPercent::series`]) so the on-screen
    /// label always names the actual series being plotted.
    pub const fn display_label(self) -> &'static str {
        match self {
            CpuGraphSource::User => "user",
            CpuGraphSource::System => "system",
            CpuGraphSource::Auto | CpuGraphSource::Total => "total",
        }
    }
}

impl fmt::Display for CpuGraphSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CpuGraphSource {
    type Err = ParseCpuGraphSourceError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CpuGraphSource::ALL
            .iter()
            .copied()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseCpuGraphSourceError(s.to_string()))
    }
}

impl Serialize for CpuGraphSource {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CpuGraphSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <String>::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_scale_default_is_celsius() {
        assert_eq!(TempScale::default(), TempScale::Celsius);
    }

    #[test]
    fn temp_scale_names_match_all() {
        assert_eq!(TempScale::NAMES.len(), TempScale::ALL.len());
        for (name, value) in TempScale::NAMES.iter().zip(TempScale::ALL.iter()) {
            assert_eq!(*name, value.as_str());
        }
    }

    #[test]
    fn temp_scale_round_trips() {
        for value in TempScale::ALL.iter().copied() {
            let s = value.as_str();
            assert_eq!(s.parse::<TempScale>().unwrap(), value);
            assert_eq!(value.to_string(), s);
        }
    }

    #[test]
    fn temp_scale_rejects_invalid() {
        assert!("celcius".parse::<TempScale>().is_err());
        assert!("Celsius".parse::<TempScale>().is_err());
        assert!("".parse::<TempScale>().is_err());
    }

    #[test]
    fn graph_symbol_default_is_default() {
        assert_eq!(GraphSymbol::default(), GraphSymbol::Default);
    }

    #[test]
    fn graph_symbol_names_match_all() {
        assert_eq!(GraphSymbol::NAMES.len(), GraphSymbol::ALL.len());
        for (name, value) in GraphSymbol::NAMES.iter().zip(GraphSymbol::ALL.iter()) {
            assert_eq!(*name, value.as_str());
        }
    }

    #[test]
    fn graph_symbol_round_trips() {
        for value in GraphSymbol::ALL.iter().copied() {
            let s = value.as_str();
            assert_eq!(s.parse::<GraphSymbol>().unwrap(), value);
            assert_eq!(value.to_string(), s);
        }
    }

    #[test]
    fn graph_symbol_rejects_invalid() {
        assert!("ascii".parse::<GraphSymbol>().is_err());
        assert!("Block".parse::<GraphSymbol>().is_err());
    }

    #[test]
    fn cpu_graph_source_default_is_auto() {
        assert_eq!(CpuGraphSource::default(), CpuGraphSource::Auto);
    }

    #[test]
    fn cpu_graph_source_names_match_all() {
        assert_eq!(CpuGraphSource::NAMES.len(), CpuGraphSource::ALL.len());
        for (name, value) in CpuGraphSource::NAMES.iter().zip(CpuGraphSource::ALL.iter()) {
            assert_eq!(*name, value.as_str());
        }
    }

    #[test]
    fn cpu_graph_source_round_trips() {
        for value in CpuGraphSource::ALL.iter().copied() {
            let s = value.as_str();
            assert_eq!(s.parse::<CpuGraphSource>().unwrap(), value);
            assert_eq!(value.to_string(), s);
        }
    }

    #[test]
    fn cpu_graph_source_rejects_invalid() {
        assert!("Auto".parse::<CpuGraphSource>().is_err());
        assert!("kernel".parse::<CpuGraphSource>().is_err());
    }
}
