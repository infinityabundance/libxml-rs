//! Capability epochs and compatibility profiles (§68, §85 Phase 11, 11.1-R).
//!
//! Historical behavior differences must flow through deliberate compatibility
//! structures, never through scattered `if version == ...` branches. Each
//! behavioral capability whose semantics changed at a documented upstream
//! boundary is modelled as a *capability epoch*; a [`CompatibilityProfile`]
//! resolves every capability for a target upstream version pair.
//!
//! The epoch boundaries are derived from the evidence in
//! `atlas/SEMANTIC_EPOCHS.md` (E-001..E-008) and the surface delta engine
//! (`tools/evidence/surface_delta_engine.py` -> `atlas/HISTORICAL_SURFACE_EPOCHS.json`).
//! The candidate currently implements the current-system behavior
//! (libxml2 2.15.3 / libxslt 1.1.45); the resolver exists so that future
//! historical-emulation work (and any regression triage against older oracles)
//! addresses one deliberate structure instead of ad-hoc version checks.
//!
//! # Capabilities
//!
//! | Capability | Upstream evidence | Boundary |
//! |---|---|---|
//! | `XPathNodeSetSerialization` | E-001 (xmllint --xpath output) | 2.9.10 |
//! | `ParserDiagnostic` | E-002 (second parse-error diagnostic) | 2.12.x |
//! | `EntityCompactStorage` | E-004 (entity content debug node) | 2.13.0 |
//! | `ValidationExit` | E-005 (parser/validation exit codes) | 2.13.0 |
//! | `XpathAttrEmptyExit` | E-003 (empty node-set exit code) | 2.11.0 / 2.12.6 |
//! | `HtmlSerializer` | E-007 (HTML dump newlines) | 2.15.0 |
//! | `ValidationNoDtdExit` | E-006 (--valid without DTD) | 2.15.0 |
//! | `GlobalStateInit` | 2.12 lazy-init rework | 2.12.0 |
//! | `XslTransform` | E-008 (libxslt output frozen) | stable since ≤1.1.26 |

use core::fmt;

/// Value of the XPath node-set serialization capability (E-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XPathNodeSetSerialization {
    /// `xmllint --xpath` prints nodes concatenated (<= 2.9.4).
    Concatenated,
    /// `xmllint --xpath` prints one node per line with a final newline
    /// (>= 2.9.10; upstream-documented breaking change, commit da35eeae).
    NewlineSeparated,
}

/// Value of the parser-diagnostic capability (E-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserDiagnostic {
    /// Two diagnostics for unexpected EOF ("Premature end of data in tag ..."
    /// as the second line) — libxml2 <= 2.9.4 and >= 2.9.11 (fix de5b624f).
    Dual,
    /// The 2.9.10 regression variant ("EndTag: '</' not found").
    Regression,
    /// Single diagnostic; the second line was dropped in the 2.12.x error
    /// handling rework (>= 2.12.6; crate's current epoch).
    Single,
}

/// Value of the entity-content storage capability (E-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityCompactStorage {
    /// `--debug --noent` dumps the entity content child as `TEXT` (<= 2.12.6).
    Plain,
    /// Dumps as `TEXT compact` (>= 2.13.0, commit 8d04f0ee).
    Compact,
}

/// Value of the validation-exit-code capability (E-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationExit {
    /// parse-error/undeclared exit 1, valid-invalid exit 4 (<= 2.12.6).
    Legacy,
    /// parse-error/undeclared exit 4, valid-invalid exit 3 (>= 2.13.0).
    Reworked,
}

/// Value of the `xpath-attr` empty node-set exit code (E-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpathAttrEmptyExit {
    /// Exit 10 (<= 2.9.x era; "XPath set is empty").
    Legacy,
    /// Exit 0 (2.11.0..2.12.5, commit e85f9b98).
    NoError,
    /// Exit 11 (>= 2.12.6, commit 387a952b).
    Error11,
}

/// Value of the HTML serialization capability (E-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlSerializer {
    /// Newline after elements in the dump path (<= 2.14.1).
    Formatted,
    /// Single-line output (>= 2.15.0; newline writes removed from HTMLtree.c).
    SingleLine,
}

/// Value of the `--valid`-without-DTD exit capability (E-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationNoDtdExit {
    /// Exit 3 (2.13.0..2.14.1).
    Error3,
    /// Exit 0 (>= 2.15.0).
    Ok0,
}

/// Value of the global-state initialisation capability (2.12 rework).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalStateInit {
    /// Eager static initialisation (<= 2.11.x).
    Eager,
    /// Lazy per-context initialisation (>= 2.12.0).
    Lazy,
}

/// Value of the libxslt transform output capability (E-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XslTransform {
    /// Transform output frozen; byte-identical 1.1.26 .. 1.1.45.
    Stable,
}

/// Every capability the profiles module tracks, with its resolved value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub xpath_node_set_serialization: XPathNodeSetSerialization,
    pub parser_diagnostic: ParserDiagnostic,
    pub entity_compact_storage: EntityCompactStorage,
    pub validation_exit: ValidationExit,
    pub xpath_attr_empty_exit: XpathAttrEmptyExit,
    pub html_serializer: HtmlSerializer,
    pub validation_no_dtd_exit: ValidationNoDtdExit,
    pub global_state_init: GlobalStateInit,
    pub xsl_transform: XslTransform,
}

/// Parse a version string like `"2.15.3"` into `(major, minor, patch)`.
fn parse_version(v: &str) -> (u32, u32, u32) {
    let mut it = v.trim_start_matches('v').split('.');
    let major = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

fn at_least(version: &str, major: u32, minor: u32) -> bool {
    let (maj, min, _) = parse_version(version);
    (maj, min) >= (major, minor)
}

/// Resolve the capability values for a target libxml2 version.
///
/// Boundaries are evidence-backed (E-001..E-008); see the module docs for
/// the exact upstream commits/releases that created each change.
pub fn capabilities_for_libxml2(version: &str) -> Capabilities {
    Capabilities {
        xpath_node_set_serialization: {
            let (maj, min, pat) = parse_version(version);
            if (maj, min) > (2, 9) || (maj == 2 && min == 9 && pat >= 10) {
                XPathNodeSetSerialization::NewlineSeparated
            } else {
                XPathNodeSetSerialization::Concatenated
            }
        },
        parser_diagnostic: {
            let (maj, min, pat) = parse_version(version);
            if (maj, min, pat) >= (2, 9, 10) && (maj, min, pat) < (2, 9, 11) {
                ParserDiagnostic::Regression
            } else if (maj, min) >= (2, 12) {
                ParserDiagnostic::Single
            } else {
                ParserDiagnostic::Dual
            }
        },
        entity_compact_storage: if at_least(version, 2, 13) {
            EntityCompactStorage::Compact
        } else {
            EntityCompactStorage::Plain
        },
        validation_exit: if at_least(version, 2, 13) {
            ValidationExit::Reworked
        } else {
            ValidationExit::Legacy
        },
        xpath_attr_empty_exit: {
            let (maj, min) = (parse_version(version).0, parse_version(version).1);
            if (maj, min) < (2, 11) {
                XpathAttrEmptyExit::Legacy
            } else if (maj, min) < (2, 12)
                || (maj == 2 && min == 12 && parse_version(version).2 < 6)
            {
                XpathAttrEmptyExit::NoError
            } else {
                XpathAttrEmptyExit::Error11
            }
        },
        html_serializer: if at_least(version, 2, 15) {
            HtmlSerializer::SingleLine
        } else {
            HtmlSerializer::Formatted
        },
        validation_no_dtd_exit: if at_least(version, 2, 15) {
            ValidationNoDtdExit::Ok0
        } else {
            ValidationNoDtdExit::Error3
        },
        global_state_init: if at_least(version, 2, 12) {
            GlobalStateInit::Lazy
        } else {
            GlobalStateInit::Eager
        },
        xsl_transform: XslTransform::Stable,
    }
}

/// A resolved compatibility profile for a target upstream version pair.
///
/// The candidate's current target is the system oracle
/// (libxml2 2.15.3 / libxslt 1.1.45); emulating older releases resolves this
/// profile against the capability table instead of ad-hoc version branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityProfile {
    /// Target libxml2 version, e.g. "2.15.3".
    pub libxml2_version: &'static str,
    /// Target libxslt version, e.g. "1.1.45".
    pub libxslt_version: &'static str,
    /// Resolved capabilities.
    pub capabilities: Capabilities,
}

impl CompatibilityProfile {
    /// The candidate's current-system profile (libxml2 2.15.3 / libxslt 1.1.45).
    pub fn current() -> CompatibilityProfile {
        CompatibilityProfile {
            libxml2_version: "2.15.3",
            libxslt_version: "1.1.45",
            capabilities: capabilities_for_libxml2("2.15.3"),
        }
    }

    /// Resolve a profile for an explicit libxml2 version (libxslt assumed
    /// at its matching current version). Panics on versions newer than the
    /// system oracle to avoid inventing unverifiable epochs.
    pub fn for_libxml2(version: &str) -> CompatibilityProfile {
        assert!(
            parse_version(version) <= parse_version("2.15.3"),
            "no evidence-backed epoch for libxml2 {version}"
        );
        CompatibilityProfile {
            libxml2_version: "2.15.3",
            libxslt_version: "1.1.45",
            capabilities: capabilities_for_libxml2(version),
        }
    }
}

impl fmt::Display for CompatibilityProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "profile(libxml2 {}, libxslt {}, caps={:?})",
            self.libxml2_version, self.libxslt_version, self.capabilities
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boundary table below is the executable encoding of the E-epoch
    /// findings (atlas/SEMANTIC_EPOCHS.md); each assertion is evidence-backed.
    #[test]
    fn e001_xpath_node_set_boundary() {
        assert_eq!(
            capabilities_for_libxml2("2.9.4").xpath_node_set_serialization,
            XPathNodeSetSerialization::Concatenated
        );
        assert_eq!(
            capabilities_for_libxml2("2.9.10").xpath_node_set_serialization,
            XPathNodeSetSerialization::NewlineSeparated
        );
        assert_eq!(
            CompatibilityProfile::current()
                .capabilities
                .xpath_node_set_serialization,
            XPathNodeSetSerialization::NewlineSeparated
        );
    }

    #[test]
    fn e002_parser_diagnostic_window() {
        assert_eq!(
            capabilities_for_libxml2("2.9.4").parser_diagnostic,
            ParserDiagnostic::Dual
        );
        assert_eq!(
            capabilities_for_libxml2("2.9.10").parser_diagnostic,
            ParserDiagnostic::Regression
        );
        assert_eq!(
            capabilities_for_libxml2("2.12.6").parser_diagnostic,
            ParserDiagnostic::Single
        );
    }

    #[test]
    fn e004_entity_compact_boundary() {
        assert_eq!(
            capabilities_for_libxml2("2.12.6").entity_compact_storage,
            EntityCompactStorage::Plain
        );
        assert_eq!(
            capabilities_for_libxml2("2.13.0").entity_compact_storage,
            EntityCompactStorage::Compact
        );
    }

    #[test]
    fn e005_validation_exit_boundary() {
        assert_eq!(
            capabilities_for_libxml2("2.12.6").validation_exit,
            ValidationExit::Legacy
        );
        assert_eq!(
            capabilities_for_libxml2("2.13.0").validation_exit,
            ValidationExit::Reworked
        );
    }

    #[test]
    fn e003_xpath_attr_empty_exit_chain() {
        assert_eq!(
            capabilities_for_libxml2("2.9.14").xpath_attr_empty_exit,
            XpathAttrEmptyExit::Legacy
        );
        assert_eq!(
            capabilities_for_libxml2("2.11.5").xpath_attr_empty_exit,
            XpathAttrEmptyExit::NoError
        );
        assert_eq!(
            capabilities_for_libxml2("2.12.6").xpath_attr_empty_exit,
            XpathAttrEmptyExit::Error11
        );
    }

    #[test]
    fn e006_e007_boundaries() {
        assert_eq!(
            capabilities_for_libxml2("2.14.1").validation_no_dtd_exit,
            ValidationNoDtdExit::Error3
        );
        assert_eq!(
            capabilities_for_libxml2("2.15.0").validation_no_dtd_exit,
            ValidationNoDtdExit::Ok0
        );
        assert_eq!(
            capabilities_for_libxml2("2.14.1").html_serializer,
            HtmlSerializer::Formatted
        );
        assert_eq!(
            capabilities_for_libxml2("2.15.0").html_serializer,
            HtmlSerializer::SingleLine
        );
    }

    #[test]
    fn global_state_init_boundary() {
        assert_eq!(
            capabilities_for_libxml2("2.11.5").global_state_init,
            GlobalStateInit::Eager
        );
        assert_eq!(
            capabilities_for_libxml2("2.12.0").global_state_init,
            GlobalStateInit::Lazy
        );
    }

    #[test]
    fn xslt_transform_stable_epoch() {
        // E-008: byte-identical output 1.1.26 .. 1.1.45.
        assert_eq!(
            capabilities_for_libxml2("2.15.3").xsl_transform,
            XslTransform::Stable
        );
    }

    #[test]
    fn current_profile_resolves_current_epochs() {
        let p = CompatibilityProfile::current();
        assert_eq!(p.libxml2_version, "2.15.3");
        assert_eq!(p.capabilities.parser_diagnostic, ParserDiagnostic::Single);
        assert_eq!(p.capabilities.html_serializer, HtmlSerializer::SingleLine);
        assert_eq!(
            p.capabilities.validation_no_dtd_exit,
            ValidationNoDtdExit::Ok0
        );
    }
}
