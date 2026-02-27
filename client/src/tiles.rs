#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::Reflect;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::HtmlImageElement;

const HQ_CONCURRENCY: usize = 6;
const LQ_CONCURRENCY: usize = 6;
const HQ_UPGRADE_CONCURRENCY: usize = 2;
const INITIAL_VIEW_CENTER_X: f64 = -300.0;
const INITIAL_VIEW_CENTER_Z: f64 = -3100.0;
const ONLOAD_HANDLE_KEY: &str = "__sequoiaTileOnload";
const ONERROR_HANDLE_KEY: &str = "__sequoiaTileOnerror";

type IdleCallback = Rc<dyn Fn()>;
type SharedIdleCallback = Rc<RefCell<Option<IdleCallback>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TileQuality {
    Low,
    High,
}

/// A loaded map tile image with its world coordinate bounds.
#[derive(Clone)]
pub struct LoadedTile {
    pub id: usize,
    pub quality: TileQuality,
    pub image: HtmlImageElement,
    pub x1: i32,
    pub z1: i32,
    pub x2: i32,
    pub z2: i32,
}

/// Static tile definitions: (filename, start_x, start_z, end_x, end_z).
/// Coordinates from wynnmap (Zatzou/wynnmap) — Main grid + Realm of Light.
const TILES: &[(&str, i32, i32, i32, i32)] = &[
    ("main-1-1.webp", -2560, -6144, -1025, -5121),
    ("main-1-2.webp", -1024, -6144, 1023, -5121),
    ("main-1-3.webp", 1024, -6144, 2047, -5121),
    ("main-2-1.webp", -2560, -5120, -1025, -4097),
    ("main-2-2.webp", -1024, -5120, 1023, -4097),
    ("main-2-3.webp", 1024, -5120, 2047, -4097),
    ("main-3-1.webp", -2560, -4096, -1025, -3073),
    ("main-3-2.webp", -1024, -4096, 1023, -3073),
    ("main-3-3.webp", 1024, -4096, 2047, -3073),
    ("main-4-1.webp", -2560, -3072, -1025, -2049),
    ("main-4-2.webp", -1024, -3072, 1023, -2049),
    ("main-4-3.webp", 1024, -3072, 2047, -2049),
    ("main-5-1.webp", -2560, -2048, -1025, -1025),
    ("main-5-2.webp", -1024, -2048, 1023, -1025),
    ("main-5-3.webp", 1024, -2048, 2047, -1025),
    ("main-6-1.webp", -2560, -1024, -1025, -1),
    ("main-6-2.webp", -1024, -1024, 1023, -1),
    ("main-6-3.webp", 1024, -1024, 2047, -1),
    ("realm-of-light.webp", -1536, -6656, -513, -5633),
];

#[derive(Clone, Copy)]
struct LoadJob {
    id: usize,
    filename: &'static str,
    quality: TileQuality,
    x1: i32,
    z1: i32,
    x2: i32,
    z2: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupTileMode {
    HighOnly,
    ProgressiveLowToHigh,
}

/// Load local map tile images from static assets.
pub fn fetch_tiles(tiles_signal: RwSignal<Vec<LoadedTile>>) {
    tiles_signal.set(Vec::new());

    match detect_startup_tile_mode() {
        StartupTileMode::HighOnly => {
            let hq_jobs = make_jobs(TileQuality::High);
            start_queue(tiles_signal, hq_jobs, HQ_CONCURRENCY, None);
        }
        StartupTileMode::ProgressiveLowToHigh => {
            let lq_jobs = make_jobs(TileQuality::Low);
            let signal_for_hq = tiles_signal;
            let on_lq_complete: Rc<dyn Fn()> = Rc::new(move || {
                let hq_jobs = make_jobs(TileQuality::High);
                start_queue(signal_for_hq, hq_jobs, HQ_UPGRADE_CONCURRENCY, None);
            });
            start_queue(tiles_signal, lq_jobs, LQ_CONCURRENCY, Some(on_lq_complete));
        }
    }
}

fn detect_startup_tile_mode() -> StartupTileMode {
    let Some(window) = web_sys::window() else {
        return StartupTileMode::ProgressiveLowToHigh;
    };

    let Ok(navigator) = Reflect::get(window.as_ref(), &JsValue::from_str("navigator")) else {
        return StartupTileMode::ProgressiveLowToHigh;
    };
    let Ok(connection) = Reflect::get(&navigator, &JsValue::from_str("connection")) else {
        return StartupTileMode::ProgressiveLowToHigh;
    };

    let save_data = Reflect::get(&connection, &JsValue::from_str("saveData"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if save_data {
        return StartupTileMode::ProgressiveLowToHigh;
    }

    let effective_type = Reflect::get(&connection, &JsValue::from_str("effectiveType"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(effective_type.as_str(), "slow-2g" | "2g" | "3g") {
        return StartupTileMode::ProgressiveLowToHigh;
    }

    let downlink_mbps = Reflect::get(&connection, &JsValue::from_str("downlink"))
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(10.0);
    if downlink_mbps >= 8.0 && effective_type == "4g" {
        return StartupTileMode::HighOnly;
    }

    StartupTileMode::ProgressiveLowToHigh
}

fn make_jobs(quality: TileQuality) -> VecDeque<LoadJob> {
    let mut jobs: Vec<_> = TILES
        .iter()
        .enumerate()
        .map(|(id, &(filename, x1, z1, x2, z2))| LoadJob {
            id,
            filename,
            quality,
            x1,
            z1,
            x2,
            z2,
        })
        .collect();

    jobs.sort_by(|a, b| {
        distance_sq_to_initial_view(a)
            .total_cmp(&distance_sq_to_initial_view(b))
            .then_with(|| a.id.cmp(&b.id))
    });

    jobs.into()
}

fn distance_sq_to_initial_view(job: &LoadJob) -> f64 {
    let center_x = (job.x1 as f64 + job.x2 as f64) * 0.5;
    let center_z = (job.z1 as f64 + job.z2 as f64) * 0.5;
    let dx = center_x - INITIAL_VIEW_CENTER_X;
    let dz = center_z - INITIAL_VIEW_CENTER_Z;
    dx * dx + dz * dz
}

fn start_queue(
    tiles_signal: RwSignal<Vec<LoadedTile>>,
    jobs: VecDeque<LoadJob>,
    max_concurrency: usize,
    on_idle: Option<IdleCallback>,
) {
    if jobs.is_empty() {
        if let Some(cb) = on_idle {
            cb();
        }
        return;
    }

    let queue = Rc::new(RefCell::new(jobs));
    let in_flight = Rc::new(Cell::new(0usize));
    let on_idle = Rc::new(RefCell::new(on_idle));
    pump_queue(tiles_signal, queue, in_flight, max_concurrency, on_idle);
}

fn pump_queue(
    tiles_signal: RwSignal<Vec<LoadedTile>>,
    queue: Rc<RefCell<VecDeque<LoadJob>>>,
    in_flight: Rc<Cell<usize>>,
    max_concurrency: usize,
    on_idle: SharedIdleCallback,
) {
    while in_flight.get() < max_concurrency {
        let Some(job) = queue.borrow_mut().pop_front() else {
            break;
        };
        in_flight.set(in_flight.get() + 1);

        let queue_next = queue.clone();
        let in_flight_next = in_flight.clone();
        let on_idle_next = on_idle.clone();
        let on_done: IdleCallback = Rc::new(move || {
            in_flight_next.set(in_flight_next.get().saturating_sub(1));
            pump_queue(
                tiles_signal,
                queue_next.clone(),
                in_flight_next.clone(),
                max_concurrency,
                on_idle_next.clone(),
            );
        });

        load_tile_job(tiles_signal, job, on_done);
    }

    if queue.borrow().is_empty()
        && in_flight.get() == 0
        && let Some(cb) = on_idle.borrow_mut().take()
    {
        cb();
    }
}

fn load_tile_job(tiles_signal: RwSignal<Vec<LoadedTile>>, job: LoadJob, on_done: Rc<dyn Fn()>) {
    let src = tile_src(job.filename, job.quality);
    let img = match HtmlImageElement::new() {
        Ok(img) => img,
        Err(_) => {
            on_done();
            return;
        }
    };

    let img_for_load = img.clone();
    let on_done_load = on_done.clone();
    let onload = Closure::<dyn FnMut()>::new(move || {
        clear_image_handlers(&img_for_load);

        let img_for_decode = img_for_load.clone();
        let on_done_load = on_done_load.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(img_for_decode.decode()).await;
            upsert_tile(
                tiles_signal,
                LoadedTile {
                    id: job.id,
                    quality: job.quality,
                    image: img_for_decode,
                    x1: job.x1,
                    z1: job.z1,
                    x2: job.x2,
                    z2: job.z2,
                },
            );
            on_done_load();
        });
    });

    let img_for_error = img.clone();
    let on_done_error = on_done.clone();
    let onerror = Closure::<dyn FnMut()>::new(move || {
        clear_image_handlers(&img_for_error);
        on_done_error();
    });

    let onload_js = onload.into_js_value();
    let onerror_js = onerror.into_js_value();
    img.set_onload(Some(onload_js.unchecked_ref()));
    img.set_onerror(Some(onerror_js.unchecked_ref()));
    let _ = Reflect::set(
        img.as_ref(),
        &JsValue::from_str(ONLOAD_HANDLE_KEY),
        &onload_js,
    );
    let _ = Reflect::set(
        img.as_ref(),
        &JsValue::from_str(ONERROR_HANDLE_KEY),
        &onerror_js,
    );
    img.set_src(&src);
}

fn clear_image_handlers(img: &HtmlImageElement) {
    img.set_onload(None);
    img.set_onerror(None);
    let _ = Reflect::delete_property(img.as_ref(), &JsValue::from_str(ONLOAD_HANDLE_KEY));
    let _ = Reflect::delete_property(img.as_ref(), &JsValue::from_str(ONERROR_HANDLE_KEY));
}

fn tile_src(filename: &str, quality: TileQuality) -> String {
    match quality {
        TileQuality::Low => format!("/tiles/lq/{filename}"),
        TileQuality::High => format!("/tiles/{filename}"),
    }
}

fn upsert_tile(tiles_signal: RwSignal<Vec<LoadedTile>>, incoming: LoadedTile) {
    tiles_signal.update(|loaded| {
        if let Some(existing) = loaded.iter_mut().find(|tile| tile.id == incoming.id) {
            if incoming.quality >= existing.quality {
                *existing = incoming;
            }
            return;
        }

        loaded.push(incoming);
        loaded.sort_by_key(|tile| tile.id);
    });
}
