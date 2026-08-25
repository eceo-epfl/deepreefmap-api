//! What a run's output file is for, read off its path in the run directory.
//!
//! Root files fall into three purposes; anything in a subdirectory belongs to that
//! directory. The console groups the outputs tab by the same rule
//! (`deepreefmap-ui/src/archive/purpose.ts`), so a bundle download packs exactly the
//! group the reader asked for.

/// The products of the run: the ortho, the cover figures, the clouds.
pub const RESULTS: &str = "Results";
/// What the run was: its manifest, log and command line.
pub const RECORD: &str = "Record";
/// Root files that are neither, kept because the run wrote them.
pub const WORKING: &str = "Working data";
/// Every archived file, the value the download route takes for the whole run.
pub const EVERYTHING: &str = "all";

const RECORD_FILES: &[&str] = &["run_manifest.json", "run.log", "run_command.sh"];

// The names come from the pipeline, which writes them lowercase, and the console
// matches them the same way.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn is_result(name: &str) -> bool {
    name == "ortho.png"
        || (name.starts_with("ortho") && name.ends_with(".npz"))
        || name == "benthic_cover.json"
        || name.ends_with(".ply")
        || name.ends_with(".scene.zarr.zip")
        || name == "cloud_web.drmw"
}

/// The group a run-directory path belongs to.
#[must_use]
pub fn purpose_of(relpath: &str) -> &str {
    if let Some((directory, _)) = relpath.split_once('/') {
        return directory;
    }
    if is_result(relpath) {
        return RESULTS;
    }
    if RECORD_FILES.contains(&relpath) {
        return RECORD;
    }
    WORKING
}

/// Whether a path belongs in the bundle asked for. `EVERYTHING` takes them all.
#[must_use]
pub fn selects(purpose: &str, relpath: &str) -> bool {
    purpose == EVERYTHING || purpose_of(relpath) == purpose
}

/// A purpose as a filename part: lowercase, with runs of other characters as dashes.
#[must_use]
pub fn slug(purpose: &str) -> String {
    let mut out = String::with_capacity(purpose.len());
    for character in purpose.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_files_fall_into_three_purposes() {
        assert_eq!(purpose_of("ortho.png"), RESULTS);
        assert_eq!(purpose_of("ortho.npz"), RESULTS);
        assert_eq!(purpose_of("benthic_cover.json"), RESULTS);
        assert_eq!(purpose_of("semantic_reference_cloud.ply"), RESULTS);
        assert_eq!(purpose_of("cloud_web.drmw"), RESULTS);
        assert_eq!(purpose_of("scene_v9.scene.zarr.zip"), RESULTS);
        assert_eq!(purpose_of("run_manifest.json"), RECORD);
        assert_eq!(purpose_of("run.log"), RECORD);
        assert_eq!(purpose_of("mapping_outputs.npz"), WORKING);
    }

    #[test]
    fn test_a_subdirectory_is_its_own_group() {
        assert_eq!(purpose_of("frames/000001.png"), "frames");
        assert_eq!(purpose_of("labels/000001.png"), "labels");
    }

    #[test]
    fn test_everything_selects_every_path() {
        assert!(selects(EVERYTHING, "frames/000001.png"));
        assert!(selects("frames", "frames/000001.png"));
        assert!(!selects("labels", "frames/000001.png"));
        assert!(selects(RESULTS, "ortho.png"));
    }

    #[test]
    fn test_a_slug_is_filename_safe() {
        assert_eq!(slug("Working data"), "working-data");
        assert_eq!(slug("Results"), "results");
        assert_eq!(slug("frames"), "frames");
    }
}
