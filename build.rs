use std::fs::{read, read_to_string};

use embedinator::{Icon, ResourceBuilder};

include!("assets/ids.rs");

const ICONS: &[(u16, &str)] = &[(APP_ICON, "assets/app.png")];

fn main() {
    let mut res = ResourceBuilder::from_env().add_manifest(read_to_string("assets/app.manifest").unwrap());

    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/ids.rs");

    let tray_icons = [false, true].into_iter().flat_map(|dark| {
        TRAY_SIZES.iter().enumerate().map(move |(index, size)| {
            let theme = if dark { "dark" } else { "light" };
            (tray_icon_id(dark, index), format!("assets/icons/tray_{theme}_{size}.png"))
        })
    });
    let icons = ICONS.iter().map(|&(id, name)| (id, name.to_string())).chain(tray_icons);

    for (id, name) in icons {
        println!("cargo:rerun-if-changed={}", name);
        res = res.add_icon(id, Icon::from_png_bytes(read(&name).unwrap()));
    }

    res.finish();

    println!("cargo:rerun-if-changed=build.rs");
}
