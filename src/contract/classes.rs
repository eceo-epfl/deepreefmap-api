//! Benthic classes, the groups they roll up into, and the colour each is drawn in.
//!
//! Derived from the pipeline's `classes_coralscapes.yaml` and the desktop application's
//! `class_groups.yaml`. Published so a cover figure reads the same in every viewer.

use std::sync::LazyLock;

/// Taxonomy version the groupings below come from.
pub const TAXONOMY_VERSION: i32 = 1;

/// One segmentation class: its colour, and the group it rolls into at each level.
///
/// `name` is also the class's own group at the `fine` level.
#[derive(Debug, Clone, Copy, serde::Serialize, utoipa::ToSchema)]
pub struct BenthicClass {
    /// Label id the segmentation model writes, and the value in a run's `ortho.npz`.
    pub id: i32,
    pub name: &'static str,
    /// `#rrggbb`, so a browser can use it without conversion.
    pub colour: &'static str,
    pub intermediate: &'static str,
    pub coarse: &'static str,
}

/// One benthic class group and the colour every viewer draws it in.
#[derive(Debug, Clone, Copy, serde::Serialize, utoipa::ToSchema)]
pub struct ClassGroup {
    pub level: &'static str,
    pub name: &'static str,
    /// `#rrggbb`, so a browser can use it without conversion.
    pub colour: &'static str,
}

/// Every class the taxonomy names, by label id.
// One line per class, so the table reads as a table.
#[rustfmt::skip]
pub const CLASSES: &[BenthicClass] = &[
    BenthicClass { id: 1, name: "seagrass", colour: "#7dde7d", intermediate: "seagrass", coarse: "seagrass" },
    BenthicClass { id: 2, name: "trash", colour: "#ff0086", intermediate: "trash", coarse: "trash" },
    BenthicClass { id: 3, name: "other coral dead", colour: "#723c3d", intermediate: "coral dead", coarse: "coral dead" },
    BenthicClass { id: 4, name: "other coral bleached", colour: "#fae0e1", intermediate: "coral bleached", coarse: "coral bleached" },
    BenthicClass { id: 5, name: "sand", colour: "#c2b280", intermediate: "sand", coarse: "sand" },
    BenthicClass { id: 6, name: "other coral alive", colour: "#e07677", intermediate: "coral alive", coarse: "coral alive" },
    BenthicClass { id: 7, name: "human", colour: "#ff0000", intermediate: "human", coarse: "human" },
    BenthicClass { id: 8, name: "transect tools", colour: "#08cd0c", intermediate: "transect tools", coarse: "transect tools" },
    BenthicClass { id: 9, name: "fish", colour: "#ffff00", intermediate: "fish", coarse: "fish" },
    BenthicClass { id: 10, name: "algae covered substrate", colour: "#7da37d", intermediate: "algae covered substrate", coarse: "algae covered substrate" },
    BenthicClass { id: 11, name: "other animal", colour: "#00ffff", intermediate: "other animal", coarse: "other animal" },
    BenthicClass { id: 12, name: "unknown hard substrate", colour: "#7d7d7d", intermediate: "hard substrate", coarse: "hard substrate" },
    BenthicClass { id: 13, name: "background", colour: "#1da2d8", intermediate: "background", coarse: "background" },
    BenthicClass { id: 14, name: "dark", colour: "#1f1f1f", intermediate: "dark", coarse: "dark" },
    BenthicClass { id: 15, name: "transect line", colour: "#00ff00", intermediate: "transect line", coarse: "transect line" },
    BenthicClass { id: 16, name: "massive/meandering bleached", colour: "#fff8e4", intermediate: "massive/meandering bleached", coarse: "coral bleached" },
    BenthicClass { id: 17, name: "massive/meandering alive", colour: "#ec9615", intermediate: "massive/meandering alive", coarse: "coral alive" },
    BenthicClass { id: 18, name: "rubble", colour: "#a19980", intermediate: "rubble", coarse: "hard substrate" },
    BenthicClass { id: 19, name: "branching bleached", colour: "#fce7f0", intermediate: "branching bleached", coarse: "coral bleached" },
    BenthicClass { id: 20, name: "branching dead", colour: "#7b3256", intermediate: "branching dead", coarse: "coral dead" },
    BenthicClass { id: 21, name: "millepora", colour: "#f49673", intermediate: "millepora", coarse: "coral alive" },
    BenthicClass { id: 22, name: "branching alive", colour: "#e25b9d", intermediate: "branching alive", coarse: "coral alive" },
    BenthicClass { id: 23, name: "massive/meandering dead", colour: "#865612", intermediate: "massive/meandering dead", coarse: "coral dead" },
    BenthicClass { id: 24, name: "clam", colour: "#bdffea", intermediate: "clam", coarse: "other animal" },
    BenthicClass { id: 25, name: "acropora alive", colour: "#ec80ff", intermediate: "coral alive", coarse: "coral alive" },
    BenthicClass { id: 26, name: "sea cucumber", colour: "#00e7ff", intermediate: "other animal", coarse: "other animal" },
    BenthicClass { id: 27, name: "turbinaria", colour: "#e4ff77", intermediate: "coral alive", coarse: "coral alive" },
    BenthicClass { id: 28, name: "table acropora alive", colour: "#bd77ff", intermediate: "coral alive", coarse: "coral alive" },
    BenthicClass { id: 29, name: "sponge", colour: "#f05050", intermediate: "other animal", coarse: "other animal" },
    BenthicClass { id: 30, name: "anemone", colour: "#00ffbd", intermediate: "other animal", coarse: "other animal" },
    BenthicClass { id: 31, name: "pocillopora alive", colour: "#ff9296", intermediate: "coral alive", coarse: "coral alive" },
    BenthicClass { id: 32, name: "table acropora dead", colour: "#553574", intermediate: "coral dead", coarse: "coral dead" },
    BenthicClass { id: 33, name: "meandering bleached", colour: "#fbf3d8", intermediate: "massive/meandering bleached", coarse: "coral bleached" },
    BenthicClass { id: 34, name: "stylophora alive", colour: "#ff6fc2", intermediate: "coral alive", coarse: "coral alive" },
    BenthicClass { id: 35, name: "sea urchin", colour: "#008eff", intermediate: "other animal", coarse: "other animal" },
    BenthicClass { id: 36, name: "meandering alive", colour: "#e6c100", intermediate: "massive/meandering alive", coarse: "coral alive" },
    BenthicClass { id: 37, name: "meandering dead", colour: "#77640e", intermediate: "massive/meandering dead", coarse: "coral dead" },
    BenthicClass { id: 38, name: "crown of thorn", colour: "#b3f5ea", intermediate: "other animal", coarse: "other animal" },
    BenthicClass { id: 39, name: "dead clam", colour: "#599b86", intermediate: "other animal", coarse: "other animal" },];

/// Every group at every level, 78 in total.
///
/// Rolled up from `CLASSES`: a group's colour is the first class colour that falls into
/// it, which is how the desktop viewer picks it. Groups are named alphabetically within
/// a level so the response order is stable.
pub static CLASS_GROUPS: LazyLock<Vec<ClassGroup>> = LazyLock::new(|| {
    let mut groups: Vec<ClassGroup> = Vec::new();
    for level in ["fine", "intermediate", "coarse"] {
        let mut seen: Vec<ClassGroup> = Vec::new();
        for class in CLASSES {
            let name = match level {
                "intermediate" => class.intermediate,
                "coarse" => class.coarse,
                _ => class.name,
            };
            if !seen.iter().any(|group| group.name == name) {
                seen.push(ClassGroup {
                    level,
                    name,
                    colour: class.colour,
                });
            }
        }
        seen.sort_by_key(|group| group.name);
        groups.extend(seen);
    }
    groups
});

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
        for group in CLASS_GROUPS.iter() {
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
    fn test_class_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for class in CLASSES {
            assert!(seen.insert(class.id), "class id {} repeats", class.id);
        }
    }

    /// A group's colour is the first class colour that falls into it.
    #[test]
    fn test_group_colour_comes_from_its_first_class() {
        for group in CLASS_GROUPS.iter() {
            let first = CLASSES
                .iter()
                .find(|class| match group.level {
                    "intermediate" => class.intermediate == group.name,
                    "coarse" => class.coarse == group.name,
                    _ => class.name == group.name,
                })
                .expect("every group comes from a class");
            assert_eq!(
                group.colour, first.colour,
                "{} at {}",
                group.name, group.level
            );
        }
    }

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
