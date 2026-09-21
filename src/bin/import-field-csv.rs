//! Import the field spreadsheets into the registry as console-owned, validated rows.
//!
//! Reads `csv_videos_timestamp_export_*.csv` (one row per clip, comma-joined lists of
//! passes) and `results.csv` (per-transect coordinates), folding free-text places onto
//! sites through `import/site_aliases.json`. Dry run by default: `--apply` writes.
//! Ids are derived from the natural keys, so running it twice changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, Database, Statement, TransactionTrait, Value};
use uuid::Uuid;

use deepreefmap_api::contract::vocab::{self, Vocabulary};

const USAGE: &str = "usage: import-field-csv --videos <csv> [--results <csv>] \
                     [--aliases import/site_aliases.json] [--apply]";
const IMPORTER: &str = "field-csv-import";

struct Args {
    videos: PathBuf,
    results: Option<PathBuf>,
    aliases: PathBuf,
    apply: bool,
}

#[derive(Debug, Clone)]
struct Site {
    id: Uuid,
    name: String,
    country: Option<String>,
    region: Option<String>,
}

#[derive(Debug, Clone)]
struct Campaign {
    id: Uuid,
    name: String,
    begin: Option<NaiveDate>,
    end: Option<NaiveDate>,
}

#[derive(Debug, Clone)]
struct Transect {
    id: Uuid,
    site_id: Option<Uuid>,
    name: String,
    length_m: Option<f64>,
    depth_m: Option<f64>,
    start_depth_m: Option<f64>,
    end_depth_m: Option<f64>,
    start: Option<(f64, f64)>,
    end: Option<(f64, f64)>,
}

#[derive(Debug, Clone)]
struct Video {
    id: Uuid,
    file_name: String,
    camera_label: Option<String>,
    rig_position: Option<&'static str>,
    upside_down: bool,
    review: &'static str,
    notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct Pass {
    id: Uuid,
    transect_id: Option<Uuid>,
    campaign_id: Option<Uuid>,
    video_id: Uuid,
    begin_s: f64,
    end_s: f64,
    direction: Option<&'static str>,
    quality: Option<&'static str>,
    surveyed_on: Option<NaiveDate>,
    label: String,
}

#[derive(Default)]
struct Catalogue {
    sites: BTreeMap<Uuid, Site>,
    campaigns: BTreeMap<Uuid, Campaign>,
    transects: BTreeMap<Uuid, Transect>,
    videos: Vec<Video>,
    passes: Vec<Pass>,
}

#[derive(Default)]
struct Report {
    rows: usize,
    blank_rows: usize,
    placeholder_rows: usize,
    unmapped_places: BTreeMap<String, usize>,
    unmapped_qualities: BTreeMap<String, usize>,
    unmapped_directions: BTreeMap<String, usize>,
    open_ended_windows: usize,
    unreadable_windows: BTreeMap<String, usize>,
    calibration_rows: usize,
    duplicate_paths: usize,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let aliases = match read_aliases(&args.aliases) {
        Ok(aliases) => aliases,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let mut report = Report::default();
    let mut catalogue = Catalogue::default();
    if let Err(message) = read_videos(&args.videos, &aliases, &mut catalogue, &mut report) {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    if let Some(results) = &args.results
        && let Err(message) = read_results(results, &aliases, &mut catalogue, &mut report)
    {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    print_report(&catalogue, &report);
    if !args.apply {
        println!("\nDry run: nothing written. Pass --apply to import.");
        return ExitCode::SUCCESS;
    }
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    match runtime.block_on(write(&catalogue)) {
        Ok(()) => {
            println!("Imported.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Import failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut videos = None;
    let mut results = None;
    let mut aliases = PathBuf::from("import/site_aliases.json");
    let mut apply = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--videos" => videos = args.next().map(PathBuf::from),
            "--results" => results = args.next().map(PathBuf::from),
            "--aliases" => aliases = args.next().map(PathBuf::from).unwrap_or(aliases),
            "--apply" => apply = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        videos: videos.ok_or("--videos is required")?,
        results,
        aliases,
        apply,
    })
}

// --- Aliases ---

#[derive(Clone)]
struct Place {
    country: Option<String>,
    region: Option<String>,
    site: String,
}

type Aliases = BTreeMap<String, Place>;

fn read_aliases(path: &Path) -> Result<Aliases, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut aliases = Aliases::new();
    let Some(places) = document
        .get("places")
        .and_then(serde_json::Value::as_object)
    else {
        return Err(format!("{}: no places", path.display()));
    };
    for (key, entry) in places {
        let text = |name: &str| {
            entry
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        aliases.insert(
            normalise(key),
            Place {
                country: text("country"),
                region: text("region"),
                site: text("site").ok_or_else(|| format!("{key}: no site"))?,
            },
        );
    }
    Ok(aliases)
}

fn normalise(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// --- Identity ---

fn namespace() -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, IMPORTER.as_bytes())
}

fn id_for(kind: &str, key: &str) -> Uuid {
    Uuid::new_v5(&namespace(), format!("{kind}:{key}").as_bytes())
}

// --- The clip spreadsheet ---

/// One spreadsheet row, as text.
struct ClipRow {
    file: String,
    direction: String,
    flip: String,
    windows: String,
    cut: String,
    comment: String,
    place: String,
    quality: String,
    transect_ids: String,
    date: String,
    length: String,
    depth: String,
}

fn read_videos(
    path: &Path,
    aliases: &Aliases,
    catalogue: &mut Catalogue,
    report: &mut Report,
) -> Result<(), String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}').trim().to_lowercase())
        .collect();
    let column = |name: &str| headers.iter().position(|h| h == name);
    let col_file = column("filename").ok_or("no filename column")?;
    let columns: Vec<Option<usize>> = [
        "fw/bw",
        "upside_down",
        "transects",
        "cut",
        "comment",
        "place",
        "transect_quality",
        "transect_id",
        "date",
        "transect length",
        "depth",
    ]
    .iter()
    .map(|name| column(name))
    .collect();

    let mut seen_paths = BTreeSet::new();
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        let cell = |index: Option<usize>| -> String {
            index
                .and_then(|i| record.get(i))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        report.rows += 1;
        let file = cell(Some(col_file));
        if file.is_empty() {
            report.blank_rows += 1;
            continue;
        }
        if file.to_uppercase().starts_with("TODO") {
            report.placeholder_rows += 1;
            continue;
        }
        if !seen_paths.insert(file.clone()) {
            report.duplicate_paths += 1;
        }
        let row = ClipRow {
            file,
            direction: cell(columns[0]),
            flip: cell(columns[1]),
            windows: cell(columns[2]),
            cut: cell(columns[3]),
            comment: cell(columns[4]),
            place: cell(columns[5]),
            quality: cell(columns[6]),
            transect_ids: cell(columns[7]),
            date: cell(columns[8]),
            length: cell(columns[9]),
            depth: cell(columns[10]),
        };
        ingest_clip(&row, aliases, catalogue, report);
    }
    Ok(())
}

/// One clip and the passes cut from it.
fn ingest_clip(row: &ClipRow, aliases: &Aliases, catalogue: &mut Catalogue, report: &mut Report) {
    let parts: Vec<&str> = row.file.split('/').collect();
    let campaign_id = campaign_for(parts[0], &row.date, catalogue);
    let place = normalise(&row.place);
    let site_id = if place.is_empty() {
        None
    } else if let Some(found) = aliases.get(&place) {
        Some(site_for(found, catalogue))
    } else {
        *report.unmapped_places.entry(place.clone()).or_default() += 1;
        None
    };

    let cut = row.cut.to_lowercase();
    let video_id = id_for("video", &row.file);
    let mut notes = Vec::new();
    if !row.comment.is_empty() {
        notes.push(row.comment.clone());
    }
    let calibration = normalise(&row.direction) == "calibration"
        || row.comment.to_lowercase().contains("calibration");
    if calibration {
        report.calibration_rows += 1;
    }
    let pass_count = ingest_passes(
        row,
        site_id,
        campaign_id,
        video_id,
        calibration,
        &mut notes,
        catalogue,
        report,
    );

    let review = if cut == "exclude" {
        "excluded"
    } else if pass_count > 0 {
        "usable"
    } else {
        "unreviewed"
    };
    if !cut.is_empty() && cut != "exclude" {
        notes.push(format!("cut {cut}"));
    }
    catalogue.videos.push(Video {
        id: video_id,
        file_name: parts
            .last()
            .map(|s| (*s).to_string())
            .unwrap_or(row.file.clone()),
        camera_label: camera_label(&parts),
        rig_position: rig_position(&row.comment),
        upside_down: row.flip == "1" || row.file.contains("_flip180"),
        review,
        notes,
    });
}

/// The passes a row's parallel lists describe, returning how many were kept.
#[allow(clippy::too_many_arguments)]
fn ingest_passes(
    row: &ClipRow,
    site_id: Option<Uuid>,
    campaign_id: Option<Uuid>,
    video_id: Uuid,
    calibration: bool,
    notes: &mut Vec<String>,
    catalogue: &mut Catalogue,
    report: &mut Report,
) -> usize {
    let windows = list(&row.windows);
    let transect_ids = list(&row.transect_ids);
    let qualities = list(&row.quality);
    let directions = direction_list(&row.direction, windows.len().max(1), report);
    let lengths = list(&row.length);
    let depth = depth_range(&row.depth);
    let day = parse_day(&row.date);

    let mut kept = 0;
    for (index, window) in windows.iter().enumerate() {
        let transect_name = transect_ids
            .get(index)
            .or_else(|| transect_ids.last())
            .cloned()
            .unwrap_or_default();
        let transect_id = if transect_name.is_empty() {
            None
        } else {
            let length = lengths
                .get(index)
                .or_else(|| lengths.last())
                .and_then(|l| metres(l));
            Some(transect_for(
                &transect_name,
                site_id,
                length,
                depth,
                catalogue,
            ))
        };
        let (begin_s, end_s) = match parse_window(window) {
            Ok((begin, Some(end))) => (begin, end),
            Ok((_, None)) => {
                report.open_ended_windows += 1;
                notes.push(format!(
                    "pass {} window {window} runs to the end of the clip",
                    index + 1
                ));
                continue;
            }
            Err(()) => {
                *report.unreadable_windows.entry(window.clone()).or_default() += 1;
                notes.push(format!(
                    "pass {} window {window} could not be read",
                    index + 1
                ));
                continue;
            }
        };
        if calibration {
            continue;
        }
        let quality = qualities
            .get(index)
            .or_else(|| qualities.last())
            .and_then(|q| code_for(&vocab::PASS_QUALITY, q, &mut report.unmapped_qualities));
        let direction = directions
            .get(index)
            .copied()
            .flatten()
            .filter(|d| *d != "calibration");
        kept += 1;
        catalogue.passes.push(Pass {
            id: id_for("pass", &format!("{}|{index}", row.file)),
            transect_id,
            campaign_id,
            video_id,
            begin_s,
            end_s,
            direction,
            quality,
            surveyed_on: day,
            label: String::new(),
        });
    }
    kept
}

fn campaign_for(name: &str, day: &str, catalogue: &mut Catalogue) -> Option<Uuid> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let id = id_for("campaign", name);
    let day = parse_day(day);
    let entry = catalogue.campaigns.entry(id).or_insert_with(|| Campaign {
        id,
        name: name.to_string(),
        begin: None,
        end: None,
    });
    if let Some(day) = day {
        entry.begin = Some(entry.begin.map_or(day, |b| b.min(day)));
        entry.end = Some(entry.end.map_or(day, |e| e.max(day)));
    }
    Some(id)
}

fn site_for(place: &Place, catalogue: &mut Catalogue) -> Uuid {
    let key = format!(
        "{}|{}",
        place.country.as_deref().unwrap_or("").to_lowercase(),
        place.site.to_lowercase()
    );
    let id = id_for("site", &key);
    catalogue.sites.entry(id).or_insert_with(|| Site {
        id,
        name: place.site.clone(),
        country: place.country.clone(),
        region: place.region.clone(),
    });
    id
}

fn transect_for(
    name: &str,
    site_id: Option<Uuid>,
    length_m: Option<f64>,
    depth: Option<DepthRange>,
    catalogue: &mut Catalogue,
) -> Uuid {
    let key = format!(
        "{}|{}",
        site_id.map(|s| s.to_string()).unwrap_or_default(),
        name.to_lowercase()
    );
    let id = id_for("transect", &key);
    let entry = catalogue.transects.entry(id).or_insert_with(|| Transect {
        id,
        site_id,
        name: name.to_string(),
        length_m: None,
        depth_m: None,
        start_depth_m: None,
        end_depth_m: None,
        start: None,
        end: None,
    });
    if entry.length_m.is_none() {
        entry.length_m = length_m;
    }
    if entry.depth_m.is_none()
        && let Some(depth) = depth
    {
        entry.depth_m = Some(depth.mean());
        entry.start_depth_m = depth.end.map(|_| depth.start);
        entry.end_depth_m = depth.end;
    }
    id
}

/// A depth cell: one reading, or the readings at the two ends as `5-8`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DepthRange {
    start: f64,
    end: Option<f64>,
}

impl DepthRange {
    fn mean(self) -> f64 {
        match self.end {
            Some(end) => f64::midpoint(self.start, end),
            None => self.start,
        }
    }
}

/// `8m` as a single reading; `5-8`, `5 - 8 m`, `5–8` as the depth at each end.
fn depth_range(cell: &str) -> Option<DepthRange> {
    let mut ends = cell.split(['-', '\u{2013}']).map(metres).take(2);
    let start = ends.next().flatten()?;
    let end = ends.next().flatten();
    Some(DepthRange { start, end })
}

/// Comma-separated cells, trimmed, empties dropped.
fn list(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// One direction per pass. A cell like `fw then bw` names two passes in order.
fn direction_list(cell: &str, passes: usize, report: &mut Report) -> Vec<Option<&'static str>> {
    let text = normalise(cell);
    if text.is_empty() {
        return vec![None; passes];
    }
    let words: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|w| !w.is_empty() && *w != "then")
        .collect();
    let mut out: Vec<Option<&'static str>> = Vec::new();
    for word in words {
        if word == "calibration" {
            out.push(Some("calibration"));
            continue;
        }
        out.push(code_for(
            &vocab::PASS_DIRECTION,
            word,
            &mut report.unmapped_directions,
        ));
    }
    if out.len() == 1 {
        return vec![out[0]; passes];
    }
    out.resize(passes, *out.last().unwrap_or(&None));
    out
}

/// A term's code by code, alias, or unknown alias, else counted and None.
fn code_for(
    vocabulary: &Vocabulary,
    raw: &str,
    unmapped: &mut BTreeMap<String, usize>,
) -> Option<&'static str> {
    let text = normalise(raw);
    if text.is_empty() || vocabulary.unknown_aliases.contains(&text.as_str()) {
        return None;
    }
    // A cell naming several terms takes the worst reading, as the vocabulary says.
    let mut worst: Option<usize> = None;
    for part in text.split('/') {
        let part = part.trim();
        let found = vocabulary
            .terms
            .iter()
            .position(|term| term.code == part || term.aliases.contains(&part));
        let Some(index) = found else {
            *unmapped.entry(text.clone()).or_default() += 1;
            return None;
        };
        worst = Some(worst.map_or(index, |w| w.max(index)));
    }
    worst.map(|index| vocabulary.terms[index].code)
}

/// `100m`, `50 m`, `12.5m` as metres: the first number in the cell.
fn metres(cell: &str) -> Option<f64> {
    let trimmed = cell.trim_start_matches(|c: char| !c.is_ascii_digit());
    let number: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    number.parse().ok().filter(|m: &f64| *m > 0.0)
}

/// `begin-01:56` as (0, Some(116)); `04:00-end` as (240, None).
fn parse_window(window: &str) -> Result<(f64, Option<f64>), ()> {
    let (start, stop) = window.split_once('-').ok_or(())?;
    let begin = if start.trim() == "begin" {
        0.0
    } else {
        seconds(start)?
    };
    let end = if stop.trim() == "end" {
        None
    } else {
        Some(seconds(stop)?)
    };
    if let Some(end) = end
        && end <= begin
    {
        return Err(());
    }
    Ok((begin, end))
}

fn seconds(text: &str) -> Result<f64, ()> {
    let parts: Vec<f64> = text
        .trim()
        .split(':')
        .map(|p| p.trim().parse::<f64>().map_err(|_| ()))
        .collect::<Result<_, _>>()?;
    Ok(match parts.as_slice() {
        [s] => *s,
        [m, s] => m * 60.0 + s,
        [h, m, s] => h * 3600.0 + m * 60.0 + s,
        _ => return Err(()),
    })
}

/// `DD.MM.YYYY`, as the field sheets write it.
fn parse_day(cell: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(cell.trim(), "%d.%m.%Y").ok()
}

/// The camera folder, where the path has one: `cam3`, `GoPro_2`, `cam4_hero13`.
fn camera_label(parts: &[&str]) -> Option<String> {
    let parent = parts.iter().rev().nth(1)?;
    let lower = parent.to_lowercase();
    let looks_like_camera = (lower.starts_with("cam") || lower.starts_with("gopro"))
        && lower.chars().any(|c| c.is_ascii_digit());
    looks_like_camera.then(|| (*parent).to_string())
}

fn rig_position(comment: &str) -> Option<&'static str> {
    let words: Vec<String> = comment
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    for word in &words {
        match word.as_str() {
            "center" | "centre" => return Some("centre"),
            "left" => return Some("left"),
            "right" => return Some("right"),
            _ => {}
        }
    }
    None
}

// --- The results sheet: coordinates per transect ---

fn read_results(
    path: &Path,
    aliases: &Aliases,
    catalogue: &mut Catalogue,
    report: &mut Report,
) -> Result<(), String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let column = |name: &str| headers.iter().position(|h| h == name);
    let (Some(col_tid), Some(col_place)) = (column("transect_id"), column("place")) else {
        return Err(format!("{}: needs transect_id and place", path.display()));
    };
    let coordinate = |record: &csv::StringRecord, name: &str| -> Option<f64> {
        column(name)
            .and_then(|i| record.get(i))
            .and_then(|s| s.trim().parse::<f64>().ok())
    };
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        let cell = |index: usize| {
            record
                .get(index)
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let place = normalise(&cell(col_place));
        let Some(found) = aliases.get(&place) else {
            if !place.is_empty() {
                *report.unmapped_places.entry(place).or_default() += 1;
            }
            continue;
        };
        let site_id = site_for(found, catalogue);
        let length = column("length")
            .and_then(|i| record.get(i))
            .and_then(metres);
        let transect_id = transect_for(&cell(col_tid), Some(site_id), length, None, catalogue);
        let start =
            coordinate(&record, "latitude_begin").zip(coordinate(&record, "longitude_begin"));
        let end = coordinate(&record, "latitude_end").zip(coordinate(&record, "longitude_end"));
        if let Some(transect) = catalogue.transects.get_mut(&transect_id) {
            transect.start = transect.start.or(start);
            transect.end = transect.end.or(end);
        }
    }
    Ok(())
}

// --- Report ---

fn print_report(catalogue: &Catalogue, report: &Report) {
    println!(
        "{} rows read: {} blank, {} placeholders, {} duplicate paths, {} calibration",
        report.rows,
        report.blank_rows,
        report.placeholder_rows,
        report.duplicate_paths,
        report.calibration_rows
    );
    println!(
        "{} sites, {} campaigns, {} transects, {} clips, {} passes",
        catalogue.sites.len(),
        catalogue.campaigns.len(),
        catalogue.transects.len(),
        catalogue.videos.len(),
        catalogue.passes.len()
    );
    let with_ends = catalogue
        .transects
        .values()
        .filter(|t| t.start.is_some())
        .count();
    let site_less = catalogue
        .transects
        .values()
        .filter(|t| t.site_id.is_none())
        .count();
    println!("{with_ends} transects with end points, {site_less} without a site");
    println!(
        "{} pass windows run to the end of a clip and were noted on the clip instead",
        report.open_ended_windows
    );
    for (label, table) in [
        ("unmapped places", &report.unmapped_places),
        ("unmapped qualities", &report.unmapped_qualities),
        ("unmapped directions", &report.unmapped_directions),
        ("unreadable windows", &report.unreadable_windows),
    ] {
        if table.is_empty() {
            println!("0 {label}");
            continue;
        }
        println!("{} {label}:", table.len());
        for (value, count) in table {
            println!("  {count:4}  {value:?}");
        }
    }
}

// --- Writing ---

async fn write(catalogue: &Catalogue) -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL")?;
    let db = Database::connect(&url).await?;
    let txn = db.begin().await?;
    write_catalogue(&txn, catalogue).await?;
    write_footage(&txn, catalogue).await?;
    txn.commit().await?;
    Ok(())
}

async fn write_catalogue<C: ConnectionTrait>(
    db: &C,
    catalogue: &Catalogue,
) -> Result<(), sea_orm::DbErr> {
    for site in catalogue.sites.values() {
        exec(
            db,
            "INSERT INTO site (id, name, country, region, description, validated_at, validated_by) \
             VALUES ($1, $2, $3, $4, '', NOW(), $5) ON CONFLICT DO NOTHING",
            vec![
                site.id.into(),
                site.name.clone().into(),
                Value::String(site.country.clone()),
                Value::String(site.region.clone()),
                IMPORTER.into(),
            ],
        )
        .await?;
    }
    for campaign in catalogue.campaigns.values() {
        exec(
            db,
            "INSERT INTO campaign (id, name, begin_date, end_date, description, validated_at, validated_by) \
             VALUES ($1, $2, $3, $4, '', NOW(), $5) ON CONFLICT DO NOTHING",
            vec![
                campaign.id.into(),
                campaign.name.clone().into(),
                Value::ChronoDate(campaign.begin),
                Value::ChronoDate(campaign.end),
                IMPORTER.into(),
            ],
        )
        .await?;
    }
    for transect in catalogue.transects.values() {
        exec(
            db,
            "INSERT INTO transect (id, site_id, name, description, start_lat, start_lon, end_lat, end_lon, \
             length_m, depth_m, start_depth_m, end_depth_m, validated_at, validated_by) \
             VALUES ($1, $2, $3, '', $4, $5, $6, $7, $8, $9, $10, $11, NOW(), $12) ON CONFLICT DO NOTHING",
            vec![
                transect.id.into(),
                Value::Uuid(transect.site_id),
                transect.name.clone().into(),
                Value::Double(transect.start.map(|p| p.0)),
                Value::Double(transect.start.map(|p| p.1)),
                Value::Double(transect.end.map(|p| p.0)),
                Value::Double(transect.end.map(|p| p.1)),
                Value::Double(transect.length_m),
                Value::Double(transect.depth_m),
                Value::Double(transect.start_depth_m),
                Value::Double(transect.end_depth_m),
                IMPORTER.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn write_footage<C: ConnectionTrait>(
    db: &C,
    catalogue: &Catalogue,
) -> Result<(), sea_orm::DbErr> {
    for video in &catalogue.videos {
        exec(
            db,
            "INSERT INTO video_asset (id, file_name, gravity, gps, camera_label, rig_position, upside_down, \
             review, notes, validated_at, validated_by) \
             VALUES ($1, $2, 'unknown', 'unknown', $3, $4, $5, $6, $7, NOW(), $8) ON CONFLICT DO NOTHING",
            vec![
                video.id.into(),
                video.file_name.clone().into(),
                Value::String(video.camera_label.clone()),
                Value::String(video.rig_position.map(str::to_string)),
                video.upside_down.into(),
                video.review.into(),
                video.notes.join("\n").into(),
                IMPORTER.into(),
            ],
        )
        .await?;
    }
    for pass in &catalogue.passes {
        exec(
            db,
            "INSERT INTO transect_pass (id, transect_id, campaign_id, begin_s, end_s, direction, label, notes, \
             quality, surveyed_on, validated_at, validated_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, '', $8, $9, NOW(), $10) ON CONFLICT DO NOTHING",
            vec![
                pass.id.into(),
                Value::Uuid(pass.transect_id),
                Value::Uuid(pass.campaign_id),
                pass.begin_s.into(),
                pass.end_s.into(),
                Value::String(pass.direction.map(str::to_string)),
                pass.label.clone().into(),
                Value::String(pass.quality.map(str::to_string)),
                Value::ChronoDate(pass.surveyed_on),
                IMPORTER.into(),
            ],
        )
        .await?;
        exec(
            db,
            "INSERT INTO pass_video (id, pass_id, video_id, ordinal, validated_at, validated_by) \
             VALUES ($1, $2, $3, 0, NOW(), $4) ON CONFLICT DO NOTHING",
            vec![
                id_for("pass_video", &pass.id.to_string()).into(),
                pass.id.into(),
                pass.video_id.into(),
                IMPORTER.into(),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn exec<C: ConnectionTrait>(
    db: &C,
    sql: &str,
    values: Vec<Value>,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        sql,
        values,
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DepthRange, depth_range, metres};

    #[test]
    fn test_metres_reads_the_first_number() {
        assert_eq!(metres("100m"), Some(100.0));
        assert_eq!(metres("50 m"), Some(50.0));
        assert_eq!(metres("12.5"), Some(12.5));
        assert_eq!(metres("~8 m"), Some(8.0));
        assert_eq!(metres(""), None);
        assert_eq!(metres("0"), None);
        assert_eq!(metres("n/a"), None);
    }

    #[test]
    fn test_depth_range_single_reading() {
        let depth = depth_range("8 m").expect("a reading");
        assert_eq!(
            depth,
            DepthRange {
                start: 8.0,
                end: None
            }
        );
        assert!((depth.mean() - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_depth_range_at_each_end() {
        let depth = depth_range("5-8").expect("a range");
        assert_eq!(
            depth,
            DepthRange {
                start: 5.0,
                end: Some(8.0)
            }
        );
        assert!((depth.mean() - 6.5).abs() < f64::EPSILON); // (5 + 8) / 2
        assert_eq!(depth_range("5 - 8 m"), Some(depth));
        assert_eq!(depth_range("5\u{2013}8"), Some(depth));
    }

    #[test]
    fn test_depth_range_empty() {
        assert_eq!(depth_range(""), None);
        assert_eq!(depth_range("-"), None);
    }
}
