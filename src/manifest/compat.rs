//! Manifest wire-format compatibility. Everything leaves this module as the
//! current [`Manifest`] type; package and bundle code never carry legacy state.

use super::{probe_format, Compatibility, Manifest, FORMAT};
use crate::overrides::Overrides;
use crate::{Error, RomTarget};
use serde::Deserialize;
use std::collections::BTreeMap;

const FORMAT_1: u32 = 1;

pub(super) fn parse(
    raw: &str,
    targets: impl IntoIterator<Item = RomTarget>,
) -> Result<Manifest, Error> {
    match probe_format(raw)? {
        FORMAT => Manifest::parse(raw),
        FORMAT_1 => adapt_format_1(raw, targets),
        other => Err(Error::UnsupportedFormat(other)),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Format1Manifest {
    format: u32,
    name: String,
    version: semver::Version,
    title: String,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    netplay: Compatibility,
    #[serde(default)]
    rom_overrides: toml::Table,
}

fn adapt_format_1(
    raw: &str,
    targets: impl IntoIterator<Item = RomTarget>,
) -> Result<Manifest, Error> {
    let legacy: Format1Manifest = toml::from_str(raw)?;
    debug_assert_eq!(legacy.format, FORMAT_1);

    let mut fields = legacy.rom_overrides;
    let ranges: BTreeMap<RomTarget, Vec<[usize; 2]>> = match fields.remove("legal_chip_ranges") {
        Some(value) => value.try_into()?,
        None => BTreeMap::new(),
    };
    let common: Overrides = toml::Value::Table(fields).try_into()?;
    let mut rom_overrides = BTreeMap::new();
    if !common.is_empty() {
        rom_overrides.extend(targets.into_iter().map(|target| (target, common.clone())));
    }
    for (target, ranges) in ranges {
        if let Some([start, end]) = ranges.iter().find(|[start, end]| start > end) {
            return Err(Error::Invalid(format!(
                "rom_overrides.legal_chip_ranges.{target}: range start {start} exceeds end {end}"
            )));
        }
        let mut overrides = common.clone();
        overrides.legal_chip_ranges = Some(ranges);
        rom_overrides.insert(target, overrides);
    }

    let manifest = Manifest {
        format: FORMAT,
        name: legacy.name,
        version: legacy.version,
        title: legacy.title,
        authors: legacy.authors,
        license: legacy.license,
        source: legacy.source,
        netplay: legacy.netplay,
        rom_overrides,
    };
    manifest.validate()?;
    Ok(manifest)
}
