//! Manifest wire-format compatibility. Everything leaves this module as the
//! current [`Manifest`] type; package and bundle code never carry legacy state.

use super::{probe_format, Manifest, FORMAT};
use crate::{Error, RomTarget};

const FORMAT_1: u32 = 1;

pub(super) fn parse(
    table: toml::Table,
    targets: impl IntoIterator<Item = RomTarget>,
) -> Result<Manifest, Error> {
    match probe_format(&table)? {
        FORMAT => Manifest::from_table(table),
        FORMAT_1 => adapt_format_1(table, targets),
        other => Err(Error::UnsupportedFormat(other)),
    }
}

fn adapt_format_1(
    mut table: toml::Table,
    targets: impl IntoIterator<Item = RomTarget>,
) -> Result<Manifest, Error> {
    let common: toml::Table = match table.remove("rom_overrides") {
        Some(value) => value.try_into()?,
        None => toml::Table::new(),
    };

    let mut rom_overrides = toml::Table::new();
    if !common.is_empty() {
        rom_overrides.extend(
            targets
                .into_iter()
                .map(|target| (target.to_string(), toml::Value::Table(common.clone()))),
        );
    }

    table.insert("format".into(), toml::Value::Integer(FORMAT.into()));
    if !rom_overrides.is_empty() {
        table.insert("rom_overrides".into(), toml::Value::Table(rom_overrides));
    }
    Manifest::from_table(table)
}
