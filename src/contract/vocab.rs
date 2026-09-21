//! Every controlled vocabulary in the registry, with the legacy field spellings that
//! fold into it.
//!
//! Quality and direction hang off `transect_pass`, not `video_asset`, because the field
//! spreadsheets record them per swim ('bw then fw', 'excellent,good') inside one clip.
//!
//! `cover_row.level` is a vocabulary but `cover_row.class_group` is not: group names are
//! taxonomy data, versioned by `run_record.taxonomy_version`/`taxonomy_hash`.

use serde::Serialize;

/// One term: the wire code, how it reads, what it means, and its legacy spellings.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Term {
    pub code: &'static str,
    pub label: &'static str,
    pub definition: &'static str,
    /// Free-text values a legacy importer folds into `code`, already normalised.
    pub aliases: &'static [&'static str],
}

/// What a legacy value listing several terms means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiValue {
    /// Terms are ranked worst-last, so an ambiguous cell takes the worst reading.
    Worst,
    /// The cell describes several passes and the importer must split the row.
    Split,
}

/// One vocabulary: its terms, the columns it constrains, and how it degrades.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Vocabulary {
    pub name: &'static str,
    /// `table.column` for every column this vocabulary constrains.
    pub columns: &'static [&'static str],
    /// Whether the column may be null, which is always a distinct meaning from any code.
    pub nullable: bool,
    pub multi_value: MultiValue,
    /// Legacy spellings that mean "not recorded", so the column stays null.
    pub unknown_aliases: &'static [&'static str],
    /// The code an unreadable value takes where null is not a state, so an importer
    /// always has something legal to write.
    pub unknown_code: Option<&'static str>,
    pub terms: &'static [Term],
}

impl Vocabulary {
    /// The codes in declaration order.
    #[must_use]
    pub fn codes(&self) -> Vec<&'static str> {
        self.terms.iter().map(|t| t.code).collect()
    }
}

/// A legacy value that is not a survey observation at all.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Exclusion {
    pub raw: &'static str,
    pub reason: &'static str,
}

/// Values that disqualify the whole spreadsheet row instead of mapping to a code.
pub const LEGACY_EXCLUSIONS: &[Exclusion] = &[
    Exclusion {
        raw: "calibration",
        reason: "camera calibration footage, so there is no pass to record",
    },
    Exclusion {
        raw: "exclude",
        reason: "marked unusable in the field, so the footage is not surveyed",
    },
];

/// Diver's assessment of a pass. Null means not assessed, which no code stands for.
///
/// Ranked best to worst, and an ambiguous cell ('good/meh') takes the worse reading so
/// aggregate cover is never flattered by a guess.
pub const PASS_QUALITY: Vocabulary = Vocabulary {
    name: "pass_quality",
    columns: &["transect_pass.quality"],
    nullable: true,
    multi_value: MultiValue::Worst,
    unknown_aliases: &["undefined", "unknown", "n/a", "?"],
    unknown_code: None,
    terms: &[
        Term {
            code: "excellent",
            label: "Excellent",
            definition: "Clear water, steady swim, the transect line visible throughout.",
            aliases: &["excelent"],
        },
        Term {
            code: "very_good",
            label: "Very good",
            definition: "Minor turbidity or camera motion, nothing that costs coverage.",
            aliases: &["very good", "verygood"],
        },
        Term {
            code: "good",
            label: "Good",
            definition: "Usable throughout, with visible turbidity or uneven pacing.",
            aliases: &[],
        },
        Term {
            code: "meh",
            label: "Mediocre",
            definition: "Reconstructs, but parts of the swim are too poor to trust.",
            aliases: &["mediocre", "ok", "okay", "average"],
        },
        Term {
            code: "bad",
            label: "Bad",
            definition: "Largely unusable: heavy turbidity, surge, or lost line.",
            aliases: &[],
        },
        Term {
            code: "very_bad",
            label: "Very bad",
            definition: "No usable reconstruction is expected from this swim.",
            aliases: &["very bad", "verybad", "terrible", "extremely bad"],
        },
    ],
};

/// Which way along the tape the diver swam.
///
/// 'bw then fw' splits into two passes rather than resolving, and blank or 'undefined'
/// leaves the direction unrecorded.
pub const PASS_DIRECTION: Vocabulary = Vocabulary {
    name: "pass_direction",
    columns: &["transect_pass.direction"],
    nullable: true,
    multi_value: MultiValue::Split,
    unknown_aliases: &["undefined", "unknown", "n/a", "?"],
    unknown_code: None,
    terms: &[
        Term {
            code: "forward",
            label: "Forward",
            definition: "Swum from the transect's start point towards its end point.",
            aliases: &["fw", "fwd", "f", "out"],
        },
        Term {
            code: "reverse",
            label: "Reverse",
            definition: "Swum from the transect's end point back to its start point.",
            aliases: &["bw", "bwd", "b", "backward", "backwards", "back", "return"],
        },
    ],
};

/// Whether a telemetry stream is present in a clip.
///
/// Three states because a camera that recorded none differs from a file never read. Null
/// is not among them: a blank cell is `unknown`, which the column can hold.
pub const TELEMETRY_TRISTATE: Vocabulary = Vocabulary {
    name: "telemetry_tristate",
    columns: &["video_asset.gravity", "video_asset.gps"],
    nullable: false,
    multi_value: MultiValue::Split,
    unknown_aliases: &[],
    unknown_code: Some("unknown"),
    terms: &[
        Term {
            code: "yes",
            label: "Yes",
            definition: "The stream is present in the clip's telemetry.",
            aliases: &["y", "true", "t", "1", "present"],
        },
        Term {
            code: "no",
            label: "No",
            definition: "The clip was read and carries no such stream.",
            aliases: &["n", "false", "f", "0", "absent"],
        },
        Term {
            code: "unknown",
            label: "Unknown",
            definition: "The clip's telemetry has not been read.",
            aliases: &["unread", "?", "n/a"],
        },
    ],
};

/// Where a clip's `captured_at` came from, since a container stamp and an mtime differ
/// in how far they can be trusted.
pub const CAPTURE_SOURCE: Vocabulary = Vocabulary {
    name: "capture_source",
    columns: &["video_asset.captured_source"],
    nullable: true,
    multi_value: MultiValue::Split,
    unknown_aliases: &["unknown", "n/a"],
    unknown_code: None,
    terms: &[
        Term {
            code: "container",
            label: "Container metadata",
            definition: "Read from the video container's own creation stamp.",
            aliases: &["metadata", "embedded"],
        },
        Term {
            code: "mtime",
            label: "File modification time",
            definition: "Taken from the file's modification time, so it is an estimate.",
            aliases: &["file", "filesystem", "modified"],
        },
    ],
};

/// State of a reconstruction the desktop application reports.
///
/// The interface's `queued` and `incomplete` are absent: they describe rows that do not
/// exist yet, so no stored run can carry them.
pub const RUN_STATUS: Vocabulary = Vocabulary {
    name: "run_status",
    columns: &["run_record.status"],
    nullable: false,
    multi_value: MultiValue::Split,
    unknown_aliases: &[],
    unknown_code: None,
    terms: &[
        Term {
            code: "pending",
            label: "Pending",
            definition: "Recorded but not started.",
            aliases: &["queued", "waiting"],
        },
        Term {
            code: "running",
            label: "Running",
            definition: "In flight on the producing device.",
            aliases: &["in_progress", "processing"],
        },
        Term {
            code: "succeeded",
            label: "Succeeded",
            definition: "Finished and wrote its outputs.",
            aliases: &["success", "done", "complete", "completed"],
        },
        Term {
            code: "failed",
            label: "Failed",
            definition: "Stopped on an error, which `error` carries.",
            aliases: &["failure", "error"],
        },
        Term {
            code: "cancelled",
            label: "Cancelled",
            definition: "Stopped on purpose, so nothing is owed.",
            aliases: &["canceled", "aborted"],
        },
        Term {
            code: "interrupted",
            label: "Interrupted",
            definition: "The process stopped mid-run, so it is worth redoing.",
            aliases: &["crashed", "incomplete"],
        },
    ],
};

/// Level of the class hierarchy a cover row sits at. `fine` is the segmentation class
/// itself, the coarser two come from the taxonomy roll-up.
pub const COVER_LEVEL: Vocabulary = Vocabulary {
    name: "cover_level",
    columns: &["cover_row.level"],
    nullable: false,
    multi_value: MultiValue::Split,
    unknown_aliases: &[],
    unknown_code: None,
    terms: &[
        Term {
            code: "fine",
            label: "Fine",
            definition: "One row per segmentation class, named as the class list names it.",
            aliases: &["class", "classes", "raw"],
        },
        Term {
            code: "intermediate",
            label: "Intermediate",
            definition: "Classes rolled up to the taxonomy's intermediate groups.",
            aliases: &["mid", "medium", "group"],
        },
        Term {
            code: "coarse",
            label: "Coarse",
            definition: "Classes rolled up to the taxonomy's broadest groups.",
            aliases: &["broad", "top"],
        },
    ],
};

/// How a cover fraction was estimated across the passes of a transect.
pub const COVER_ESTIMATOR: Vocabulary = Vocabulary {
    name: "cover_estimator",
    columns: &["cover_row.estimator"],
    nullable: false,
    multi_value: MultiValue::Split,
    unknown_aliases: &[],
    unknown_code: None,
    terms: &[
        Term {
            code: "per_pass",
            label: "Per pass",
            definition: "Measured on one pass alone.",
            aliases: &["per pass", "pass", "single", "single_pass"],
        },
        Term {
            code: "pooled",
            label: "Pooled",
            definition: "Count-weighted estimate over every contributing pass.",
            aliases: &["pool", "combined", "transect"],
        },
    ],
};

/// Which point cloud the fractions were measured on: the choice changes their meaning.
pub const COVER_METRIC_SOURCE: Vocabulary = Vocabulary {
    name: "cover_metric_source",
    columns: &["cover_row.metric_source"],
    nullable: true,
    multi_value: MultiValue::Split,
    unknown_aliases: &["unknown", "n/a"],
    unknown_code: None,
    terms: &[
        Term {
            code: "unprojected",
            label: "Unprojected cloud",
            definition: "Depth unprojected per frame, the pipeline's default cloud.",
            aliases: &["unprojection", "raw", "default"],
        },
        Term {
            code: "tsdf",
            label: "TSDF cloud",
            definition: "Fused TSDF cloud, produced only when fusion was enabled.",
            aliases: &["tsdf_cloud", "fused", "fusion"],
        },
    ],
};

/// Where a camera sat on the rig relative to the diver. Null means not recorded.
pub const RIG_POSITION: Vocabulary = Vocabulary {
    name: "rig_position",
    columns: &["video_asset.rig_position"],
    nullable: true,
    multi_value: MultiValue::Split,
    unknown_aliases: &["unknown", "n/a", "?"],
    unknown_code: None,
    terms: &[
        Term {
            code: "left",
            label: "Left",
            definition: "To the diver's left.",
            aliases: &["l", "left of diver"],
        },
        Term {
            code: "centre",
            label: "Centre",
            definition: "Directly ahead of the diver.",
            aliases: &["center", "c", "middle", "front"],
        },
        Term {
            code: "right",
            label: "Right",
            definition: "To the diver's right.",
            aliases: &["r", "right of diver"],
        },
    ],
};

/// Whether a clip is fit to survey from. A clip nobody has looked at is `unreviewed`,
/// which is not the same as one looked at and rejected.
pub const VIDEO_REVIEW: Vocabulary = Vocabulary {
    name: "video_review",
    columns: &["video_asset.review"],
    nullable: false,
    multi_value: MultiValue::Split,
    unknown_aliases: &[],
    unknown_code: Some("unreviewed"),
    terms: &[
        Term {
            code: "unreviewed",
            label: "Unreviewed",
            definition: "Nobody has judged the footage yet.",
            aliases: &["pending", "todo"],
        },
        Term {
            code: "usable",
            label: "Usable",
            definition: "Footage a pass can be cut from.",
            aliases: &["ok", "good", "keep"],
        },
        Term {
            code: "excluded",
            label: "Excluded",
            definition: "Footage judged unusable, or a duplicate of another clip.",
            aliases: &["exclude", "duplicate", "reject", "rejected", "unusable"],
        },
    ],
};

/// Every vocabulary, so callers can render or check them all without a list of their own.
pub const VOCABULARIES: &[&Vocabulary] = &[
    &PASS_QUALITY,
    &PASS_DIRECTION,
    &TELEMETRY_TRISTATE,
    &CAPTURE_SOURCE,
    &RIG_POSITION,
    &VIDEO_REVIEW,
    &RUN_STATUS,
    &COVER_LEVEL,
    &COVER_ESTIMATOR,
    &COVER_METRIC_SOURCE,
];

/// The vocabulary constraining `table.column`.
#[must_use]
pub fn vocabulary_for_column(column: &str) -> Option<&'static Vocabulary> {
    VOCABULARIES
        .iter()
        .copied()
        .find(|v| v.columns.contains(&column))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_normalised(value: &str) -> bool {
        value == value.trim() && value == value.to_lowercase() && !value.contains("  ")
    }

    #[test]
    fn test_codes_are_unique_per_vocabulary() {
        for vocab in VOCABULARIES {
            let mut seen = std::collections::HashSet::new();
            for code in vocab.codes() {
                assert!(seen.insert(code), "{} repeats {code}", vocab.name);
            }
        }
    }

    #[test]
    fn test_no_alias_collides_with_code_or_alias() {
        for vocab in VOCABULARIES {
            let codes = vocab.codes();
            let mut seen = std::collections::HashSet::new();
            for term in vocab.terms {
                for alias in term.aliases {
                    // Published for an importer to match against, so a padded or
                    // capitalised alias would never match the value it is meant to catch.
                    assert!(
                        is_normalised(alias),
                        "{}: {alias} is not normalised",
                        vocab.name
                    );
                    assert!(
                        !codes.contains(alias),
                        "{}: alias {alias} is also a code",
                        vocab.name
                    );
                    assert!(seen.insert(*alias), "{}: alias {alias} repeats", vocab.name);
                }
            }
            for unknown in vocab.unknown_aliases {
                assert!(
                    !codes.contains(unknown) && !seen.contains(unknown),
                    "{}: {unknown} is both unknown and a term",
                    vocab.name
                );
            }
        }
    }

    #[test]
    fn test_unknown_code_is_a_term_of_a_not_null_vocabulary() {
        for vocab in VOCABULARIES {
            if let Some(code) = vocab.unknown_code {
                assert!(
                    vocab.codes().contains(&code),
                    "{}: unknown_code {code} is not a term",
                    vocab.name
                );
                assert!(
                    !vocab.nullable,
                    "{}: a nullable column says 'not recorded' with null",
                    vocab.name
                );
            }
        }
    }

    #[test]
    fn test_lookup_by_column() {
        assert_eq!(
            vocabulary_for_column("video_asset.gps").map(|v| v.name),
            Some("telemetry_tristate")
        );
        assert!(vocabulary_for_column("site.place").is_none());
    }

    #[test]
    fn test_every_vocabulary_column_is_qualified_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for vocab in VOCABULARIES {
            for column in vocab.columns {
                assert!(column.contains('.'), "{column} is not table.column");
                assert!(seen.insert(*column), "{column} is claimed twice");
            }
        }
    }
}
