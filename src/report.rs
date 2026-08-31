//! The JSON wire format.
//!
//! Every field name here is part of the CLI's contract — renaming one is a
//! breaking change, and `docs/src/commands/` must say the same thing.

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// What `madoqua doctor --json` emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionReport {
    /// The crate version this binary was built from.
    pub version: String,
}

impl VersionReport {
    /// Build the report from the resolved configuration.
    pub fn new(_config: &Config) -> Self {
        Self { version: env!("CARGO_PKG_VERSION").to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_round_trips_through_json() {
        let report = VersionReport::new(&Config::default());
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            serde_json::from_str::<VersionReport>(&json).unwrap(),
            report,
            "the wire format must survive a round trip"
        );
    }
}
