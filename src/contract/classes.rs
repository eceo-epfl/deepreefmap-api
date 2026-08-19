//! Benthic class groups and the colour each is drawn in.
//!
//! Derived from the pipeline's `classes_coralscapes.yaml` and the desktop application's
//! `class_groups.yaml`. Published so a cover figure reads the same in every viewer.

/// Taxonomy version the groupings below come from.
pub const TAXONOMY_VERSION: i32 = 1;

/// One benthic class group and the colour every viewer draws it in.
#[derive(Debug, Clone, Copy, serde::Serialize, utoipa::ToSchema)]
pub struct ClassGroup {
    pub level: &'static str,
    pub name: &'static str,
    /// `#rrggbb`, so a browser can use it without conversion.
    pub colour: &'static str,
}

/// Every group at every level, 78 in total.
///
/// A group's colour is the first class colour that rolls up into it, which is how the
/// desktop viewer picks it. Published so the console and the viewer agree.
// One line per group, so the table reads as a table.
#[rustfmt::skip]
pub const CLASS_GROUPS: &[ClassGroup] = &[
    ClassGroup { level: "fine", name: "acropora alive", colour: "#ec80ff" },
    ClassGroup { level: "fine", name: "algae covered substrate", colour: "#7da37d" },
    ClassGroup { level: "fine", name: "anemone", colour: "#00ffbd" },
    ClassGroup { level: "fine", name: "background", colour: "#1da2d8" },
    ClassGroup { level: "fine", name: "branching alive", colour: "#e25b9d" },
    ClassGroup { level: "fine", name: "branching bleached", colour: "#fce7f0" },
    ClassGroup { level: "fine", name: "branching dead", colour: "#7b3256" },
    ClassGroup { level: "fine", name: "clam", colour: "#bdffea" },
    ClassGroup { level: "fine", name: "crown of thorn", colour: "#b3f5ea" },
    ClassGroup { level: "fine", name: "dark", colour: "#1f1f1f" },
    ClassGroup { level: "fine", name: "dead clam", colour: "#599b86" },
    ClassGroup { level: "fine", name: "fish", colour: "#ffff00" },
    ClassGroup { level: "fine", name: "human", colour: "#ff0000" },
    ClassGroup { level: "fine", name: "massive/meandering alive", colour: "#ec9615" },
    ClassGroup { level: "fine", name: "massive/meandering bleached", colour: "#fff8e4" },
    ClassGroup { level: "fine", name: "massive/meandering dead", colour: "#865612" },
    ClassGroup { level: "fine", name: "meandering alive", colour: "#e6c100" },
    ClassGroup { level: "fine", name: "meandering bleached", colour: "#fbf3d8" },
    ClassGroup { level: "fine", name: "meandering dead", colour: "#77640e" },
    ClassGroup { level: "fine", name: "millepora", colour: "#f49673" },
    ClassGroup { level: "fine", name: "other animal", colour: "#00ffff" },
    ClassGroup { level: "fine", name: "other coral alive", colour: "#e07677" },
    ClassGroup { level: "fine", name: "other coral bleached", colour: "#fae0e1" },
    ClassGroup { level: "fine", name: "other coral dead", colour: "#723c3d" },
    ClassGroup { level: "fine", name: "pocillopora alive", colour: "#ff9296" },
    ClassGroup { level: "fine", name: "rubble", colour: "#a19980" },
    ClassGroup { level: "fine", name: "sand", colour: "#c2b280" },
    ClassGroup { level: "fine", name: "sea cucumber", colour: "#00e7ff" },
    ClassGroup { level: "fine", name: "sea urchin", colour: "#008eff" },
    ClassGroup { level: "fine", name: "seagrass", colour: "#7dde7d" },
    ClassGroup { level: "fine", name: "sponge", colour: "#f05050" },
    ClassGroup { level: "fine", name: "stylophora alive", colour: "#ff6fc2" },
    ClassGroup { level: "fine", name: "table acropora alive", colour: "#bd77ff" },
    ClassGroup { level: "fine", name: "table acropora dead", colour: "#553574" },
    ClassGroup { level: "fine", name: "transect line", colour: "#00ff00" },
    ClassGroup { level: "fine", name: "transect tools", colour: "#08cd0c" },
    ClassGroup { level: "fine", name: "trash", colour: "#ff0086" },
    ClassGroup { level: "fine", name: "turbinaria", colour: "#e4ff77" },
    ClassGroup { level: "fine", name: "unknown hard substrate", colour: "#7d7d7d" },
    ClassGroup { level: "intermediate", name: "algae covered substrate", colour: "#7da37d" },
    ClassGroup { level: "intermediate", name: "background", colour: "#1da2d8" },
    ClassGroup { level: "intermediate", name: "branching alive", colour: "#e25b9d" },
    ClassGroup { level: "intermediate", name: "branching bleached", colour: "#fce7f0" },
    ClassGroup { level: "intermediate", name: "branching dead", colour: "#7b3256" },
    ClassGroup { level: "intermediate", name: "clam", colour: "#bdffea" },
    ClassGroup { level: "intermediate", name: "coral alive", colour: "#e07677" },
    ClassGroup { level: "intermediate", name: "coral bleached", colour: "#fae0e1" },
    ClassGroup { level: "intermediate", name: "coral dead", colour: "#723c3d" },
    ClassGroup { level: "intermediate", name: "dark", colour: "#1f1f1f" },
    ClassGroup { level: "intermediate", name: "fish", colour: "#ffff00" },
    ClassGroup { level: "intermediate", name: "hard substrate", colour: "#7d7d7d" },
    ClassGroup { level: "intermediate", name: "human", colour: "#ff0000" },
    ClassGroup { level: "intermediate", name: "massive/meandering alive", colour: "#ec9615" },
    ClassGroup { level: "intermediate", name: "massive/meandering bleached", colour: "#fff8e4" },
    ClassGroup { level: "intermediate", name: "massive/meandering dead", colour: "#865612" },
    ClassGroup { level: "intermediate", name: "millepora", colour: "#f49673" },
    ClassGroup { level: "intermediate", name: "other animal", colour: "#00ffff" },
    ClassGroup { level: "intermediate", name: "rubble", colour: "#a19980" },
    ClassGroup { level: "intermediate", name: "sand", colour: "#c2b280" },
    ClassGroup { level: "intermediate", name: "seagrass", colour: "#7dde7d" },
    ClassGroup { level: "intermediate", name: "transect line", colour: "#00ff00" },
    ClassGroup { level: "intermediate", name: "transect tools", colour: "#08cd0c" },
    ClassGroup { level: "intermediate", name: "trash", colour: "#ff0086" },
    ClassGroup { level: "coarse", name: "algae covered substrate", colour: "#7da37d" },
    ClassGroup { level: "coarse", name: "background", colour: "#1da2d8" },
    ClassGroup { level: "coarse", name: "coral alive", colour: "#e07677" },
    ClassGroup { level: "coarse", name: "coral bleached", colour: "#fae0e1" },
    ClassGroup { level: "coarse", name: "coral dead", colour: "#723c3d" },
    ClassGroup { level: "coarse", name: "dark", colour: "#1f1f1f" },
    ClassGroup { level: "coarse", name: "fish", colour: "#ffff00" },
    ClassGroup { level: "coarse", name: "hard substrate", colour: "#7d7d7d" },
    ClassGroup { level: "coarse", name: "human", colour: "#ff0000" },
    ClassGroup { level: "coarse", name: "other animal", colour: "#00ffff" },
    ClassGroup { level: "coarse", name: "sand", colour: "#c2b280" },
    ClassGroup { level: "coarse", name: "seagrass", colour: "#7dde7d" },
    ClassGroup { level: "coarse", name: "transect line", colour: "#00ff00" },
    ClassGroup { level: "coarse", name: "transect tools", colour: "#08cd0c" },
    ClassGroup { level: "coarse", name: "trash", colour: "#ff0086" },
];

/// The groups at one level, or an empty slice for an unknown level.
#[must_use]
pub fn at_level(level: &str) -> Vec<&'static ClassGroup> {
    CLASS_GROUPS.iter().filter(|g| g.level == level).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::vocab::COVER_LEVEL;

    #[test]
    fn test_every_cover_level_has_groups() {
        for term in COVER_LEVEL.terms {
            assert!(
                !at_level(term.code).is_empty(),
                "no class groups at level {}",
                term.code
            );
        }
    }

    #[test]
    fn test_group_names_are_unique_per_level() {
        for term in COVER_LEVEL.terms {
            let mut seen = std::collections::HashSet::new();
            for group in at_level(term.code) {
                assert!(
                    seen.insert(group.name),
                    "{} repeats {} at {}",
                    term.code,
                    group.name,
                    term.code
                );
            }
        }
    }

    #[test]
    fn test_every_colour_is_a_hex_triplet() {
        for group in CLASS_GROUPS {
            let hex = group.colour.strip_prefix('#').expect("a leading hash");
            assert_eq!(hex.len(), 6, "{} is not #rrggbb", group.colour);
            assert!(
                hex.chars().all(|c| c.is_ascii_hexdigit()),
                "{} is not hexadecimal",
                group.colour
            );
        }
    }

    /// A coarse group is a roll-up of intermediate ones, so it cannot introduce a name
    /// the finer levels have never heard of.
    #[test]
    fn test_coarse_groups_are_drawn_from_the_finer_levels() {
        let finer: std::collections::HashSet<&str> = CLASS_GROUPS
            .iter()
            .filter(|g| g.level != "coarse")
            .map(|g| g.name)
            .collect();
        for group in at_level("coarse") {
            assert!(
                finer.contains(group.name),
                "{} appears only at coarse",
                group.name
            );
        }
    }
}
