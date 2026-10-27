//! The path finder against the reference's own answers, case by case.
//!
//! `fixtures/path-cases.txt` is the list of walks both sides run (`tools/gen-path-cases.py`, from the
//! reference's map catalogue), and `fixtures/path-results.csv` is what the reference's own `PathFinding`
//! said about each one (`tools/path-oracle.c`, compiled from the reference's sources and run by
//! `tools/parity-path.py --write`).
//!
//! The comparison is exact: the same `result`, the same number of waypoints, and the same waypoints down
//! to the raw bits of each coordinate. That is the only honest way to compare an A* -- an implementation
//! that finds "a" path agrees with nobody, and one that takes a different corridor in a corridor matters.
//!
//! One case is compared on its answer alone: the reference reads `t` and `s` uninitialised when a step is
//! parallel to the line being cut against, so it answers that walk with two different waypoint counts on
//! different runs. There is nothing to pin; the fixture lists it in its header.
//!
//! Skipped when `Gw.dat` is not there, the same way the database tests skip without a database.

use std::collections::HashMap;

use asura_world::gwdat::GwDat;
use asura_world::map_data::PathStaticData;
use asura_world::pathfind::{PathContext, Pos, Waypoint};

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

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// The answer a case produced, in the reference's own CSV shape.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Answer {
    /// The seven fields that name the case.
    case: String,
    result: u8,
    waypoints: String,
}

impl Answer {
    fn line(&self, count: usize) -> String {
        format!("{},{},{},{}", self.case, self.result, count, self.waypoints)
    }
}

fn parse_answer(line: &str) -> Answer {
    let fields: Vec<&str> = line.split(',').collect();
    assert!(
        fields.len() >= 9,
        "a result row has at least nine fields: {line:?}"
    );
    Answer {
        case: fields[..7].join(","),
        result: fields[7].parse().expect("a result flag"),
        waypoints: fields[9..].join(","),
    }
}

/// The row this repo's finder produces for one case, in the fixture's exact spelling.
fn answer_for(
    data: &PathStaticData,
    waypoints: &mut Vec<Waypoint>,
    src: Pos,
    dst: Pos,
    case: &str,
) -> (Answer, usize, bool) {
    waypoints.clear();
    let mut finder = PathContext::new(data);
    let found = finder.path_finding(src, dst, waypoints);

    let joined = waypoints
        .iter()
        .map(|waypoint| {
            format!(
                "{},{:08X},{:08X},{}",
                waypoint.trap_id,
                waypoint.pos.x.to_bits(),
                waypoint.pos.y.to_bits(),
                waypoint.pos.plane
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    (
        Answer {
            case: case.to_string(),
            result: u8::from(found),
            waypoints: joined,
        },
        waypoints.len(),
        found,
    )
}

/// One line of `path-cases.txt`.
struct Case {
    blob: u32,
    src: Pos,
    dst: Pos,
    key: String,
}

fn cases() -> Vec<Case> {
    let text = fixture("path-cases.txt");
    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 7, "a case has seven fields: {line:?}");
        let blob = u32::from_str_radix(fields[0].trim_start_matches("0x"), 16)
            .unwrap_or_else(|_| panic!("{line:?} does not start with a file id"));
        let src = Pos {
            x: fields[1].parse().expect("a source x"),
            y: fields[2].parse().expect("a source y"),
            plane: fields[3].parse().expect("a source plane"),
        };
        let dst = Pos {
            x: fields[4].parse().expect("a destination x"),
            y: fields[5].parse().expect("a destination y"),
            plane: fields[6].parse().expect("a destination plane"),
        };
        // The key is the fixture's spelling of the same seven fields, as bit patterns.
        let key = format!(
            "{blob:#010X},{:08X},{:08X},{},{:08X},{:08X},{}",
            src.x.to_bits(),
            src.y.to_bits(),
            src.plane,
            dst.x.to_bits(),
            dst.y.to_bits(),
            dst.plane
        );
        cases.push(Case {
            blob,
            src,
            dst,
            key,
        });
    }
    cases
}

#[test]
fn every_case_produces_the_references_own_waypoints() {
    let Some(path) = gw_dat_path() else {
        eprintln!("skipped: no Gw.dat beside the reference checkout");
        return;
    };
    let text = fixture("path-results.csv");

    let mut expected: HashMap<String, String> = HashMap::new();
    let mut unstable: HashMap<String, u8> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# unstable,") {
            let fields: Vec<&str> = rest.split(',').collect();
            unstable.insert(
                fields[..7].join(","),
                fields[7].parse().expect("the agreed result flag"),
            );
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with("blob,") {
            continue;
        }
        expected.insert(parse_answer(line).case.clone(), line.to_string());
    }
    assert!(
        expected.len() >= 90,
        "the fixture carries the reference's answers: {} rows",
        expected.len()
    );

    let cases = cases();
    assert!(
        cases.len() >= 90,
        "the case list is the catalogue's spawn points: {} cases",
        cases.len()
    );

    let mut archive = GwDat::open(&path).expect("open Gw.dat");
    let mut imported: HashMap<u32, PathStaticData> = HashMap::new();
    let mut waypoints: Vec<Waypoint> = Vec::new();
    let mut checked = 0usize;
    let mut found_walks = 0usize;

    for case in &cases {
        if let std::collections::hash_map::Entry::Vacant(slot) = imported.entry(case.blob) {
            let content = archive
                .file(case.blob)
                .unwrap_or_else(|err| panic!("{:#010X}: {err}", case.blob))
                .unwrap_or_else(|| panic!("{:#010X} is not in the archive", case.blob));
            let data = PathStaticData::import(&content)
                .unwrap_or_else(|err| panic!("{:#010X}: {err}", case.blob));
            slot.insert(data);
        }
        let data = &imported[&case.blob];
        let (answer, count, found) =
            answer_for(data, &mut waypoints, case.src, case.dst, &case.key);
        if found {
            found_walks += 1;
        }

        if let Some(agreed) = unstable.get(&case.key) {
            assert_eq!(
                answer.result, *agreed,
                "{}: the reference has no single waypoint list here, but it agrees on whether a walk exists",
                case.key
            );
            checked += 1;
            continue;
        }

        let reference = expected
            .get(&case.key)
            .unwrap_or_else(|| panic!("{}: the fixture has no row for this case", case.key));
        assert_eq!(
            answer.line(count),
            *reference,
            "{}: the waypoints differ from the reference's own path finder",
            case.key
        );
        checked += 1;
    }

    assert_eq!(checked, cases.len(), "every case is checked");
    assert!(
        found_walks > 30,
        "the cases are meant to include walks that exist: {found_walks}"
    );
}

#[test]
fn the_fixed_point_helpers_are_the_games_own_tables() {
    // `PathReciproc` and `PathSqrt` are table lookups, not the FPU, so they are only right if the tables
    // and the integer arithmetic around them are right. These are the values from the reference's own
    // tables, checked against the real numbers to within the tables' 8-bit mantissa: a wrong table, a
    // wrong shift or a byte-swapped constant misses by far more than that.
    use asura_world::pathfind::{path_reciproc, path_sqrt};

    for value in [1.0f32, 2.0, 4.0, 10.0, 263.0, 1000.0] {
        let expected = 1.0 / value;
        let found = path_reciproc(value);
        assert!(
            (found - expected).abs() / expected < 0.01,
            "PathReciproc({value}) = {found}, and 1/{value} = {expected}"
        );
    }

    for value in [1.0f32, 4.0, 100.0, 2500.0, 65_536.0] {
        let expected = value.sqrt();
        let found = path_sqrt(value);
        assert!(
            (found - expected).abs() / expected < 0.01,
            "PathSqrt({value}) = {found}, and sqrt({value}) = {expected}"
        );
    }

    // The tables are only meant for positive inputs: `PathReciproc` masks off the sign bit and adds a
    // constant, so a negative input produces nonsense. The reference calls it that way anyway
    // (`PathReciproc(dst_pos.y - src_pos.y)`), and this port reproduces the bit pattern it produces --
    // which is what the case-by-case comparison above is measuring, so nothing is asserted about it here.
}
