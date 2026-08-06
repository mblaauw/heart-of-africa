//! WS1 map pipeline: Tiled TMJ import -> geography.toml round trip,
//! projection math, geography-overrides-legacy, and validation failures.
//!
//! The fixture is a fictional 4×4-tile map (32 px tiles -> 128×128 px) whose
//! expected km/tile values are exact given KM_PER_CELL = 33.3.

use std::path::{Path, PathBuf};

use heart_of_africa::content::import::{ImportProjection, MapManifest, import_tiled};
use heart_of_africa::content::load::load_world;

mod common;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/geography.tmj");

fn assert_km(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

fn real_world() -> heart_of_africa::world::World {
    load_world(common::content_dir()).expect("content loads")
}

/// Per-test temp dir under the system temp dir.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hoa-map-pipeline-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("creating temp dir");
    dir
}

fn write_temp_json(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("writing temp TMJ");
    path
}

#[test]
fn import_round_trip_extracts_all_layers() {
    let world = real_world();
    let (geo, manifest) = import_tiled(Path::new(FIXTURE), &world).expect("import succeeds");

    assert_eq!(geo.version, 1);
    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.tmj_width_px, 1536);
    assert_eq!(manifest.tmj_height_px, 2112);

    assert_eq!(geo.settlements.len(), 2);
    let t1 = &geo.settlements[0];
    assert_eq!(t1.id, "t1");
    assert_eq!(t1.polity, "Northdom");
    assert_eq!((t1.x, t1.y), (15, 8));
    let t2 = &geo.settlements[1];
    assert_eq!(t2.id, "t2");
    assert_eq!(t2.polity, "Unknown");
    assert_eq!((t2.x, t2.y), (16, 9));

    assert_eq!(geo.rivers.len(), 2);
    let r1 = &geo.rivers[0];
    assert_eq!(r1.id, "r1");
    assert_eq!(r1.points, vec![(14, 6), (14, 12)]);
    assert_km(r1.length_km, 199.8);
    assert!(!r1.navigable);
    assert_eq!(r1.width_class, "narrow");
    assert_eq!(r1.seasonal.as_deref(), Some("dry"));
    let r2 = &geo.rivers[1];
    assert_eq!(r2.points, vec![(15, 13)]);
    assert_km(r2.length_km, 0.0);
    assert!(r2.navigable);
    assert_eq!(r2.width_class, "normal");
    assert_eq!(r2.seasonal, None);

    assert_eq!(geo.routes.len(), 1);
    let route = &geo.routes[0];
    assert_eq!(route.id, "t1-t2");
    assert_eq!(route.from, "t1");
    assert_eq!(route.to, "t2");
    assert!(route.risky);
    assert_eq!(route.tiles, vec![(15, 8), (16, 9)]);
    assert_km(route.path_km, 33.3 * 2f64.sqrt());
    assert_eq!(
        route.modes,
        vec!["caravan".to_string(), "camel".to_string()]
    );
    assert!(!route.seasonal);
    assert!(!route.blocked);

    assert_eq!(geo.regions.len(), 1);
    let region = &geo.regions[0];
    assert_eq!(region.id, "region-a");
    assert_eq!(region.polygon.len(), 5);
    assert_km(region.polygon[0].0, 0.0);
    assert_km(region.polygon[2].0, 1598.4);
    assert_km(region.polygon[2].1, 2197.8);
    assert_km(region.polygon[4].1, 0.0);

    assert_eq!(geo.landmarks.len(), 1);
    let tomb = &geo.landmarks[0];
    assert_eq!(tomb.id, "tomb-a");
    assert_eq!(tomb.kind, "tomb");
    assert_km(tomb.point.0, 432.9);
    assert_km(tomb.point.1, 233.1);
}

#[test]
fn imported_geography_round_trips_through_toml_and_overrides_legacy() {
    let world = real_world();
    let (geo, _manifest) = import_tiled(Path::new(FIXTURE), &world).expect("import succeeds");

    let dir = temp_dir("override");
    let maps = dir.join("maps");
    std::fs::create_dir_all(&maps).expect("creating maps dir");
    std::fs::copy(
        common::content_dir().join("world.toml"),
        dir.join("world.toml"),
    )
    .expect("copying world.toml");
    std::fs::copy(
        common::content_dir().join("maps/world.png"),
        maps.join("world.png"),
    )
    .expect("copying biome png");
    let geo_text = toml::to_string(&geo).expect("serializing geography.toml");
    std::fs::write(dir.join("geography.toml"), geo_text).expect("writing geography.toml");

    let reloaded = load_world(&dir).expect("reloads with imported geography");
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(reloaded.settlements.len(), 2);
    assert_eq!(reloaded.settlements[0].id, "t1");
    assert_eq!(
        (reloaded.settlements[0].x, reloaded.settlements[0].y),
        (15, 8)
    );
    assert_eq!(reloaded.settlements[0].polity, "Northdom");
    assert_km(reloaded.rivers[0].length_km, 199.8);
    assert_eq!(reloaded.routes[0].tiles, vec![(15, 8), (16, 9)]);
    assert_eq!(reloaded.landmarks[0].id, "tomb-a");
    assert_eq!(reloaded.regions[0].id, "region-a");
}

#[test]
fn legacy_world_toml_geography_gets_straight_route_fallback() {
    let world = real_world();
    assert!(
        world.settlements.len() > 2,
        "legacy map has real settlements"
    );
    for route in &world.routes {
        assert!(!route.tiles.is_empty(), "legacy routes get a fallback path");
        assert!(route.path_km > 0.0, "legacy routes get a km length");
        let start = world
            .settlements
            .iter()
            .find(|s| s.id == route.from)
            .unwrap();
        let end = world.settlements.iter().find(|s| s.id == route.to).unwrap();
        assert_eq!(route.tiles.first(), Some(&(start.x, start.y)));
        assert_eq!(route.tiles.last(), Some(&(end.x, end.y)));
        // Every step moves at most one tile along each axis (straight walk).
        for w in route.tiles.windows(2) {
            let dx = (w[1].0 as i64 - w[0].0 as i64).abs();
            let dy = (w[1].1 as i64 - w[0].1 as i64).abs();
            assert!(dx <= 1 && dy <= 1 && dx + dy >= 1);
        }
    }
    assert!(
        world.rivers.iter().all(|r| r.length_km == 0.0),
        "legacy rivers carry no km length"
    );
}

#[test]
fn projection_math() {
    let proj = ImportProjection::new(128, 128, 133.2, 133.2);

    let (x, y) = proj.px_to_km(64.0, 64.0);
    assert_km(x, 66.6);
    assert_km(y, 66.6);
    assert_eq!(proj.km_to_tile(x, y), (2, 2));

    assert_eq!(proj.km_to_tile(0.0, 0.0), (0, 0));
    assert_eq!(proj.km_to_tile(33.3, 33.3), (1, 1));
    assert_eq!(proj.km_to_tile(-5.0, 133.2), (0, 4));
    let (x, y) = proj.px_to_km(128.0, 128.0);
    assert_eq!(proj.km_to_tile(x, y), (4, 4));
}

#[test]
fn settlement_outside_map_is_rejected() {
    let tmj = r#"{
        "width": 4, "height": 4, "tilewidth": 32, "tileheight": 32,
        "layers": [{
            "name": "settlements", "type": "objectgroup",
            "objects": [{
                "id": 1, "name": "far-away", "x": 200, "y": 200,
                "width": 0, "height": 0, "point": true, "properties": []
            }]
        }]
    }"#;
    let world = real_world();
    let dir = temp_dir("bad-settlement");
    let path = write_temp_json(&dir, "bad-settlement.tmj", tmj);
    let err = import_tiled(&path, &world).expect_err("out-of-bounds settlement fails");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        err.to_string().contains("outside"),
        "unexpected error: {err}"
    );
}

#[test]
fn route_without_from_property_is_rejected() {
    let tmj = r#"{
        "width": 4, "height": 4, "tilewidth": 32, "tileheight": 32,
        "layers": [{
            "name": "routes", "type": "objectgroup",
            "objects": [{
                "id": 1, "name": "mystery-route", "x": 32, "y": 32,
                "width": 0, "height": 0,
                "polyline": [{ "x": 0, "y": 0 }, { "x": 32, "y": 32 }],
                "properties": []
            }]
        }]
    }"#;
    let world = real_world();
    let dir = temp_dir("bad-route");
    let path = write_temp_json(&dir, "bad-route.tmj", tmj);
    let err = import_tiled(&path, &world).expect_err("route without from fails");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        err.to_string().contains("missing 'from' property"),
        "unexpected error: {err}"
    );
}

#[test]
fn manifest_is_public_api() {
    let m = MapManifest::new(1536, 2112);
    assert_eq!(m.version, 1);
    assert_eq!(m.tmj_width_px, 1536);
    assert_eq!(m.tmj_height_px, 2112);
}
