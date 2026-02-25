use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent, PointerEvent, WheelEvent};

use sequoia_shared::TreasuryLevel;

use crate::app::{
    AbbreviateNames, BoldConnections, BoldNames, BoldTags, CurrentMode, HistoryTimestamp, Hovered,
    MapMode, NameColor, NameColorSetting, ReadableFont, ResourceHighlight, Selected, ShowCountdown,
    ShowGranularMapTime, ShowNames, SidebarOpen, ThickCooldownBorders, ThickNameOutline,
    ThickTagOutline,
};
use crate::colors::{brighten, rgba_css};
use crate::gpu::{GpuRenderer, RenderFrameInput};
use crate::icons::ResourceIcons;
use crate::render_loop::RenderScheduler;
use crate::spatial::SpatialGrid;
use crate::territory::ClientTerritoryMap;
use crate::tiles::{LoadedTile, TileQuality};
use crate::time_format::format_hms;
use crate::viewport::Viewport;

/// Render scale for text supersampling.
/// Always at least 2x for crisp text, or native DPR if higher.
const GPU_PIXEL_BUDGET_DEFAULT: f64 = 3_200_000.0;
const TEXT_PIXEL_BUDGET_DEFAULT: f64 = 4_500_000.0;
const FIREFOX_TEXT_PIXEL_BUDGET: f64 = TEXT_PIXEL_BUDGET_DEFAULT;
const GPU_PIXEL_BUDGET_MIN: f64 = 2_400_000.0;
const GPU_PIXEL_BUDGET_MAX: f64 = 7_200_000.0;
const DESKTOP_TARGET_FPS: f64 = 120.0;
const MOBILE_TARGET_FPS: f64 = 60.0;
const FIREFOX_DESKTOP_TARGET_FPS: f64 = 90.0;
const FIREFOX_MOBILE_TARGET_FPS: f64 = 60.0;
const INTERACTION_SETTLE_MS: f64 = 140.0;
const TEXT_REDRAW_IDLE_MS: f64 = 50.0;
const TEXT_REDRAW_PAN_INTERACT_MS: f64 = 85.0;
const TEXT_REDRAW_ZOOM_INTERACT_MS: f64 = 24.0;
const PAN_REDRAW_DISTANCE_PX: f64 = 96.0;
const PAN_VISIBILITY_REDRAW_DISTANCE_PX: f64 = 56.0;
const PAN_VISIBILITY_REDRAW_MS: f64 = 70.0;
const ZOOM_REFRESH_RATIO_DELTA: f64 = 0.01;
const GPU_SCALE_QUANTUM: f64 = 0.05;
const TERRITORY_OVERLAY_SCALE: f64 = 1.08;
const ZOOM_OUT_TEXT_BOOST_START: f64 = 0.35;
const ZOOM_OUT_TEXT_BOOST_END: f64 = 0.05;
const ZOOM_OUT_TEXT_BOOST_MAX: f64 = 1.26;
const TEXT_WIDTH_CACHE_MAX_ENTRIES: usize = 40_000;
const NAME_FIT_CACHE_MAX_ENTRIES: usize = 24_000;

fn tile_upload_signature(tiles: &[LoadedTile]) -> u64 {
    tiles.iter().fold(0u64, |acc, tile| {
        let quality_bits = match tile.quality {
            TileQuality::Low => 1u64,
            TileQuality::High => 2u64,
        };
        acc.wrapping_mul(1_099_511_628_211)
            .wrapping_add(((tile.id as u64) << 2) ^ quality_bits)
    })
}

fn clamp_scale_for_pixel_budget(
    css_width: u32,
    css_height: u32,
    desired_scale: f64,
    min_scale: f64,
    pixel_budget: f64,
) -> f64 {
    let mut scale = desired_scale.max(min_scale);
    if css_width == 0 || css_height == 0 {
        return scale;
    }
    let total_pixels = css_width as f64 * css_height as f64 * scale * scale;
    if total_pixels > pixel_budget {
        scale *= (pixel_budget / total_pixels).sqrt();
    }
    scale.max(min_scale)
}

fn clamp_budget(value: f64, min_budget: f64, max_budget: f64) -> f64 {
    value.clamp(min_budget, max_budget)
}

fn quantize_scale(value: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return value;
    }
    (value / step).round() * step
}

#[derive(Clone, Copy)]
struct BrowserPerfHints {
    is_firefox: bool,
    target_fps: f64,
    text_pixel_budget: f64,
}

fn browser_perf_hints() -> BrowserPerfHints {
    let ua = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_firefox = ua.contains("firefox") || ua.contains("fxios");
    let is_mobile = ua.contains("android")
        || ua.contains("iphone")
        || ua.contains("ipad")
        || ua.contains("mobile");
    let target_fps = if is_firefox {
        if is_mobile {
            FIREFOX_MOBILE_TARGET_FPS
        } else {
            FIREFOX_DESKTOP_TARGET_FPS
        }
    } else if is_mobile {
        MOBILE_TARGET_FPS
    } else {
        DESKTOP_TARGET_FPS
    };
    let text_pixel_budget = if is_firefox {
        FIREFOX_TEXT_PIXEL_BUDGET
    } else {
        TEXT_PIXEL_BUDGET_DEFAULT
    };
    BrowserPerfHints {
        is_firefox,
        target_fps,
        text_pixel_budget,
    }
}

pub fn gpu_render_scale(css_width: u32, css_height: u32, pixel_budget: f64) -> f64 {
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    clamp_scale_for_pixel_budget(css_width, css_height, dpr, 1.0, pixel_budget)
}

pub fn render_scale_for(css_width: u32, css_height: u32, pixel_budget: f64) -> f64 {
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    clamp_scale_for_pixel_budget(css_width, css_height, dpr.max(2.0), 1.25, pixel_budget)
}

pub fn render_scale() -> f64 {
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    dpr.max(2.0)
}

/// Two-canvas map renderer: wgpu for geometry/effects, Canvas 2D overlay for text labels.
#[component]
pub fn MapCanvas() -> impl IntoView {
    let territories: RwSignal<ClientTerritoryMap> = expect_context();
    let viewport: RwSignal<Viewport> = expect_context();
    let Hovered(hovered) = expect_context();
    let Selected(selected) = expect_context();
    let CurrentMode(map_mode) = expect_context();
    let HistoryTimestamp(history_timestamp) = expect_context();
    let SidebarOpen(sidebar_open) = expect_context();
    let mouse_pos: RwSignal<(f64, f64)> = expect_context();
    let loaded_tiles: RwSignal<Vec<LoadedTile>> = expect_context();
    let tick: RwSignal<i64> = expect_context();
    let show_connections: RwSignal<bool> = expect_context();
    let AbbreviateNames(abbreviate_names) = expect_context();
    let NameColorSetting(name_color) = expect_context();
    let ShowCountdown(show_countdown) = expect_context();
    let ShowGranularMapTime(show_granular_map_time) = expect_context();
    let ShowNames(show_names) = expect_context();
    let ThickCooldownBorders(thick_cooldown_borders) = expect_context();
    let BoldNames(bold_names) = expect_context();
    let BoldTags(bold_tags) = expect_context();
    let ThickTagOutline(thick_tag_outline) = expect_context();
    let ThickNameOutline(thick_name_outline) = expect_context();
    let ReadableFont(readable_font) = expect_context();
    let BoldConnections(bold_connections) = expect_context();
    let ResourceHighlight(resource_highlight) = expect_context();
    let loaded_icons: RwSignal<Option<ResourceIcons>> = expect_context();

    let gpu_canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let text_canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let perf_hints = browser_perf_hints();
    let text_canvas_style: &'static str = if perf_hints.is_firefox {
        "position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; transform-origin: 0 0;"
    } else {
        "position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; transform-origin: 0 0; will-change: transform;"
    };

    // Track drag state
    let is_dragging = Rc::new(Cell::new(false));
    let drag_start_x = Rc::new(Cell::new(0.0f64));
    let drag_start_y = Rc::new(Cell::new(0.0f64));
    let last_x = Rc::new(Cell::new(0.0f64));
    let last_y = Rc::new(Cell::new(0.0f64));

    // Track pinch state
    let pinch_dist = Rc::new(Cell::new(0.0f64));
    // Interaction activity window (drag/wheel/pinch). Used to soften expensive redraw work.
    let interaction_deadline = Rc::new(Cell::new(0.0f64));

    // Spatial grid for O(1) hit-testing
    let spatial_grid: Rc<RefCell<SpatialGrid>> =
        Rc::new(RefCell::new(SpatialGrid::build(&HashMap::new())));
    let grid_for_move = spatial_grid.clone();
    let grid_for_click = spatial_grid.clone();

    // Territory world bounds for tile culling
    type WorldBounds = Option<(f64, f64, f64, f64)>;
    let world_bounds: Rc<Cell<WorldBounds>> = Rc::new(Cell::new(None));
    let world_bounds_render = world_bounds.clone();

    // Text measurement cache
    let text_cache: Rc<RefCell<TextWidthCache>> = Rc::new(RefCell::new(HashMap::new()));
    let text_cache_render = text_cache.clone();
    // Territory-name fit cache (post-truncation display text per style/width bucket)
    let name_fit_cache: Rc<RefCell<NameFitCache>> = Rc::new(RefCell::new(HashMap::new()));
    let name_fit_cache_render = name_fit_cache.clone();

    // Text redraw invalidation channels:
    // - static: data/settings/content layout changes
    // - clock: per-second timer/history-time changes
    let text_gen_static: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let text_gen_static_render = text_gen_static.clone();
    let rendered_text_gen_static: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let text_gen_clock: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let text_gen_clock_render = text_gen_clock.clone();
    let rendered_text_gen_clock: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let last_text_time: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));

    // Viewport state at last text draw — used to compute CSS transform
    // that keeps text visually aligned between throttled redraws.
    // (offset_x, offset_y, scale); scale=0 means "not yet drawn".
    let text_vp_state: Rc<Cell<(f64, f64, f64)>> = Rc::new(Cell::new((0.0, 0.0, 0.0)));
    let text_vp_render = text_vp_state.clone();

    // Last applied CSS transform for the text canvas.
    // None means "transform: none" is currently applied.
    let text_css_transform: Rc<Cell<Option<(f64, f64, f64)>>> = Rc::new(Cell::new(None));
    let text_css_render = text_css_transform.clone();

    // Rebuild spatial grid whenever territories change
    Effect::new({
        let grid = spatial_grid.clone();
        let wb = world_bounds.clone();
        let tc = text_cache.clone();
        let nfc = name_fit_cache.clone();
        move || {
            territories.with(|t| {
                *grid.borrow_mut() = SpatialGrid::build(t);
                wb.set(grid.borrow().world_bounds());
                tc.borrow_mut().clear();
                nfc.borrow_mut().clear();
            });
        }
    });

    // Clear text width cache when font changes (different fonts have different widths)
    Effect::new({
        let tc = text_cache.clone();
        let nfc = name_fit_cache.clone();
        move || {
            readable_font.track();
            tc.borrow_mut().clear();
            nfc.borrow_mut().clear();
        }
    });

    // Fit bounds on first data load
    let fitted = Rc::new(Cell::new(false));
    let fitted_render = fitted.clone();

    // GPU renderer (initialized async, None until ready)
    let gpu: Rc<RefCell<Option<GpuRenderer>>> = Rc::new(RefCell::new(None));
    let gpu_render = gpu.clone();
    let gpu_init_started = Rc::new(Cell::new(false));

    // Track tile count for re-upload detection
    let last_tile_count: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let last_tile_signature: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    // Cached Canvas 2D context (invalidated on canvas resize)
    let cached_text_ctx: Rc<RefCell<Option<CanvasRenderingContext2d>>> =
        Rc::new(RefCell::new(None));
    let cached_text_ctx_render = cached_text_ctx.clone();

    // Adaptive performance governor.
    // Firefox uses a slightly more conservative target/budget profile.
    let target_fps = perf_hints.target_fps;
    let text_pixel_budget = perf_hints.text_pixel_budget;
    let perf_gpu_budget: Rc<Cell<f64>> = Rc::new(Cell::new(GPU_PIXEL_BUDGET_DEFAULT));
    let perf_sample_start: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let perf_frame_count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    // Hold GPU scale between frames to avoid resize thrash from tiny budget oscillations.
    let perf_gpu_scale: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let perf_last_size: Rc<Cell<(u32, u32)>> = Rc::new(Cell::new((0, 0)));

    // Render function
    let perf_gpu_budget_render = perf_gpu_budget.clone();
    let perf_sample_start_render = perf_sample_start.clone();
    let perf_frame_count_render = perf_frame_count.clone();
    let perf_gpu_scale_render = perf_gpu_scale.clone();
    let perf_last_size_render = perf_last_size.clone();
    let interaction_deadline_render = interaction_deadline.clone();
    let is_dragging_render = is_dragging.clone();
    let scheduler = RenderScheduler::new(move || {
        let Some(gpu_canvas) = gpu_canvas_ref.get_untracked() else {
            return false;
        };
        let gpu_canvas: &HtmlCanvasElement = &gpu_canvas;

        let Some(text_canvas) = text_canvas_ref.get_untracked() else {
            return false;
        };
        let text_canvas: &HtmlCanvasElement = &text_canvas;

        // Resize canvases to container with DPR/supersampling
        let Some(parent) = gpu_canvas.parent_element() else {
            return false;
        };
        let w = parent.client_width() as u32;
        let h = parent.client_height() as u32;
        if w == 0 || h == 0 {
            return false;
        }
        let perf_now = js_sys::Date::now();
        let interaction_active =
            is_dragging_render.get() || perf_now < interaction_deadline_render.get();
        let desired_gpu_scale = quantize_scale(
            gpu_render_scale(w, h, perf_gpu_budget_render.get()),
            GPU_SCALE_QUANTUM,
        )
        .max(0.5);
        let mut gpu_scale = perf_gpu_scale_render.get();
        let last_size = perf_last_size_render.get();
        if gpu_scale <= 0.0
            || last_size != (w, h)
            || (desired_gpu_scale - gpu_scale).abs() >= GPU_SCALE_QUANTUM
        {
            gpu_scale = desired_gpu_scale;
            perf_gpu_scale_render.set(gpu_scale);
            perf_last_size_render.set((w, h));
        }
        let text_scale = render_scale_for(w, h, text_pixel_budget);
        // GPU canvas: adaptive internal resolution (CSS size stays 100%)
        let gw = (w as f64 * gpu_scale).round().max(1.0) as u32;
        let gh = (h as f64 * gpu_scale).round().max(1.0) as u32;
        // Text canvas: supersampled (2x minimum) for crisp text
        let tw = (w as f64 * text_scale).round().max(1.0) as u32;
        let th = (h as f64 * text_scale).round().max(1.0) as u32;
        if gpu_canvas.width() != gw || gpu_canvas.height() != gh {
            gpu_canvas.set_width(gw);
            gpu_canvas.set_height(gh);
            text_canvas.set_width(tw);
            text_canvas.set_height(th);
            // Canvas resize resets 2D context state — invalidate cache
            *cached_text_ctx_render.borrow_mut() = None;
            text_gen_static_render.set(text_gen_static_render.get().wrapping_add(1));
            if let Some(ref mut renderer) = *gpu_render.borrow_mut() {
                renderer.resize(gw, gh, gpu_scale as f32);
            }
            text_css_render.set(None);
        }

        let vp = viewport.get_untracked();
        let hov = hovered.get_untracked();
        let sel = selected.get_untracked();
        let wall_now_secs = (perf_now / 1000.0) as i64;
        let reference_time_secs = if map_mode.get_untracked() == MapMode::History {
            history_timestamp.get_untracked().unwrap_or(wall_now_secs)
        } else {
            wall_now_secs
        };

        // Auto-fit on first data load
        let empty = territories.with_untracked(|t| t.is_empty());
        if !fitted_render.get() && !empty {
            fitted_render.set(true);
            territories.with_untracked(|t| {
                let (mut min_x, mut min_y, mut max_x, mut max_y) =
                    (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for ct in t.values() {
                    let loc = &ct.territory.location;
                    min_x = min_x.min(loc.left() as f64);
                    min_y = min_y.min(loc.top() as f64);
                    max_x = max_x.max(loc.right() as f64);
                    max_y = max_y.max(loc.bottom() as f64);
                }
                viewport.update(|vp| {
                    vp.fit_bounds(min_x, min_y, max_x, max_y, w as f64, h as f64);
                });
            });
            return false;
        }

        // Check if GPU renderer is ready
        let mut gpu_ref = gpu_render.borrow_mut();
        let has_gpu = gpu_ref.is_some();

        let text_ctx = {
            let mut ctx_cache = cached_text_ctx_render.borrow_mut();
            if ctx_cache.is_none() {
                let Some(ctx) = text_canvas
                    .get_context("2d")
                    .ok()
                    .flatten()
                    .and_then(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().ok())
                else {
                    return false;
                };
                // Apply supersampling scale — all drawing stays in CSS pixel coords
                ctx.scale(text_scale, text_scale).ok();
                *ctx_cache = Some(ctx);
            }
            let Some(ctx) = ctx_cache.clone() else {
                return false;
            };
            ctx
        };

        if !has_gpu {
            // GPU not ready yet — render Canvas 2D fallback on the TEXT canvas
            // (gpu_canvas must stay untouched so wgpu can claim it)
            let bounds = world_bounds_render.get();
            territories.with_untracked(|terr| {
                loaded_tiles.with_untracked(|tiles| {
                    render_canvas2d_fallback(Canvas2dFallbackInput {
                        ctx: &text_ctx,
                        w: w as f64,
                        h: h as f64,
                        vp: &vp,
                        territories: terr,
                        hovered: &hov,
                        selected: &sel,
                        reference_time_secs,
                        tiles,
                        world_bounds: bounds,
                        style: CanvasFallbackStyle {
                            thick_cooldown_borders: thick_cooldown_borders.get_untracked(),
                            resource_highlight: resource_highlight.get_untracked(),
                        },
                    });
                });
                // Render labels on top of fallback (don't clear — territories are already drawn)
                let mut tc = text_cache_render.borrow_mut();
                let mut nfc = name_fit_cache_render.borrow_mut();
                let sc = show_connections.get_untracked();
                let ab = abbreviate_names.get_untracked();
                let nc = name_color.get_untracked();
                let cd = show_countdown.get_untracked();
                let gmt = show_granular_map_time.get_untracked();
                let sn = show_names.get_untracked();
                let bn = bold_names.get_untracked();
                let bt = bold_tags.get_untracked();
                let tto = thick_tag_outline.get_untracked();
                let tno = thick_name_outline.get_untracked();
                let rf = readable_font.get_untracked();
                let bc = bold_connections.get_untracked();
                let ic = loaded_icons.get_untracked();
                render_text_overlay(TextOverlayInput {
                    ctx: &text_ctx,
                    w: w as f64,
                    h: h as f64,
                    vp: &vp,
                    territories: terr,
                    hovered: &hov,
                    selected: &sel,
                    reference_time_secs,
                    text_cache: &mut tc,
                    name_fit_cache: &mut nfc,
                    clear: false,
                    style: TextOverlayStyle {
                        show_connections: sc,
                        abbreviate: ab,
                        name_color: nc,
                        show_countdown: cd,
                        show_granular_map_time: gmt,
                        show_names: sn,
                        bold_names: bn,
                        bold_tags: bt,
                        thick_tag_outline: tto,
                        thick_name_outline: tno,
                        readable_font: rf,
                        bold_connections: bc,
                    },
                    icons: &ic,
                });
                if text_css_render.get().is_some() {
                    let _ =
                        web_sys::HtmlElement::style(text_canvas).set_property("transform", "none");
                    text_css_render.set(None);
                }
            });
            // Sync text generation tracking so GPU path doesn't burst-redraw on init
            rendered_text_gen_static.set(text_gen_static_render.get());
            rendered_text_gen_clock.set(text_gen_clock_render.get());
            last_text_time.set(js_sys::Date::now());
            text_vp_render.set((vp.offset_x, vp.offset_y, vp.scale));
            return false;
        }

        let Some(renderer) = gpu_ref.as_mut() else {
            return false;
        };
        let bounds = world_bounds_render.get();

        territories.with_untracked(|terr| {
            loaded_tiles.with_untracked(|tiles| {
                // Upload new tile textures if needed
                let tile_count = tiles.len();
                let signature = tile_upload_signature(tiles);
                if tile_count != last_tile_count.get() || signature != last_tile_signature.get() {
                    renderer.upload_tiles(tiles);
                    last_tile_count.set(tile_count);
                    last_tile_signature.set(signature);
                }

                let now = js_sys::Date::now();

                // Render geometry on wgpu canvas
                let has_anims = renderer.render(RenderFrameInput {
                    vp: &vp,
                    territories: terr,
                    hovered: &hov,
                    selected: &sel,
                    tiles,
                    world_bounds: bounds,
                    now,
                    reference_time_secs,
                });

                // Reference-scale text rendering with throttled redraws.
                // Text is drawn once at a reference scale, then CSS transforms
                // handle all viewport changes (pan AND zoom) smoothly.
                // Full redraws are split by stale type:
                // - static/data changes redraw promptly
                // - clock-only changes can defer while interacting
                let cur_gen_static = text_gen_static_render.get();
                let cur_gen_clock = text_gen_clock_render.get();
                let stale_static = cur_gen_static != rendered_text_gen_static.get();
                let stale_clock = cur_gen_clock != rendered_text_gen_clock.get();
                let content_stale = stale_static || stale_clock;
                let (ref_ox, ref_oy, ref_scale) = text_vp_render.get();

                // CSS transform helper: bridge text canvas to current viewport
                let apply_text_css = |rox: f64, roy: f64, rsc: f64| {
                    let ratio = vp.scale / rsc;
                    let tx = vp.offset_x - rox * ratio;
                    let ty = vp.offset_y - roy * ratio;
                    let should_update = match text_css_render.get() {
                        Some((last_tx, last_ty, last_ratio)) => {
                            (tx - last_tx).abs() > 0.05
                                || (ty - last_ty).abs() > 0.05
                                || (ratio - last_ratio).abs() > 0.0005
                        }
                        None => true,
                    };
                    if should_update {
                        let _ = web_sys::HtmlElement::style(text_canvas).set_property(
                            "transform",
                            &format!("translate({tx:.1}px,{ty:.1}px) scale({ratio:.4})"),
                        );
                        text_css_render.set(Some((tx, ty, ratio)));
                    }
                };

                let scale_ratio_delta = if ref_scale > 0.0 {
                    (vp.scale / ref_scale - 1.0).abs()
                } else {
                    0.0
                };
                let is_zooming = scale_ratio_delta >= ZOOM_REFRESH_RATIO_DELTA;
                let pan_delta = (vp.offset_x - ref_ox)
                    .abs()
                    .max((vp.offset_y - ref_oy).abs());
                let needs_pan_edge_refresh =
                    interaction_active && !is_zooming && pan_delta >= PAN_REDRAW_DISTANCE_PX;
                let needs_pan_visibility_refresh = is_dragging_render.get()
                    && !is_zooming
                    && pan_delta >= PAN_VISIBILITY_REDRAW_DISTANCE_PX;
                let needs_zoom_quality_refresh = !interaction_active && is_zooming;

                let elapsed = now - last_text_time.get();
                let redraw_interval = if interaction_active {
                    if is_zooming {
                        TEXT_REDRAW_ZOOM_INTERACT_MS
                    } else {
                        TEXT_REDRAW_PAN_INTERACT_MS
                    }
                } else {
                    TEXT_REDRAW_IDLE_MS
                };
                let should_redraw = if ref_scale <= 0.0 {
                    true
                } else if needs_pan_visibility_refresh {
                    elapsed >= PAN_VISIBILITY_REDRAW_MS
                } else if stale_static {
                    elapsed >= redraw_interval || needs_pan_edge_refresh
                } else if stale_clock {
                    !interaction_active && elapsed >= TEXT_REDRAW_IDLE_MS
                } else if interaction_active && is_zooming {
                    elapsed >= TEXT_REDRAW_ZOOM_INTERACT_MS
                } else {
                    needs_zoom_quality_refresh
                };

                if should_redraw {
                    // Full redraw at current viewport — set new reference
                    let mut tc = text_cache_render.borrow_mut();
                    let mut nfc = name_fit_cache_render.borrow_mut();
                    let sc = show_connections.get_untracked();
                    let ab = abbreviate_names.get_untracked();
                    let nc = name_color.get_untracked();
                    let cd = show_countdown.get_untracked();
                    let gmt = show_granular_map_time.get_untracked();
                    let sn = show_names.get_untracked();
                    let bn = bold_names.get_untracked();
                    let bt = bold_tags.get_untracked();
                    let tto = thick_tag_outline.get_untracked();
                    let tno = thick_name_outline.get_untracked();
                    let rf = readable_font.get_untracked();
                    let bc = bold_connections.get_untracked();
                    let ic = loaded_icons.get_untracked();
                    render_text_overlay(TextOverlayInput {
                        ctx: &text_ctx,
                        w: w as f64,
                        h: h as f64,
                        vp: &vp,
                        territories: terr,
                        hovered: &hov,
                        selected: &sel,
                        reference_time_secs,
                        text_cache: &mut tc,
                        name_fit_cache: &mut nfc,
                        clear: true,
                        style: TextOverlayStyle {
                            show_connections: sc,
                            abbreviate: ab,
                            name_color: nc,
                            show_countdown: cd,
                            show_granular_map_time: gmt,
                            show_names: sn,
                            bold_names: bn,
                            bold_tags: bt,
                            thick_tag_outline: tto,
                            thick_name_outline: tno,
                            readable_font: rf,
                            bold_connections: bc,
                        },
                        icons: &ic,
                    });
                    rendered_text_gen_static.set(cur_gen_static);
                    rendered_text_gen_clock.set(cur_gen_clock);
                    last_text_time.set(now);
                    text_vp_render.set((vp.offset_x, vp.offset_y, vp.scale));
                    if text_css_render.get().is_some() {
                        let _ = web_sys::HtmlElement::style(text_canvas)
                            .set_property("transform", "none");
                        text_css_render.set(None);
                    }
                } else if content_stale || needs_zoom_quality_refresh {
                    // Content is stale but redraw is deferred; CSS transform bridges frames.
                    if ref_scale > 0.0 {
                        apply_text_css(ref_ox, ref_oy, ref_scale);
                    }
                } else if ref_scale > 0.0 {
                    // No content change — pure CSS transform for viewport tracking
                    apply_text_css(ref_ox, ref_oy, ref_scale);
                }

                // Keep animating if GPU has active transitions OR text content is pending
                let static_redraw_pending = stale_static && !should_redraw;
                let clock_redraw_pending_idle =
                    stale_clock && !stale_static && !interaction_active && !should_redraw;
                let zoom_redraw_pending = interaction_active && is_zooming && !should_redraw;
                let waiting_for_settle_clock =
                    stale_clock && !stale_static && interaction_active && !is_dragging_render.get();
                let keep_animating = has_anims
                    || static_redraw_pending
                    || clock_redraw_pending_idle
                    || zoom_redraw_pending
                    || waiting_for_settle_clock
                    || (needs_zoom_quality_refresh && !should_redraw);

                // Adapt internal pixel budgets toward target fps.
                let sample_start = perf_sample_start_render.get();
                if sample_start <= 0.0 {
                    perf_sample_start_render.set(perf_now);
                    perf_frame_count_render.set(0);
                }
                perf_frame_count_render.set(perf_frame_count_render.get().saturating_add(1));
                let elapsed = perf_now - perf_sample_start_render.get();
                if elapsed >= 1000.0 {
                    if !interaction_active {
                        let fps = perf_frame_count_render.get() as f64 / (elapsed / 1000.0);
                        let mut gpu_budget = perf_gpu_budget_render.get();
                        if fps < target_fps * 0.92 {
                            gpu_budget *= 0.95;
                        } else if fps > target_fps * 1.05 {
                            gpu_budget *= 1.01;
                        }
                        gpu_budget =
                            clamp_budget(gpu_budget, GPU_PIXEL_BUDGET_MIN, GPU_PIXEL_BUDGET_MAX);
                        perf_gpu_budget_render.set(gpu_budget);
                    }
                    perf_sample_start_render.set(perf_now);
                    perf_frame_count_render.set(0);
                }

                keep_animating
            })
        })
    });

    let scheduler = Rc::new(scheduler);

    // Initialize GPU renderer asynchronously
    let sched_for_init = scheduler.clone();
    Effect::new({
        let gpu = gpu.clone();
        let gpu_init_started = gpu_init_started.clone();
        move || {
            if gpu_init_started.get() {
                return;
            }
            let Some(canvas_el) = gpu_canvas_ref.get() else {
                return;
            };
            gpu_init_started.set(true);

            let canvas: &HtmlCanvasElement = &canvas_el;
            let canvas: HtmlCanvasElement = canvas.clone();
            let gpu = gpu.clone();
            let sched = sched_for_init.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match GpuRenderer::init(canvas).await {
                    Ok(renderer) => {
                        *gpu.borrow_mut() = Some(renderer);
                        sched.mark_dirty();
                    }
                    Err(e) => {
                        web_sys::console::warn_1(
                            &format!("wgpu init failed, using Canvas 2D fallback: {e}").into(),
                        );
                        // Canvas 2D fallback continues to work
                    }
                }
            });
        }
    });

    // State effect — data/settings changes need instance + text rebuild
    let sched_state = scheduler.clone();
    let gpu_state = gpu.clone();
    let text_gen_state = text_gen_static.clone();
    let name_fit_state = name_fit_cache.clone();
    Effect::new(move || {
        territories.track();
        show_connections.track();
        abbreviate_names.track();
        name_color.track();
        show_countdown.track();
        show_granular_map_time.track();
        show_names.track();
        thick_cooldown_borders.track();
        bold_names.track();
        bold_tags.track();
        thick_tag_outline.track();
        thick_name_outline.track();
        readable_font.track();
        bold_connections.track();
        resource_highlight.track();
        if let Some(ref mut renderer) = *gpu_state.borrow_mut() {
            renderer.thick_cooldown_borders = thick_cooldown_borders.get_untracked();
            renderer.resource_highlight = resource_highlight.get_untracked();
            renderer.mark_instance_dirty();
        }
        name_fit_state.borrow_mut().clear();
        text_gen_state.set(text_gen_state.get().wrapping_add(1));
        sched_state.mark_dirty();
    });

    // Hover/selection effect — GPU highlight only (no text overlay rebuild)
    let sched_hover = scheduler.clone();
    let gpu_hover = gpu.clone();
    Effect::new(move || {
        hovered.track();
        selected.track();
        if let Some(ref mut renderer) = *gpu_hover.borrow_mut() {
            renderer.mark_instance_dirty();
        }
        sched_hover.mark_dirty();
    });

    // Map clock effect: keep hold-time/cooldown visuals synced in both live and history modes.
    let sched_tick = scheduler.clone();
    let text_gen_tick = text_gen_clock.clone();
    let gpu_tick = gpu.clone();
    Effect::new(move || {
        if map_mode.get() == MapMode::History {
            history_timestamp.track();
        } else {
            tick.track();
        }
        if let Some(ref mut renderer) = *gpu_tick.borrow_mut() {
            renderer.mark_instance_dirty();
        }
        text_gen_tick.set(text_gen_tick.get().wrapping_add(1));
        sched_tick.mark_dirty();
    });

    // Viewport effect — pan/zoom/tile changes need repaint for geometry and CSS overlay transforms.
    // Text overlay content no longer invalidates on every viewport update.
    let sched_vp = scheduler.clone();
    Effect::new(move || {
        viewport.track();
        loaded_tiles.track();
        sched_vp.mark_dirty();
    });
    // --- Input handlers ---

    let on_wheel = {
        let interaction_deadline = interaction_deadline.clone();
        move |e: WheelEvent| {
            e.prevent_default();
            let delta = e.delta_y();
            let x = e.offset_x() as f64;
            let y = e.offset_y() as f64;
            interaction_deadline.set(js_sys::Date::now() + INTERACTION_SETTLE_MS);
            viewport.update(|vp| vp.zoom_at(delta, x, y));
        }
    };

    let on_pointer_down = {
        let is_dragging = is_dragging.clone();
        let drag_start_x = drag_start_x.clone();
        let drag_start_y = drag_start_y.clone();
        let last_x = last_x.clone();
        let last_y = last_y.clone();
        let interaction_deadline = interaction_deadline.clone();
        move |e: PointerEvent| {
            is_dragging.set(true);
            interaction_deadline.set(js_sys::Date::now() + INTERACTION_SETTLE_MS);
            hovered.set(None);
            drag_start_x.set(e.client_x() as f64);
            drag_start_y.set(e.client_y() as f64);
            last_x.set(e.client_x() as f64);
            last_y.set(e.client_y() as f64);

            if let Some(target) = e.target()
                && let Ok(el) = target.dyn_into::<web_sys::HtmlElement>()
            {
                el.set_pointer_capture(e.pointer_id()).ok();
                el.style().set_property("cursor", "grabbing").ok();
            }
        }
    };

    let on_pointer_move = {
        let is_dragging = is_dragging.clone();
        let last_x = last_x.clone();
        let last_y = last_y.clone();
        let grid = grid_for_move;
        let interaction_deadline = interaction_deadline.clone();
        move |e: PointerEvent| {
            if is_dragging.get() {
                let dx = e.client_x() as f64 - last_x.get();
                let dy = e.client_y() as f64 - last_y.get();
                last_x.set(e.client_x() as f64);
                last_y.set(e.client_y() as f64);
                interaction_deadline.set(js_sys::Date::now() + INTERACTION_SETTLE_MS);
                viewport.update(|vp| vp.pan(dx, dy));
            } else {
                let local = gpu_canvas_ref
                    .get_untracked()
                    .map(|el| {
                        let rect = el.get_bounding_client_rect();
                        (
                            e.client_x() as f64 - rect.left(),
                            e.client_y() as f64 - rect.top(),
                        )
                    })
                    .unwrap_or((e.offset_x() as f64, e.offset_y() as f64));
                let vp = viewport.get_untracked();
                let (wx, wy) = vp.screen_to_world(local.0, local.1);
                let hit = grid.borrow().find_at(wx, wy);
                if hit != hovered.get_untracked() {
                    hovered.set(hit);
                }
                if hovered.get_untracked().is_some() {
                    mouse_pos.set((e.client_x() as f64, e.client_y() as f64));
                }
            }
        }
    };

    let on_pointer_up = {
        let is_dragging = is_dragging.clone();
        let interaction_deadline = interaction_deadline.clone();
        move |e: PointerEvent| {
            is_dragging.set(false);
            interaction_deadline.set(js_sys::Date::now() + INTERACTION_SETTLE_MS);

            if let Some(target) = e.target()
                && let Ok(el) = target.dyn_into::<web_sys::HtmlElement>()
            {
                el.style().set_property("cursor", "grab").ok();
            }
        }
    };

    let on_click = {
        let drag_start_x = drag_start_x.clone();
        let drag_start_y = drag_start_y.clone();
        let grid = grid_for_click;
        move |e: MouseEvent| {
            let dx = (e.client_x() as f64 - drag_start_x.get()).abs();
            let dy = (e.client_y() as f64 - drag_start_y.get()).abs();
            if dx < 5.0 && dy < 5.0 {
                let local = gpu_canvas_ref
                    .get_untracked()
                    .map(|el| {
                        let rect = el.get_bounding_client_rect();
                        (
                            e.client_x() as f64 - rect.left(),
                            e.client_y() as f64 - rect.top(),
                        )
                    })
                    .unwrap_or((e.offset_x() as f64, e.offset_y() as f64));
                let vp = viewport.get_untracked();
                let (wx, wy) = vp.screen_to_world(local.0, local.1);
                let hit = grid.borrow().find_at(wx, wy);
                let hit_is_some = hit.is_some();
                if hit != selected.get_untracked() {
                    selected.set(hit);
                }
                if hit_is_some
                    && map_mode.get_untracked() == MapMode::Live
                    && !sidebar_open.get_untracked()
                {
                    sidebar_open.set(true);
                }
            }
        }
    };

    let on_pointer_leave = {
        move |_: PointerEvent| {
            if hovered.get_untracked().is_some() {
                hovered.set(None);
            }
        }
    };

    let on_touch_start = {
        let pinch_dist = pinch_dist.clone();
        let interaction_deadline = interaction_deadline.clone();
        move |e: web_sys::TouchEvent| {
            let touches = e.touches();
            if touches.length() == 2 {
                e.prevent_default();
                interaction_deadline.set(js_sys::Date::now() + INTERACTION_SETTLE_MS);
                let (Some(t0), Some(t1)) = (touches.get(0), touches.get(1)) else {
                    return;
                };
                let dx = (t1.client_x() - t0.client_x()) as f64;
                let dy = (t1.client_y() - t0.client_y()) as f64;
                pinch_dist.set((dx * dx + dy * dy).sqrt());
            }
        }
    };

    let on_touch_move = {
        let pinch_dist = pinch_dist.clone();
        let interaction_deadline = interaction_deadline.clone();
        move |e: web_sys::TouchEvent| {
            let touches = e.touches();
            if touches.length() == 2 {
                e.prevent_default();
                interaction_deadline.set(js_sys::Date::now() + INTERACTION_SETTLE_MS);
                let (Some(t0), Some(t1)) = (touches.get(0), touches.get(1)) else {
                    return;
                };
                let dx = (t1.client_x() - t0.client_x()) as f64;
                let dy = (t1.client_y() - t0.client_y()) as f64;
                let new_dist = (dx * dx + dy * dy).sqrt();
                let old_dist = pinch_dist.get();

                if old_dist > 0.0 {
                    let mid_x = (t0.client_x() + t1.client_x()) as f64 / 2.0;
                    let mid_y = (t0.client_y() + t1.client_y()) as f64 / 2.0;
                    let delta = -(new_dist - old_dist) * 2.0;
                    viewport.update(|vp| vp.zoom_at(delta, mid_x, mid_y));
                }

                pinch_dist.set(new_dist);
            }
        }
    };

    // Two-canvas stack
    view! {
        <div
            style="position: relative; width: 100%; height: 100%; overflow: hidden;"
            on:wheel=on_wheel
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=on_pointer_up
            on:pointerleave=on_pointer_leave
            on:click=on_click
            on:touchstart=on_touch_start
            on:touchmove=on_touch_move
        >
            <canvas
                node_ref=gpu_canvas_ref
                style="position: absolute; inset: 0; width: 100%; height: 100%; touch-action: none; image-rendering: pixelated; cursor: grab;"
            />
            <canvas
                node_ref=text_canvas_ref
                style=text_canvas_style
            />
        </div>
    }
}

// --- Text overlay rendering (Canvas 2D) ---

const TEXT_STYLE_ROLE_TAG: u8 = 1;
const TEXT_STYLE_ROLE_DETAIL: u8 = 2;

type TextWidthCache = HashMap<(String, u16, u8), f64>;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct NameFitKey {
    name_hash: u64,
    abbreviate: bool,
    detail_size_tenths: u16,
    detail_style_key: u8,
    avail_width_half_px: u16,
}

#[derive(Clone)]
struct NameFitValue {
    name: String,
    display_name: String,
}

type NameFitCache = HashMap<NameFitKey, NameFitValue>;

#[derive(Clone, Copy)]
struct NameFitStyle {
    detail_size_tenths: u16,
    detail_style_key: u8,
}

#[derive(Clone, Copy)]
struct TextOverlayStyle {
    show_connections: bool,
    abbreviate: bool,
    name_color: NameColor,
    show_countdown: bool,
    show_granular_map_time: bool,
    show_names: bool,
    bold_names: bool,
    bold_tags: bool,
    thick_tag_outline: bool,
    thick_name_outline: bool,
    readable_font: bool,
    bold_connections: bool,
}

struct TextOverlayInput<'a> {
    ctx: &'a CanvasRenderingContext2d,
    w: f64,
    h: f64,
    vp: &'a Viewport,
    territories: &'a ClientTerritoryMap,
    hovered: &'a Option<String>,
    selected: &'a Option<String>,
    reference_time_secs: i64,
    text_cache: &'a mut TextWidthCache,
    name_fit_cache: &'a mut NameFitCache,
    clear: bool,
    style: TextOverlayStyle,
    icons: &'a Option<ResourceIcons>,
}

#[derive(Clone, Copy)]
struct CanvasFallbackStyle {
    thick_cooldown_borders: bool,
    resource_highlight: bool,
}

struct Canvas2dFallbackInput<'a> {
    ctx: &'a CanvasRenderingContext2d,
    w: f64,
    h: f64,
    vp: &'a Viewport,
    territories: &'a ClientTerritoryMap,
    hovered: &'a Option<String>,
    selected: &'a Option<String>,
    reference_time_secs: i64,
    tiles: &'a [LoadedTile],
    world_bounds: Option<(f64, f64, f64, f64)>,
    style: CanvasFallbackStyle,
}

#[inline]
fn quantize_font_size_tenths(size_px: f64) -> u16 {
    (size_px * 10.0).round().clamp(1.0, u16::MAX as f64) as u16
}

#[inline]
fn quantize_width_half_px(width_px: f64) -> u16 {
    (width_px * 2.0).round().clamp(1.0, u16::MAX as f64) as u16
}

#[inline]
fn text_style_key(role: u8, readable_font: bool, bold: bool) -> u8 {
    role | ((readable_font as u8) << 6) | ((bold as u8) << 7)
}

fn measure_text_cached(
    ctx: &CanvasRenderingContext2d,
    text: &str,
    font_size_tenths: u16,
    style_key: u8,
    cache: &mut TextWidthCache,
) -> f64 {
    if let Some(&w) = cache.get(&(text.to_string(), font_size_tenths, style_key)) {
        return w;
    }
    let w = ctx.measure_text(text).map(|m| m.width()).unwrap_or(0.0);
    if cache.len() >= TEXT_WIDTH_CACHE_MAX_ENTRIES {
        cache.clear();
    }
    cache.insert((text.to_string(), font_size_tenths, style_key), w);
    w
}

fn fit_display_name_cached(
    ctx: &CanvasRenderingContext2d,
    name: &str,
    abbreviate: bool,
    avail_width: f64,
    style: NameFitStyle,
    text_cache: &mut TextWidthCache,
    name_fit_cache: &mut NameFitCache,
) -> String {
    let NameFitStyle {
        detail_size_tenths,
        detail_style_key,
    } = style;
    let key = NameFitKey {
        name_hash: hash_name(name),
        abbreviate,
        detail_size_tenths,
        detail_style_key,
        avail_width_half_px: quantize_width_half_px(avail_width.max(0.5)),
    };
    if let Some(cached) = name_fit_cache.get(&key)
        && cached.name == name
    {
        return cached.display_name.clone();
    }

    let base_name = if abbreviate {
        abbreviate_name(name)
    } else {
        name.to_string()
    };
    let name_w = measure_text_cached(
        ctx,
        &base_name,
        detail_size_tenths,
        detail_style_key,
        text_cache,
    );
    let display_name = if name_w <= avail_width {
        base_name
    } else {
        let mut trunc = base_name;
        let mut result = trunc.clone();
        while trunc.len() > 2 {
            trunc.pop();
            let candidate = format!("{trunc}\u{2026}");
            let tw = measure_text_cached(
                ctx,
                &candidate,
                detail_size_tenths,
                detail_style_key,
                text_cache,
            );
            if tw <= avail_width {
                result = candidate;
                break;
            }
        }
        result
    };

    if name_fit_cache.len() >= NAME_FIT_CACHE_MAX_ENTRIES {
        name_fit_cache.clear();
    }
    name_fit_cache.insert(
        key,
        NameFitValue {
            name: name.to_string(),
            display_name: display_name.clone(),
        },
    );
    display_name
}

/// Draw semi-transparent connection lines between connected territories.
fn render_connections(
    ctx: &CanvasRenderingContext2d,
    w: f64,
    h: f64,
    vp: &Viewport,
    territories: &ClientTerritoryMap,
    bold: bool,
) {
    use std::collections::HashSet;

    let mut drawn: HashSet<(u64, u64)> = HashSet::new();

    // Bold mode: per-edge guild-colored strokes (can't batch into one path)
    // Normal mode: single white batch path
    if !bold {
        ctx.set_stroke_style_str("rgba(255,255,255,0.12)");
        ctx.set_line_width(1.0);
        ctx.begin_path();
    }

    let mut css = String::with_capacity(40);

    for (name, ct) in territories {
        let name_hash = hash_name(name);
        let loc = &ct.territory.location;
        let (ax, ay) = vp.world_to_screen(loc.midpoint_x() as f64, loc.midpoint_y() as f64);

        for conn_name in &ct.territory.connections {
            let conn_hash = hash_name(conn_name);
            let edge = if name_hash < conn_hash {
                (name_hash, conn_hash)
            } else {
                (conn_hash, name_hash)
            };
            if !drawn.insert(edge) {
                continue;
            }

            let Some(conn_ct) = territories.get(conn_name) else {
                continue;
            };
            let conn_loc = &conn_ct.territory.location;
            let (bx, by) =
                vp.world_to_screen(conn_loc.midpoint_x() as f64, conn_loc.midpoint_y() as f64);

            // Off-screen culling: skip if both endpoints are off the same side
            let margin = 50.0;
            if (ax < -margin && bx < -margin)
                || (ay < -margin && by < -margin)
                || (ax > w + margin && bx > w + margin)
                || (ay > h + margin && by > h + margin)
            {
                continue;
            }

            if bold {
                // Adaptive brightness: dark guild colors get boosted more
                let (cr, cg, cb) = ct.guild_color;
                let lum = 0.299 * cr as f64 + 0.587 * cg as f64 + 0.114 * cb as f64;
                // dark_boost: 1.0 for very dark (lum~0), 0.0 for bright (lum~255)
                let dark_boost = (1.0 - lum / 255.0).clamp(0.0, 1.0);
                let brighten_factor = 1.4 + dark_boost * 0.8; // 1.4–2.2
                let line_alpha = 0.35 + dark_boost * 0.20; // 0.35–0.55
                let glow_alpha = 0.15 + dark_boost * 0.20; // 0.15–0.35
                let glow_blur = 5.0 + dark_boost * 4.0; // 5–9px
                let (r, g, b) = brighten(cr, cg, cb, brighten_factor);
                css.clear();
                let _ = std::fmt::Write::write_fmt(
                    &mut css,
                    format_args!("rgba({},{},{},{:.2})", r, g, b, glow_alpha),
                );
                ctx.set_shadow_color(&css);
                ctx.set_shadow_blur(glow_blur);
                css.clear();
                let _ = std::fmt::Write::write_fmt(
                    &mut css,
                    format_args!("rgba({},{},{},{:.2})", r, g, b, line_alpha),
                );
                ctx.set_stroke_style_str(&css);
                ctx.set_line_width(2.5);
                ctx.begin_path();
                ctx.move_to(ax, ay);
                ctx.line_to(bx, by);
                ctx.stroke();
            } else {
                ctx.move_to(ax, ay);
                ctx.line_to(bx, by);
            }
        }
    }

    if !bold {
        ctx.stroke();
    } else {
        ctx.set_shadow_color("transparent");
        ctx.set_shadow_blur(0.0);
    }
}

/// Quick hash for deduplicating connection edges.
fn hash_name(name: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in name.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    h
}

/// Draw territory labels with a cartographic text-halo technique.
///
/// Two-pass rendering per text element: a thick dark stroke + shadow glow
/// provides guaranteed contrast over any map tile, then bright fill on top.
/// Guild tags use the guild's own color (brightened) for instant identification.
///
/// Zoom tiers:
///   - **Any zoom** (territory >= 18px wide): Guild tag centered
///   - **Medium** (>= 55px wide, 25px tall): Tag + relative time below
///   - **Large** (>= 100px wide, 45px tall): Tag + territory name + time
///   - **Cooldown**: Always rendered, gold countdown timer
fn render_text_overlay(input: TextOverlayInput<'_>) {
    let TextOverlayInput {
        ctx,
        w,
        h,
        vp,
        territories,
        hovered: _hovered,
        selected: _selected,
        reference_time_secs,
        text_cache,
        name_fit_cache,
        clear,
        style,
        icons,
    } = input;
    let TextOverlayStyle {
        show_connections,
        abbreviate,
        name_color,
        show_countdown,
        show_granular_map_time,
        show_names,
        bold_names,
        bold_tags,
        thick_tag_outline,
        thick_name_outline,
        readable_font,
        bold_connections,
    } = style;

    if clear {
        ctx.clear_rect(0.0, 0.0, w, h);
    }

    // Draw connection lines first (behind text)
    if show_connections {
        render_connections(ctx, w, h, vp, territories, bold_connections);
    }

    let zoom_out_boost = if vp.scale < ZOOM_OUT_TEXT_BOOST_START {
        let t = ((ZOOM_OUT_TEXT_BOOST_START - vp.scale)
            / (ZOOM_OUT_TEXT_BOOST_START - ZOOM_OUT_TEXT_BOOST_END))
            .clamp(0.0, 1.0);
        1.0 + (ZOOM_OUT_TEXT_BOOST_MAX - 1.0) * t
    } else {
        1.0
    };
    let overlay_scale = TERRITORY_OVERLAY_SCALE * zoom_out_boost;

    let base_tag = 32.0 * vp.scale * overlay_scale;
    let base_detail = 11.0 * vp.scale * overlay_scale;
    let base_time = 18.0 * vp.scale * overlay_scale;
    let base_cooldown = 13.0 * vp.scale * overlay_scale;
    let base_gap = 3.0 * vp.scale * overlay_scale;

    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    ctx.set_line_join("round");

    // Reusable buffer to avoid per-territory heap allocations for CSS color strings
    let mut css = String::with_capacity(40);
    let tag_style_key = text_style_key(TEXT_STYLE_ROLE_TAG, readable_font, bold_tags);
    let detail_style_key = text_style_key(TEXT_STYLE_ROLE_DETAIL, readable_font, bold_names);

    for (name, ct) in territories {
        let loc = &ct.territory.location;
        let (sx, sy) = vp.world_to_screen(loc.left() as f64, loc.top() as f64);
        let sw = loc.width() as f64 * vp.scale;
        let sh = loc.height() as f64 * vp.scale;

        // Hard cull: at this size text is unreadable regardless of state
        if sw < 10.0 || sh < 8.0 {
            continue;
        }

        // Off-screen cull (slight margin for shadow bleed)
        if sx + sw < -20.0 || sy + sh < -20.0 || sx > w + 20.0 || sy > h + 20.0 {
            continue;
        }

        let acquired_secs = ct.territory.acquired.timestamp();
        let age_secs = (reference_time_secs - acquired_secs).max(0);
        let is_fresh = age_secs < 600;
        let cooldown_frac = if is_fresh {
            ((600 - age_secs) as f64 / 600.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let cx = sx + sw / 2.0;
        let cy = sy + sh / 2.0;
        let tag = &ct.territory.guild.prefix;

        let is_large = sw > 100.0 && sh > 45.0;
        let is_medium = !is_large && sw > 18.0 && (sh > 30.0 || (sw > 30.0 && sh > 16.0));
        let is_tiny = sw < 18.0 || sh < 12.0;

        // Per-territory max: text caps based on territory width, absolute ceiling 26px.
        // Guild tags are fit-to-box so they stay visible at tiny zoom levels.
        let max_tag = (sw * 0.35).min(26.0);
        let mut tag_size = base_tag.clamp(5.0, max_tag.max(5.0));
        let detail_size = base_detail.clamp(7.5, (max_tag * 0.7).max(7.5));
        let time_size_full = base_time.clamp(8.0, (max_tag * 0.85).max(8.0));
        let time_size = if is_fresh {
            time_size_full
        } else {
            time_size_full * 0.55
        };
        let cooldown_size = base_cooldown.clamp(7.5, (max_tag * 0.6).max(7.5));
        let line_gap = base_gap.clamp(2.0, (max_tag * 0.25).max(2.0));

        let font_family = if readable_font {
            "'Inter', system-ui, sans-serif"
    } else {
        "'SilkscreenLocal', monospace"
    };
        let tag_weight = if bold_tags { "700" } else { "400" };
        let detail_weight = if bold_names { "700" } else { "400" };
        let detail_font = format!("{} {:.1}px {}", detail_weight, detail_size, font_family);
        let time_font = format!("{:.1}px {}", time_size, font_family);
        let cooldown_font = format!("700 {:.1}px {}", cooldown_size, font_family);
        let mut tag_font = format!("{} {:.1}px {}", tag_weight, tag_size, font_family);

        let tag_avail_w = (sw - if is_tiny { 3.0 } else { 8.0 }).max(4.0);
        let tag_avail_h = if is_large || is_medium {
            (sh * 0.42).max(6.0)
        } else {
            (sh - 2.0).max(4.0)
        };
        ctx.set_font(&tag_font);
        let tag_key = quantize_font_size_tenths(tag_size);
        let tag_w = measure_text_cached(ctx, tag, tag_key, tag_style_key, text_cache);
        if tag_w > tag_avail_w {
            tag_size *= (tag_avail_w / tag_w).clamp(0.35, 1.0);
        }
        tag_size = tag_size.min(tag_avail_h * 0.82).max(4.5);
        tag_font = format!("{} {:.1}px {}", tag_weight, tag_size, font_family);

        // Guild color — brightened for text readability over dark map
        let (gr, gg, gb) = brighten(ct.guild_color.0, ct.guild_color.1, ct.guild_color.2, 1.6);
        let mut lines = [0.0f64; 3];
        let line_count;

        if is_large || is_medium {
            let total_h = tag_size + detail_size + time_size + line_gap * 2.0;
            let top_y = cy - total_h / 2.0;
            lines[0] = top_y + tag_size / 2.0;
            lines[1] = top_y + tag_size + line_gap + detail_size / 2.0;
            lines[2] = top_y + tag_size + line_gap + detail_size + line_gap + time_size / 2.0;
            line_count = 3;
        } else {
            lines[0] = cy;
            line_count = 1;
        }

        // Stroke-only halo: thick dark outline provides contrast without expensive shadow blur.
        let halo_w = if thick_tag_outline {
            (tag_size * 0.30).clamp(0.9, 5.5)
        } else {
            (tag_size * 0.18).clamp(0.6, 3.5)
        };
        ctx.set_stroke_style_str("rgba(8, 10, 18, 0.92)");

        // Line 0: Guild tag — rendered in the guild's own color
        let tag_y = lines[0];
        ctx.set_font(&tag_font);
        ctx.set_line_width(halo_w);
        ctx.stroke_text(tag, cx, tag_y).ok();
        css.clear();
        let _ = write!(css, "rgb({},{},{})", gr, gg, gb);
        ctx.set_fill_style_str(&css);
        ctx.fill_text(tag, cx, tag_y).ok();

        // Line 1 (medium/large): territory name (truncated to fit)
        if line_count >= 3 && show_names {
            let line1_y = lines[1];
            ctx.set_font(&detail_font);
            let detail_halo = if thick_name_outline {
                (detail_size * 0.30).clamp(2.0, 5.0)
            } else {
                (detail_size * 0.18).clamp(1.0, 3.0)
            };
            ctx.set_line_width(detail_halo);
            ctx.set_stroke_style_str("rgba(8, 10, 18, 0.9)");

            let avail = sw - 10.0;
            let detail_key = quantize_font_size_tenths(detail_size);
            let display_name = fit_display_name_cached(
                ctx,
                name,
                abbreviate,
                avail,
                NameFitStyle {
                    detail_size_tenths: detail_key,
                    detail_style_key,
                },
                text_cache,
                name_fit_cache,
            );

            ctx.stroke_text(&display_name, cx, line1_y).ok();
            match name_color {
                NameColor::White => ctx.set_fill_style_str("rgba(220, 218, 210, 0.88)"),
                NameColor::Guild => {
                    css.clear();
                    let _ = write!(css, "rgba({},{},{},0.88)", gr, gg, gb);
                    ctx.set_fill_style_str(&css);
                }
                NameColor::Gold => ctx.set_fill_style_str("rgba(245, 197, 66, 0.88)"),
                NameColor::Copper => ctx.set_fill_style_str("rgba(181, 103, 39, 0.88)"),
                NameColor::Muted => ctx.set_fill_style_str("rgba(120, 116, 112, 0.78)"),
            }
            ctx.fill_text(&display_name, cx, line1_y).ok();
        }

        // Line 2 (medium/large): relative time
        if line_count >= 3 {
            let line2_y = lines[2];
            ctx.set_font(&time_font);
            let time_halo = (time_size * 0.18).clamp(1.0, 3.0);
            ctx.set_line_width(time_halo);
            ctx.set_stroke_style_str("rgba(8, 10, 18, 0.9)");

            let rel = if show_granular_map_time {
                format_hms(age_secs)
            } else {
                format_age(age_secs)
            };
            ctx.stroke_text(&rel, cx, line2_y).ok();
            if is_fresh {
                let urgency = 1.0 - cooldown_frac;
                let (cr, cg, cb) = cooldown_color(urgency);
                css.clear();
                let _ = write!(css, "rgba({},{},{},0.95)", cr, cg, cb);
                ctx.set_fill_style_str(&css);
            } else {
                let (tr, tg, tb) = TreasuryLevel::from_held_seconds(age_secs).color_rgb();
                css.clear();
                let _ = write!(css, "rgba({},{},{},0.80)", tr, tg, tb);
                ctx.set_fill_style_str(&css);
            }
            ctx.fill_text(&rel, cx, line2_y).ok();
        }

        // --- Cooldown countdown (opt-in via settings) ---
        if is_fresh && show_countdown {
            let remaining = 600 - age_secs;
            css.clear();
            let _ = write!(css, "{}:{:02}", remaining / 60, remaining % 60);

            let cd_y = if line_count > 0 {
                lines[line_count - 1] + detail_size / 2.0 + cooldown_size / 2.0 + 4.0
            } else {
                cy + tag_size / 2.0 + cooldown_size / 2.0 + 3.0
            };

            ctx.set_font(&cooldown_font);
            ctx.set_line_width((cooldown_size * 0.18).clamp(1.0, 3.0));
            ctx.set_stroke_style_str("rgba(8, 10, 18, 0.9)");
            ctx.stroke_text(&css, cx, cd_y).ok();
            let urgency = 1.0 - cooldown_frac;
            let cd_alpha = 0.88 + urgency * 0.12;
            let (cr, cg, cb) = cooldown_color(urgency);
            // Need separate string since css has the countdown text
            let cd_css = format!("rgba({},{},{},{:.2})", cr, cg, cb, cd_alpha);
            ctx.set_fill_style_str(&cd_css);
            ctx.fill_text(&css, cx, cd_y).ok();
        }

        // --- Resource icons (only for sufficiently large territories) ---
        if sw > 55.0
            && sh > 35.0
            && !ct.territory.resources.is_empty()
            && let Some(ic) = icons
        {
            let res = &ct.territory.resources;
            let icon_size = (14.0 * vp.scale * overlay_scale).clamp(8.0, 24.0);
            let icon_gap = icon_size * 1.3;

            // Build list of icons to draw
            let mut icon_refs: Vec<&web_sys::HtmlImageElement> = Vec::with_capacity(10);

            if res.has_all() {
                // All 5 resources → single rainbow star
                icon_refs.push(&ic.rainbow);
            } else {
                let res_icons: [(i32, bool, &web_sys::HtmlImageElement); 5] = [
                    (res.emeralds, res.has_double_emeralds(), &ic.emerald),
                    (res.ore, res.has_double_ore(), &ic.ore),
                    (res.crops, res.has_double_crops(), &ic.crops),
                    (res.fish, res.has_double_fish(), &ic.fish),
                    (res.wood, res.has_double_wood(), &ic.wood),
                ];
                for &(val, is_double, img) in &res_icons {
                    if val > 0 {
                        icon_refs.push(img);
                        if is_double {
                            icon_refs.push(img);
                        }
                    }
                }
            }

            let count = icon_refs.len();
            if count > 0 {
                let total_w = (count as f64 - 1.0) * icon_gap + icon_size;
                let mut dx = cx - total_w / 2.0;

                // Position below last content
                let icon_y = if is_fresh {
                    let cd_y = if line_count > 0 {
                        lines[line_count - 1] + detail_size / 2.0 + cooldown_size / 2.0 + 4.0
                    } else {
                        cy + tag_size / 2.0 + cooldown_size / 2.0 + 3.0
                    };
                    cd_y + cooldown_size / 2.0 + icon_size / 2.0 + 3.0
                } else if line_count > 0 {
                    lines[line_count - 1] + detail_size / 2.0 + icon_size / 2.0 + 4.0
                } else {
                    cy + tag_size / 2.0 + icon_size / 2.0 + 3.0
                };

                ctx.set_image_smoothing_enabled(false);
                for img in &icon_refs {
                    ctx.draw_image_with_html_image_element_and_dw_and_dh(
                        img,
                        dx,
                        icon_y - icon_size / 2.0,
                        icon_size,
                        icon_size,
                    )
                    .ok();
                    dx += icon_gap;
                }
                ctx.set_image_smoothing_enabled(true);
            }
        }
    }
}

/// Abbreviate a territory name to its initials (e.g. "Cascading Basins" → "CB").
/// Single-word names (e.g. "Ragni", "Detlas") are kept in full.
pub fn abbreviate_name(name: &str) -> String {
    if !name.contains(' ') {
        return name.to_string();
    }
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect()
}

/// Format seconds-since-acquisition into a compact string.
fn format_age(age_secs: i64) -> String {
    if age_secs < 60 {
        "now".to_string()
    } else if age_secs < 600 {
        format!("{}:{:02}", age_secs / 60, age_secs % 60)
    } else if age_secs < 3600 {
        format!("{}m", age_secs / 60)
    } else if age_secs < 86400 {
        format!("{}h", age_secs / 3600)
    } else if age_secs < 604800 {
        format!("{}d", age_secs / 86400)
    } else {
        format!("{}w", age_secs / 604800)
    }
}

/// 4-step cooldown color: green → yellow → orange → red at 2.5m intervals.
/// `urgency` goes from 0.0 (just captured) to 1.0 (cooldown expiring).
fn cooldown_color(urgency: f64) -> (u8, u8, u8) {
    if urgency < 0.25 {
        (102, 204, 102) // green  (0–2.5m)
    } else if urgency < 0.50 {
        (245, 197, 66) // yellow (2.5–5m)
    } else if urgency < 0.75 {
        (245, 158, 66) // orange (5–7.5m)
    } else {
        (235, 87, 87) // red    (7.5–10m)
    }
}

// --- Resource highlight colors (matching shader LUT) ---

const RESOURCE_COLORS: [(u8, u8, u8); 5] = [
    (0x5c, 0xb8, 0x5c), // 0: unused/fallback
    (0xe7, 0x8b, 0xc8), // 1: ore
    (0xe8, 0xb6, 0x35), // 2: crops
    (0x5d, 0x8f, 0xdb), // 3: fish
    (0x5c, 0xb8, 0x5c), // 4: wood
];

fn draw_resource_fill(
    ctx: &CanvasRenderingContext2d,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    data: [f32; 4],
    fill_alpha: f64,
) {
    let mode = data[0] as i32;
    let flags = data[3] as u32;
    let has_dbl_em = (flags & (1 << 10)) != 0;

    match mode {
        1 => {
            // Solid single resource
            let idx = data[1] as usize;
            let (cr, cg, cb) = RESOURCE_COLORS[idx.min(4)];
            ctx.set_fill_style_str(&rgba_css(cr, cg, cb, fill_alpha));
            ctx.fill_rect(sx, sy, sw, sh);
            if (flags & 1) != 0 {
                draw_hatch(ctx, sx, sy, sw, sh, 0.55);
            }
        }
        2 => {
            // Diagonal split — two resources
            let idx_a = data[1] as usize;
            let idx_b = data[2] as usize;
            let (ra, ga, ba) = RESOURCE_COLORS[idx_a.min(4)];
            let (rb, gb, bb) = RESOURCE_COLORS[idx_b.min(4)];

            // Top-left triangle
            ctx.save();
            ctx.begin_path();
            ctx.move_to(sx, sy);
            ctx.line_to(sx + sw, sy);
            ctx.line_to(sx, sy + sh);
            ctx.close_path();
            ctx.clip();
            ctx.set_fill_style_str(&rgba_css(ra, ga, ba, fill_alpha));
            ctx.fill_rect(sx, sy, sw, sh);
            if (flags & 1) != 0 {
                draw_hatch(ctx, sx, sy, sw, sh, 0.55);
            }
            ctx.restore();

            // Bottom-right triangle
            ctx.save();
            ctx.begin_path();
            ctx.move_to(sx + sw, sy);
            ctx.line_to(sx + sw, sy + sh);
            ctx.line_to(sx, sy + sh);
            ctx.close_path();
            ctx.clip();
            ctx.set_fill_style_str(&rgba_css(rb, gb, bb, fill_alpha));
            ctx.fill_rect(sx, sy, sw, sh);
            if (flags & 2) != 0 {
                draw_hatch(ctx, sx, sy, sw, sh, 0.55);
            }
            ctx.restore();
        }
        3 => {
            // Multi-stripe
            let stripe_mask = flags & 31;
            let double_mask = (flags >> 5) & 31;
            let n = stripe_mask.count_ones() as usize;
            if n == 0 && !has_dbl_em {
                return;
            }

            let diag_len = sw + sh;
            for i in 0..n {
                let res_idx = nth_set_bit_canvas(stripe_mask, i);
                let (cr, cg, cb) = RESOURCE_COLORS[res_idx.min(4)];
                let band_start = diag_len * (i as f64) / (n as f64);
                let band_end = diag_len * ((i + 1) as f64) / (n as f64);

                ctx.save();
                ctx.begin_path();
                clip_diagonal_band(ctx, sx, sy, sw, sh, band_start, band_end);
                ctx.clip();
                ctx.set_fill_style_str(&rgba_css(cr, cg, cb, fill_alpha));
                ctx.fill_rect(sx, sy, sw, sh);
                if (double_mask & (1 << res_idx)) != 0 {
                    draw_hatch(ctx, sx, sy, sw, sh, 0.55);
                }
                ctx.restore();
            }
        }
        _ => {}
    }

    // Green checker overlay for double-emerald territories
    if has_dbl_em {
        draw_emerald_checker(ctx, sx, sy, sw, sh, 0.7);
    }
}

fn draw_hatch(ctx: &CanvasRenderingContext2d, sx: f64, sy: f64, sw: f64, sh: f64, alpha: f64) {
    ctx.save();
    ctx.begin_path();
    ctx.rect(sx, sy, sw, sh);
    ctx.clip();
    ctx.set_stroke_style_str(&rgba_css(255, 255, 255, alpha));
    ctx.set_line_width(3.0);
    let period = 10.0;
    let diag = sw + sh;
    let mut d = -sh;
    while d < diag {
        // Perpendicular diagonal: lines where px.x - px.y = const
        ctx.begin_path();
        ctx.move_to(sx + d, sy);
        ctx.line_to(sx + d + sh, sy + sh);
        ctx.stroke();
        d += period;
    }
    ctx.restore();
}

fn draw_emerald_checker(
    ctx: &CanvasRenderingContext2d,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    alpha: f64,
) {
    ctx.save();
    ctx.begin_path();
    ctx.rect(sx, sy, sw, sh);
    ctx.clip();
    let period = 12.0;
    let diag = sw + sh;
    // Dark outline pass first (wider, behind the green stripe)
    ctx.set_stroke_style_str(&rgba_css(0x05, 0x14, 0x05, alpha));
    ctx.set_line_width(10.0);
    let mut d = 0.0;
    while d < diag {
        ctx.begin_path();
        ctx.move_to(sx + d, sy);
        ctx.line_to(sx, sy + d);
        ctx.stroke();
        d += period;
    }
    // Green stripe pass on top
    ctx.set_stroke_style_str(&rgba_css(0x2e, 0x8b, 0x2e, alpha));
    ctx.set_line_width(5.0);
    d = 0.0;
    while d < diag {
        ctx.begin_path();
        ctx.move_to(sx + d, sy);
        ctx.line_to(sx, sy + d);
        ctx.stroke();
        d += period;
    }
    ctx.restore();
}

fn nth_set_bit_canvas(mask: u32, n: usize) -> usize {
    let mut found = 0;
    for i in 0..5 {
        if (mask & (1 << i)) != 0 {
            if found == n {
                return i;
            }
            found += 1;
        }
    }
    0
}

fn clip_diagonal_band(
    ctx: &CanvasRenderingContext2d,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    band_start: f64,
    band_end: f64,
) {
    // Diagonal coordinate: (x - sx) + (y - sy) maps to 0..sw+sh
    // Band: band_start <= (x-sx)+(y-sy) <= band_end
    // Build polygon by intersecting band with rectangle
    let corners = [
        (sx, sy, 0.0),               // top-left, diag=0
        (sx + sw, sy, sw),           // top-right, diag=sw
        (sx + sw, sy + sh, sw + sh), // bottom-right, diag=sw+sh
        (sx, sy + sh, sh),           // bottom-left, diag=sh
    ];

    // Collect polygon vertices where band intersects rect edges
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(8);

    // Add corners inside the band
    for &(cx, cy, d) in &corners {
        if d >= band_start && d <= band_end {
            pts.push((cx, cy));
        }
    }

    // Add edge intersections
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0)];
    for &(i, j) in &edges {
        let (x0, y0, d0) = corners[i];
        let (x1, y1, d1) = corners[j];
        for &boundary in &[band_start, band_end] {
            if (d0 < boundary && d1 > boundary) || (d0 > boundary && d1 < boundary) {
                let t = (boundary - d0) / (d1 - d0);
                pts.push((x0 + t * (x1 - x0), y0 + t * (y1 - y0)));
            }
        }
    }

    if pts.len() < 3 {
        // Degenerate — just use full rect as fallback
        ctx.rect(sx, sy, sw, sh);
        return;
    }

    // Sort by angle from centroid for convex polygon
    let cx_avg = pts.iter().map(|p| p.0).sum::<f64>() / pts.len() as f64;
    let cy_avg = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
    pts.sort_by(|a, b| {
        let angle_a = (a.1 - cy_avg).atan2(a.0 - cx_avg);
        let angle_b = (b.1 - cy_avg).atan2(b.0 - cx_avg);
        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    ctx.begin_path();
    ctx.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        ctx.line_to(p.0, p.1);
    }
    ctx.close_path();
}

// --- Canvas 2D fallback (when wgpu is unavailable or not yet initialized) ---

fn render_canvas2d_fallback(input: Canvas2dFallbackInput<'_>) {
    let Canvas2dFallbackInput {
        ctx,
        w,
        h,
        vp,
        territories,
        hovered,
        selected,
        reference_time_secs,
        tiles,
        world_bounds,
        style,
    } = input;
    let CanvasFallbackStyle {
        thick_cooldown_borders,
        resource_highlight,
    } = style;

    ctx.set_fill_style_str("#0c0e17");
    ctx.fill_rect(0.0, 0.0, w, h);

    ctx.set_image_smoothing_enabled(false);
    for tile in tiles {
        let x1 = tile.x1.min(tile.x2) as f64;
        let z1 = tile.z1.min(tile.z2) as f64;
        // Tile bounds are inclusive — add 1 to get exclusive far edge
        let x2 = tile.x1.max(tile.x2) as f64 + 1.0;
        let z2 = tile.z1.max(tile.z2) as f64 + 1.0;

        if let Some((bx1, by1, bx2, by2)) = world_bounds {
            let margin = 300.0;
            if x2 < bx1 - margin || x1 > bx2 + margin || z2 < by1 - margin || z1 > by2 + margin {
                continue;
            }
        }

        let (sx, sy) = vp.world_to_screen(x1, z1);
        let (ex, ey) = vp.world_to_screen(x2, z2);
        // Snap to pixel grid: floor start, ceil end — guarantees 0-1px overlap
        // between adjacent tiles, preventing sub-pixel canvas edge artifacts
        let sx = sx.floor();
        let sy = sy.floor();
        let sw = ex.ceil() - sx;
        let sh = ey.ceil() - sy;

        if sx + sw < 0.0 || sy + sh < 0.0 || sx > w || sy > h {
            continue;
        }

        ctx.draw_image_with_html_image_element_and_dw_and_dh(&tile.image, sx, sy, sw, sh)
            .ok();
    }
    ctx.set_image_smoothing_enabled(true);

    if !tiles.is_empty() {
        ctx.set_fill_style_str("rgba(12, 14, 23, 0.28)");
        ctx.fill_rect(0.0, 0.0, w, h);
    }

    let now = js_sys::Date::now();

    let cooldowns: HashMap<&str, f64> = territories
        .iter()
        .map(|(name, ct)| {
            let acquired_secs = ct.territory.acquired.timestamp();
            let age_secs = (reference_time_secs - acquired_secs).max(0);
            let cooldown_frac = if age_secs < 600 {
                ((600 - age_secs) as f64 / 600.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (name.as_str(), cooldown_frac)
        })
        .collect();

    for (name, ct) in territories {
        let loc = &ct.territory.location;
        let (sx, sy) = vp.world_to_screen(loc.left() as f64, loc.top() as f64);
        let sw = loc.width() as f64 * vp.scale;
        let sh = loc.height() as f64 * vp.scale;

        if sx + sw < 0.0 || sy + sh < 0.0 || sx > w || sy > h {
            continue;
        }

        let animating_color = ct.animation.as_ref().and_then(|a| a.current_color(now));
        let (r, g, b) = animating_color.unwrap_or(ct.guild_color);
        let is_hovered = hovered.as_deref() == Some(name.as_str());
        let is_selected = selected.as_deref() == Some(name.as_str());

        // Resource highlight: multi-resource diagonal splits
        let res_data = if resource_highlight {
            ct.territory.resources.highlight_data()
        } else {
            [0.0; 4]
        };
        let has_resource_fill = res_data[0] > 0.5 || (res_data[3] as u32 & (1 << 10)) != 0; // mode 0 + double emeralds

        if has_resource_fill {
            let fill_alpha = if is_selected {
                0.48
            } else if is_hovered {
                0.40
            } else {
                0.30
            };
            draw_resource_fill(ctx, sx, sy, sw, sh, res_data, fill_alpha);
        } else if animating_color.is_some() {
            let fill_alpha = if is_selected {
                0.35
            } else if is_hovered {
                0.30
            } else {
                0.22
            };
            ctx.set_fill_style_str(&rgba_css(r, g, b, fill_alpha));
        } else if is_selected {
            ctx.set_fill_style_str(&ct.cached_colors.fill_selected);
        } else if is_hovered {
            ctx.set_fill_style_str(&ct.cached_colors.fill_hovered);
        } else {
            ctx.set_fill_style_str(&ct.cached_colors.fill_normal);
        }
        if !has_resource_fill {
            ctx.fill_rect(sx, sy, sw, sh);
        }

        if let Some(ref anim) = ct.animation {
            let flash = anim.flash_intensity(now);
            if flash > 0.0 {
                ctx.set_fill_style_str(&rgba_css(255, 217, 102, flash * 0.6));
                ctx.fill_rect(sx, sy, sw, sh);
            }
        }

        let cooldown_frac_here = cooldowns.get(name.as_str()).copied().unwrap_or(0.0);
        let border_width = if thick_cooldown_borders && cooldown_frac_here > 0.0 {
            6.0
        } else {
            4.0
        };
        ctx.set_shadow_color("transparent");
        ctx.set_shadow_blur(0.0);

        if is_selected {
            ctx.set_stroke_style_str(&rgba_css(r, g, b, 0.80));
        } else if is_hovered {
            ctx.set_stroke_style_str(&rgba_css(r, g, b, 0.75));
        } else if animating_color.is_none() {
            ctx.set_stroke_style_str(&ct.cached_colors.border_normal);
        } else {
            ctx.set_stroke_style_str(&rgba_css(r, g, b, 0.65));
        }
        ctx.set_line_width(border_width);
        ctx.stroke_rect(sx, sy, sw, sh);

        if vp.scale > 0.3 && sw > 40.0 && sh > 40.0 {
            ctx.set_stroke_style_str(&rgba_css(255, 255, 255, 0.06));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(sx + 1.0, sy + sh - 1.0);
            ctx.line_to(sx + 1.0, sy + 1.0);
            ctx.line_to(sx + sw - 1.0, sy + 1.0);
            ctx.stroke();

            ctx.set_stroke_style_str(&rgba_css(0, 0, 0, 0.15));
            ctx.begin_path();
            ctx.move_to(sx + sw - 1.0, sy + 1.0);
            ctx.line_to(sx + sw - 1.0, sy + sh - 1.0);
            ctx.line_to(sx + 1.0, sy + sh - 1.0);
            ctx.stroke();
        }

        // Selection handled by fill/border alpha bump — no purple glow overlay

        // Hover has no extra glow — just the fill/border alpha bump above

        let cooldown_frac = cooldowns.get(name.as_str()).copied().unwrap_or(0.0);
        if cooldown_frac > 0.0 {
            let strip_h = (sh * 0.08).clamp(3.0, 6.0);
            let strip_w = sw * (1.0 - cooldown_frac);
            let urgency = 1.0 - cooldown_frac;
            let alpha = 0.4 + urgency * 0.5;
            let (cr, cg, cb) = cooldown_color(urgency);
            ctx.set_fill_style_str(&format!("rgba({},{},{},{:.2})", cr, cg, cb, alpha));
            ctx.fill_rect(sx, sy + sh - strip_h, strip_w, strip_h);
        }
    }
}
