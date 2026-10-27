//! The map importer against the reference's own fingerprints of the same blobs.
//!
//! `fixtures/gwdat-pathing.csv` is what `tools/gwdat-pathing.c` -- the reference's own tool, built from
//! its own `GmImportMap` -- prints for the map blobs this repo's catalogue names:
//!
//! ```text
//! file_id,size,planes,trapezoids,min_x,min_y,max_x,max_y
//! ```
//!
//! Plane count, trapezoid count, and the box every trapezoid corner falls in. A port that reads the same
//! bytes but assembles the graph differently gets these wrong: a misplaced field, an off-by-one in a
//! row size, a neighbour index read as a position. `tools/parity-pathing.py --check` re-runs the
//! reference binary and compares all three sides; this test is the half that runs without it.
//!
//! Skipped when `Gw.dat` is not there, the same way the database tests skip without a database.

use asura_world::gwdat::GwDat;
use asura_world::map_data::{PathStaticData, Vec2f};

/// The archive beside the reference checkout, or `ASURA_GW_DAT`.
fn gw_dat_path() -> Option<String> {
    if let Ok(path) = std::env::var("ASURA_GW_DAT") {
        return Some(path);
    }
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    let path = repo.parent()?.join("PreSearing").join("Gw.dat");
    if path.exists() {
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    }
}

/// One row of the fixture: the blob and the reference's eight numbers for it.
struct Fingerprint {
    file_id: u32,
    fields: Vec<String>,
}

fn fixture() -> Vec<Fingerprint> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("gwdat-pathing.csv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("file_id"))
        .map(|line| {
            let fields: Vec<String> = line
                .split(',')
                .map(|field| field.trim().to_string())
                .collect();
            assert_eq!(
                fields.len(),
                8,
                "a fingerprint row has eight fields: {line:?}"
            );
            Fingerprint {
                file_id: u32::from_str_radix(fields[0].trim_start_matches("0x"), 16)
                    .unwrap_or_else(|_| panic!("{line:?} does not start with a file id")),
                fields,
            }
        })
        .collect()
}

/// The same row, computed from this repo's import.
fn fingerprint(file_id: u32, size: usize, data: &PathStaticData) -> Vec<String> {
    let mut trapezoids = 0usize;
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for plane in &data.planes {
        for trapezoid in &plane.trapezoids {
            for x in [trapezoid.xtl, trapezoid.xtr, trapezoid.xbl, trapezoid.xbr] {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
            for y in [trapezoid.yt, trapezoid.yb] {
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
            trapezoids += 1;
        }
    }
    vec![
        format!("{file_id:#010X}"),
        size.to_string(),
        data.planes.len().to_string(),
        trapezoids.to_string(),
        format!("{min_x:.3}"),
        format!("{min_y:.3}"),
        format!("{max_x:.3}"),
        format!("{max_y:.3}"),
    ]
}

#[test]
fn every_catalogue_blob_imports_to_the_references_own_fingerprint() {
    let rows = fixture();
    assert!(
        rows.len() >= 7,
        "the fixture carries every blob the catalogue names: {} rows",
        rows.len()
    );

    let Some(path) = gw_dat_path() else {
        eprintln!("skipped: no Gw.dat beside the reference checkout");
        return;
    };
    let mut archive = GwDat::open(&path).expect("open Gw.dat");

    const NAMES: [&str; 8] = [
        "file_id",
        "size",
        "planes",
        "trapezoids",
        "min_x",
        "min_y",
        "max_x",
        "max_y",
    ];

    for row in &rows {
        let content = archive
            .file(row.file_id)
            .unwrap_or_else(|err| panic!("{:#010X}: {err}", row.file_id))
            .unwrap_or_else(|| panic!("{:#010X}: not in the archive", row.file_id));
        let data = PathStaticData::import(&content)
            .unwrap_or_else(|err| panic!("{:#010X}: {err}", row.file_id));
        let ours = fingerprint(row.file_id, content.len(), &data);

        for ((name, expected), found) in NAMES.iter().zip(&row.fields).zip(&ours) {
            assert_eq!(
                found, expected,
                "{:#010X}: {name} differs from the reference's own import",
                row.file_id
            );
        }
    }
}

#[test]
fn the_settled_graph_of_every_catalogue_map_contains_its_own_spawn_point() {
    // The catalogue's spawn points are `GmMapsConfig.c`'s, and they are what a client is put down at. A
    // graph whose bounding box matches the reference's but whose walk does not reach the spawn point would
    // pass the fingerprint test and fail a client, so this is the second half of the same measurement.
    let Some(path) = gw_dat_path() else {
        eprintln!("skipped: no Gw.dat beside the reference checkout");
        return;
    };
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate is inside the repository");
    let sql = std::fs::read_to_string(repo.join("tools").join("map-config.sql"))
        .expect("the generated map catalogue");

    let mut archive = GwDat::open(&path).expect("open Gw.dat");
    let mut imported: std::collections::HashMap<u32, PathStaticData> =
        std::collections::HashMap::new();
    let mut checked = 0usize;

    for line in sql.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('(') else {
            continue;
        };
        let Some(body) = rest.split(')').next() else {
            continue;
        };
        // The name may hold commas, so only the first seven fields are positional and the rest is text.
        let fields: Vec<&str> = body.splitn(8, ',').collect();
        if fields.len() != 8 {
            continue;
        }
        let map_id: i32 = fields[0].trim().parse().expect("a map id");
        let blob: u32 = fields[2].trim().parse().expect("a blob id");
        let x: f32 = fields[4].trim().parse().expect("a spawn x");
        let y: f32 = fields[5].trim().parse().expect("a spawn y");
        let plane: u16 = fields[6].trim().parse().expect("a spawn plane");

        if let std::collections::hash_map::Entry::Vacant(slot) = imported.entry(blob) {
            let content = archive
                .file(blob)
                .expect("read the blob")
                .expect("the catalogue's blobs are in the archive");
            let data = PathStaticData::import(&content).expect("import the blob");
            slot.insert(data);
        }
        let data = &imported[&blob];
        assert!(
            data.trapezoid_at(Vec2f { x, y }, plane).is_some(),
            "map {map_id} (blob {blob:#010X}): the configured spawn point ({x}, {y}) is not in the graph"
        );
        checked += 1;
    }

    assert!(
        checked >= 12,
        "the catalogue names the reference's maps, and the reference has twelve: {checked}"
    );
}
