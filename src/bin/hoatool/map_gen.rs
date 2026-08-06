//! `hoatool map gen <world.toml> <out-dir>` — generate a Tiled project
//! template (.tsx) with the right layer structure for geography authoring.
//!
//! The generated template has layers for: settlements, rivers, routes,
//! regions, landmarks. The biome tile layer is not included (it comes
//! from the Tiled map itself); this template only provides the geography
//! object groups that the import command reads.

use std::path::Path;

use anyhow::{Context, Result, bail};

/// Run `hoatool map gen <world.toml> <out-dir>`.
pub fn run_map_gen(args: &[String]) -> Result<()> {
    if args.len() != 2 {
        bail!("usage: hoatool map gen <world.toml> <out-dir>");
    }
    let world_toml = Path::new(&args[0]);
    let out_dir = Path::new(&args[1]);

    let world = heart_of_africa::content::load::load_world(
        world_toml
            .parent()
            .context("world.toml must have a parent directory")?,
    )
    .context("loading world for dimensions")?;

    std::fs::create_dir_all(out_dir).context("creating output directory")?;

    let tsx_path = out_dir.join("geography.tsx");
    let xml = generate_tsx(&world);
    std::fs::write(&tsx_path, xml).with_context(|| format!("writing {}", tsx_path.display()))?;

    println!(
        "map gen: wrote {} ({}×{} tiles)",
        tsx_path.display(),
        world.width,
        world.height
    );
    Ok(())
}

/// Generate a minimal .tsx (Tiled XML) with the geography object groups.
fn generate_tsx(world: &heart_of_africa::world::World) -> String {
    let w = world.width;
    let h = world.height;

    let mut layers = String::new();

    // Settlements layer
    layers.push_str(&format!(
        r#"<objectgroup name="settlements" width="{w}" height="{h}">
  <!-- Add settlement objects with properties: polity (string) -->
  <!-- Example: <object id="1" name="cairo" x="512" y="256" width="32" height="32"> -->
  <!--   <properties><property name="polity" value="Khedivate of Egypt"/></properties> -->
  <!-- </object> -->
</objectgroup>
"#
    ));

    // Rivers layer
    layers.push_str(&format!(
        r#"<objectgroup name="rivers" width="{w}" height="{h}">
  <!-- Add river objects as polylines with properties: navigable (bool), width_class (string: narrow|normal|wide), seasonal (string or null) -->
  <!-- Example: <object id="1" name="nile" x="0" y="0"> -->
  <!--   <polyline points="0,0 100,50 200,100"/> -->
  <!--   <properties> -->
  <!--     <property name="navigable" type="bool" value="true"/> -->
  <!--     <property name="width_class" value="normal"/> -->
  <!--   </properties> -->
  <!-- </object> -->
</objectgroup>
"#
    ));

    // Routes layer
    layers.push_str(&format!(
        r#"<objectgroup name="routes" width="{w}" height="{h}">
  <!-- Add route objects as polylines with properties: from (string), to (string), risky (bool), modes (array), seasonal (bool), blocked (bool) -->
  <!-- Example: <object id="1" name="cairo-to-asyut" x="0" y="0"> -->
  <!--   <polyline points="0,0 100,50 200,100"/> -->
  <!--   <properties> -->
  <!--     <property name="from" value="cairo"/> -->
  <!--     <property name="to" value="asyut"/> -->
  <!--     <property name="risky" type="bool" value="false"/> -->
  <!--     <property name="modes" type="array" value="caravan"/> -->
  <!--   </properties> -->
  <!-- </object> -->
</objectgroup>
"#
    ));

    // Regions layer
    layers.push_str(&format!(
        r#"<objectgroup name="regions" width="{w}" height="{h}">
  <!-- Add region objects as closed polygons -->
  <!-- Example: <object id="1" name="nile-delta" x="0" y="0"> -->
  <!--   <polygon points="0,0 100,0 100,100 0,100 0,0"/> -->
  <!-- </object> -->
</objectgroup>
"#
    ));

    // Landmarks layer
    layers.push_str(&format!(
        r#"<objectgroup name="landmarks" width="{w}" height="{h}">
  <!-- Add landmark objects as points with property: kind (string) -->
  <!-- Example: <object id="1" name="great-pyramid" x="512" y="256" point="true"> -->
  <!--   <properties><property name="kind" value="landmark"/></properties> -->
  <!-- </object> -->
</objectgroup>
"#
    ));

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<tiledmap version="1.10" orientation="orthogonal" renderorder="right-down" width="{w}" height="{h}" tilewidth="32" tileheight="32" infinite="0" nextlayerid="1" nextobjectid="1">
{layers}</tiledmap>"#
    )
}
