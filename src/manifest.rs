//! `manifest.toml` — the one metadata file inside a `.tangopatch`.
//!
//! A package is exactly *one version* of one patch, so the manifest is
//! flat: there is no `[versions.'x.y.z']` nesting and nothing in it is
//! repeated per version.
//!
//! ```toml
//! format = 2
//! name = "bn6_allstars"
//! version = "1.1.0"
//! title = "BN6 All-Stars + BBN6"
//! authors = ["Someone <someone@example.com>"]
//! license = "MIT"
//! source = "https://github.com/luckytyphlosion/bn6-all-stars"
//! netplay = "group:bn6allstars"
//!
//! [rom_overrides.BR5E_00]
//! language = "en-US"
//! charset = [" ", "0", "1", ...]
//! legal_chip_ranges = [[1, 202], [221, 280], [301, 305]]
//!
//! [rom_overrides.BR6E_00]
//! language = "en-US"
//! charset = [" ", "0", "1", ...]
//! legal_chip_ranges = [[1, 202], [221, 280], [306, 310]]
//! ```
//!
//! Which games a package patches is still read off the archive's `roms/`
//! entries; target names appear in the manifest only when an override must
//! differ between those ROMs. The netplay family comes from the game being
//! played, which makes [`Compatibility::Vanilla`] and [`Compatibility::Group`]
//! impossible to state ambiguously — see [`crate::tag`].

mod compat;

use crate::overrides::Overrides;
use crate::{Error, RomTarget};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The manifest format emitted by this crate. The reader also accepts
/// legacy format 1 and upgrades it in memory.
pub const FORMAT: u32 = 2;

#[derive(Deserialize)]
struct FormatProbe {
    format: u32,
}

fn probe_format(raw: &str) -> Result<u32, Error> {
    Ok(toml::from_str::<FormatProbe>(raw)?.format)
}

/// Longest accepted patch name / netplay group.
pub const MAX_NAME_LEN: usize = 64;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
// A misspelled or stale key (`netplay_compatibility`, say) is an author
// mistake that would otherwise be silently ignored and leave the patch
// matching nothing.
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format version. Bumped only for breaking schema changes;
    /// [`Manifest::parse`] rejects anything it doesn't understand rather
    /// than silently ignoring fields.
    pub format: u32,
    /// Stable identifier, unique within a repo. Also the on-disk and URL
    /// stem — see [`crate::validate_name`] for the accepted charset.
    pub name: String,
    pub version: semver::Version,
    /// Human-readable name shown in the UI.
    pub title: String,
    /// `Display Name <email@address>` strings, as in `Cargo.toml`. Kept
    /// raw here; consumers that want just the display name parse them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// SPDX identifier. `None` means UNLICENSED.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// URL to the patch's source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Who this version can netplay against. Defaults to
    /// [`Compatibility::Isolated`] — the conservative choice, since a
    /// gameplay-affecting patch that silently claimed broader
    /// compatibility would desync rather than fail to match.
    #[serde(default, skip_serializing_if = "Compatibility::is_default")]
    pub netplay: Compatibility,
    /// Overrides applied on top of each exact patched ROM's own data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rom_overrides: BTreeMap<RomTarget, Overrides>,
}

impl Manifest {
    /// Parse and validate the current manifest format. Compatibility readers
    /// adapt older input to this format before constructing a `Manifest`.
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let format = probe_format(raw)?;
        if format != FORMAT {
            return Err(Error::UnsupportedFormat(format));
        }
        let manifest: Manifest = toml::from_str(raw)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Adapt supported legacy input at the I/O boundary. The returned value
    /// is always a complete, ordinary format-2 manifest.
    pub(crate) fn parse_compatible(
        raw: &str,
        targets: impl IntoIterator<Item = RomTarget>,
    ) -> Result<Self, Error> {
        compat::parse(raw, targets)
    }

    fn validate(&self) -> Result<(), Error> {
        crate::validate_name(&self.name).map_err(|e| Error::Invalid(format!("name: {e}")))?;
        if self.title.trim().is_empty() {
            return Err(Error::Invalid("title: must not be empty".into()));
        }
        if let Compatibility::Group(group) = &self.netplay {
            crate::validate_name(group).map_err(|e| Error::Invalid(format!("netplay group: {e}")))?;
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, Error> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// `<name>-<version>` — the package's file stem and index key.
    pub fn stem(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }

    /// `<name>-<version>.tangopatch`.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.stem(), crate::EXTENSION)
    }
}

/// Who a patch version may netplay against.
///
/// Serialized as a single string in both TOML and JSON so there is
/// exactly one way to write each case:
///
/// | value | meaning |
/// |---|---|
/// | `"isolated"` | only the identical patch at the identical version (default) |
/// | `"vanilla"`  | the unpatched game, and any other `vanilla` patch for it |
/// | `"group:NAME"` | anything else declaring the same group |
///
/// The old `netplay_compatibility` string could express all three, but
/// only by convention — vanilla meant "type out the ROM family name",
/// which then collided with any group that happened to be named after a
/// family. Here the cases are distinct variants, and the family is never
/// author-supplied at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Compatibility {
    /// This exact `(name, version)` and nothing else.
    #[default]
    Isolated,
    /// Cosmetic-only: interchangeable with the unpatched game.
    Vanilla,
    /// A named group, opted into deliberately. Shared across versions of
    /// one patch (so `1.0.1` can play `1.0.0`) or across patches that
    /// deliberately stay in lockstep.
    Group(String),
}

impl Compatibility {
    const ISOLATED: &'static str = "isolated";
    const VANILLA: &'static str = "vanilla";
    const GROUP_PREFIX: &'static str = "group:";

    fn is_default(&self) -> bool {
        *self == Compatibility::Isolated
    }

    pub fn as_str(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Compatibility::Isolated => Self::ISOLATED.into(),
            Compatibility::Vanilla => Self::VANILLA.into(),
            Compatibility::Group(g) => format!("{}{g}", Self::GROUP_PREFIX).into(),
        }
    }
}

impl std::fmt::Display for Compatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl std::str::FromStr for Compatibility {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        if let Some(group) = s.strip_prefix(Self::GROUP_PREFIX) {
            crate::validate_name(group).map_err(|e| Error::Invalid(format!("netplay group: {e}")))?;
            return Ok(Compatibility::Group(group.to_owned()));
        }
        match s {
            Self::ISOLATED => Ok(Compatibility::Isolated),
            Self::VANILLA => Ok(Compatibility::Vanilla),
            other => Err(Error::Invalid(format!(
                "netplay: expected \"isolated\", \"vanilla\", or \"group:NAME\", got {other:?}"
            ))),
        }
    }
}

impl Serialize for Compatibility {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for Compatibility {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
format = 2
name = "bn6_allstars"
version = "1.1.0"
title = "BN6 All-Stars"
"#;

    #[test]
    fn minimal_manifest_defaults_to_isolated() {
        let m = Manifest::parse(MINIMAL).unwrap();
        assert_eq!(m.netplay, Compatibility::Isolated);
        assert!(m.authors.is_empty());
        assert_eq!(m.stem(), "bn6_allstars-1.1.0");
        assert_eq!(m.file_name(), "bn6_allstars-1.1.0.tangopatch");
    }

    #[test]
    fn compatibility_round_trips_through_strings() {
        for c in [
            Compatibility::Isolated,
            Compatibility::Vanilla,
            Compatibility::Group("bn6allstars".into()),
        ] {
            assert_eq!(c.as_str().parse::<Compatibility>().unwrap(), c);
        }
    }

    #[test]
    fn bad_netplay_values_are_rejected() {
        for bad in ["", "group:", "Vanilla", "group:has spaces", "bn6"] {
            assert!(
                bad.parse::<Compatibility>().is_err(),
                "{bad:?} should not parse as a compatibility"
            );
        }
    }

    #[test]
    fn manifest_round_trips_through_toml() {
        let mut m = Manifest::parse(MINIMAL).unwrap();
        m.netplay = Compatibility::Group("bn6allstars".into());
        m.authors = vec!["Someone <someone@example.com>".into()];
        m.license = Some("MIT".into());
        assert_eq!(Manifest::parse(&m.to_toml().unwrap()).unwrap(), m);
    }

    #[test]
    fn rom_overrides_are_keyed_by_exact_target() {
        let raw = format!(
            r#"{MINIMAL}
[rom_overrides.BR5E_00]
language = "en-US"
legal_chip_ranges = [[1, 202], [301, 305]]

[rom_overrides.BR6E_00]
legal_chip_ranges = [[1, 202], [306, 310]]
"#
        );
        let manifest = Manifest::parse(&raw).unwrap();
        assert_eq!(manifest.rom_overrides.len(), 2);
        assert_eq!(
            manifest.rom_overrides[&"BR5E_00".parse().unwrap()]
                .legal_chip_ranges
                .as_deref(),
            Some([[1, 202], [301, 305]].as_slice())
        );
        assert_eq!(Manifest::parse(&manifest.to_toml().unwrap()).unwrap(), manifest);
    }

    #[test]
    fn format_1_is_adapted_to_a_complete_format_2_manifest() {
        let raw = r#"
format = 1
name = "legacy"
version = "1.0.0"
title = "Legacy"

[rom_overrides]
language = "en-US"
charset = [" ", "A"]

[rom_overrides.legal_chip_ranges]
BR5E_00 = [[1, 202], [301, 305]]
"#;
        let gregar = "BR5E_00".parse().unwrap();
        let falzar = "BR6E_00".parse().unwrap();
        assert!(matches!(
            Manifest::parse(raw),
            Err(Error::UnsupportedFormat(1))
        ));

        let manifest = Manifest::parse_compatible(raw, [gregar, falzar]).unwrap();

        assert_eq!(manifest.format, FORMAT);
        assert_eq!(
            manifest.rom_overrides[&gregar].legal_chip_ranges.as_deref(),
            Some([[1, 202], [301, 305]].as_slice())
        );
        assert_eq!(
            manifest.rom_overrides[&falzar].language.as_ref().unwrap().to_string(),
            "en-US"
        );
        assert!(manifest.rom_overrides[&falzar].legal_chip_ranges.is_none());

        let upgraded = manifest.to_toml().unwrap();
        assert!(upgraded.contains("format = 2"));
        assert!(upgraded.contains("[rom_overrides.BR5E_00]"));
        assert!(upgraded.contains("[rom_overrides.BR6E_00]"));
        assert_eq!(Manifest::parse(&upgraded).unwrap(), manifest);
    }

    #[test]
    fn format_1_ranges_are_validated_by_the_format_2_parser() {
        let raw = r#"
format = 1
name = "legacy"
version = "1.0.0"
title = "Legacy"

[rom_overrides.legal_chip_ranges]
BR5E_00 = [[202, 1]]
"#;
        let err = Manifest::parse_compatible(raw, ["BR5E_00".parse().unwrap()]).unwrap_err();
        assert!(matches!(err, Error::ManifestSyntax(_)));
        assert!(
            err.to_string()
                .contains("legal_chip_ranges: range start 202 exceeds end 1"),
            "{err}"
        );
    }

    #[test]
    fn unsupported_formats_are_rejected() {
        for format in [0, 3] {
            let raw = MINIMAL.replace("format = 2", &format!("format = {format}"));
            assert!(matches!(
                Manifest::parse(&raw),
                Err(Error::UnsupportedFormat(actual)) if actual == format
            ));
        }
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let raw = format!("{MINIMAL}netplay_compatibility = \"bn6\"\n");
        assert!(Manifest::parse(&raw).is_err());
    }
}
