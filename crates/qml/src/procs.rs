//! Native QML types of the `Impulse` module that are a `Process` whose work is Rust
//! (qmltypes/Impulse/*.qmltypes with `prototype: "Process"`): QML sets their properties and
//! `running`, and reads `stdout` and `exited` as from any Process. Each type is an
//! [`Element`] with its own job.
use crate::impulse::Element;
use crate::jobs::{Job, text};
use crate::runtime::World;

pub static GEOCLUE_CHECK: Element = Element {
    job: Some(geoclue_check),
    created: Some(init),
    ..Element::new("GeoClueCheck")
};

pub static IMAGE_SIZE: Element = Element {
    job: Some(image_size),
    created: Some(init),
    ..Element::new("ImageSize")
};

pub static MICROTEX_RENDER: Element = Element {
    job: Some(microtex_render),
    created: Some(init),
    ..Element::new("MicroTexRender")
};

pub static PATH_KIND: Element = Element {
    job: Some(path_kind),
    created: Some(init),
    ..Element::new("PathKind")
};

pub static UPDATES_COUNT: Element = Element {
    job: Some(updates_count),
    created: Some(init),
    ..Element::new("UpdatesCount")
};

pub static UPDATES_PROBE: Element = Element {
    job: Some(updates_probe),
    created: Some(init),
    ..Element::new("UpdatesProbe")
};

pub static WALLPAPER_PICKER: Element = Element {
    job: Some(wallpaper_picker),
    created: Some(init),
    ..Element::new("WallpaperPicker")
};

pub static WALLPAPER_SWITCH: Element = Element {
    job: Some(crate::jobs_wall_gcloud_trans::switchwall_proc),
    created: Some(init),
    ..Element::new("WallpaperSwitch")
};

pub static WALLPICK_RUN: Element = Element {
    job: Some(crate::jobs_wallpick::job),
    created: Some(init),
    ..Element::new("WallpickRun")
};

pub static WEATHER_FETCH: Element = Element {
    job: Some(weather_fetch),
    created: Some(init),
    ..Element::new("WeatherFetch")
};

/// A new instance `id`: its declared properties with their C++ default values.
fn init(w: &mut World, id: usize) {
    let Some((_, t)) = type_of(w, id).and_then(|n| crate::types::registry().resolve(n)) else {
        return;
    };
    for p in &t.props {
        w.declare(id, &p.name, "var");
        if let Some(v) = w.items[id].props.get_mut(p.name.as_str()) {
            v.value = crate::types::cpp_default(p);
        }
    }
}

/// The `Impulse` element `p` is an instance of, by name.
pub fn type_of(w: &World, p: usize) -> Option<&'static str> {
    crate::impulse::element_of(w, p).map(|e| e.name)
}

/// Property `name` of `p` as a path, without a `file://` prefix.
fn path(w: &mut World, p: usize, name: &str) -> String {
    crate::jobs_files::trim_file_protocol(&text(w, p, name)).to_string()
}

fn geoclue_check(_: &mut World, _: usize) -> Option<Job> {
    Some(Box::new(|_, _| crate::probe::geoclue().code))
}

fn image_size(w: &mut World, p: usize) -> Option<Job> {
    crate::jobs_identify::job(&path(w, p, "path"))
}

fn microtex_render(w: &mut World, p: usize) -> Option<Job> {
    let args = crate::latex::microtex_args(
        &text(w, p, "expression"),
        &text(w, p, "output"),
        &text(w, p, "textSize"),
        &text(w, p, "padding"),
        &text(w, p, "foreground"),
    );
    Some(crate::latex::job(
        text(w, p, "binaryDir"),
        text(w, p, "binaryName"),
        text(w, p, "output"),
        args,
    ))
}

fn path_kind(w: &mut World, p: usize) -> Option<Job> {
    let dir = path(w, p, "path");
    Some(Box::new(move |io, _| {
        let out = crate::jobs_files::validate_dir_out(&dir);
        io.out(out);
        if out == "dir\n" { 0 } else { 1 }
    }))
}

fn updates_count(_: &mut World, _: usize) -> Option<Job> {
    Some(crate::jobs_updates::count_job())
}

fn updates_probe(_: &mut World, _: usize) -> Option<Job> {
    Some(crate::jobs_updates::probe_job())
}

fn wallpaper_picker(_: &mut World, _: usize) -> Option<Job> {
    Some(crate::jobs_wall_gcloud_trans::switchwall_picker())
}

fn weather_fetch(w: &mut World, p: usize) -> Option<Job> {
    Some(crate::jobs::weather(text(w, p, "place")))
}
