mod animation;
mod app;
mod canvas;
mod colors;
#[cfg(target_arch = "wasm32")]
mod gpu;
mod history;
mod icons;
mod minimap;
mod playback;
mod render_loop;
mod sidebar;
mod spatial;
mod sse;
mod territory;
mod tiles;
mod time_format;
mod timeline;
mod tower;
mod viewport;

#[cfg(not(target_arch = "wasm32"))]
mod gpu {
    use crate::territory::ClientTerritoryMap;
    use crate::tiles::LoadedTile;
    use crate::viewport::Viewport;

    pub struct RenderFrameInput<'a> {
        pub vp: &'a Viewport,
        pub territories: &'a ClientTerritoryMap,
        pub hovered: &'a Option<String>,
        pub selected: &'a Option<String>,
        pub tiles: &'a [LoadedTile],
        pub world_bounds: Option<(f64, f64, f64, f64)>,
        pub now: f64,
        pub reference_time_secs: i64,
    }

    pub struct GpuRenderer {
        pub thick_cooldown_borders: bool,
        pub resource_highlight: bool,
    }

    impl GpuRenderer {
        pub async fn init(_canvas: web_sys::HtmlCanvasElement) -> Result<Self, String> {
            Err("not wasm".into())
        }
        pub fn mark_instance_dirty(&mut self) {}
        pub fn resize(&mut self, _w: u32, _h: u32, _dpr: f32) {}
        pub fn upload_tiles(&mut self, _tiles: &[LoadedTile]) {}
        pub fn render(&mut self, frame: RenderFrameInput<'_>) -> bool {
            let _ = (
                frame.vp,
                frame.territories,
                frame.hovered,
                frame.selected,
                frame.tiles,
                frame.world_bounds,
                frame.now,
                frame.reference_time_secs,
            );
            false
        }
    }
}

use leptos::mount::mount_to;
use std::any::Any;
use std::cell::RefCell;
use wasm_bindgen::JsCast;

thread_local! {
    static APP_MOUNT_HANDLE: RefCell<Option<Box<dyn Any>>> = RefCell::new(None);
}

fn main() {
    console_error_panic_hook::set_once();
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let mount_target = document
        .get_element_by_id("app")
        .and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
        .or_else(|| document.body());
    let Some(target) = mount_target else {
        return;
    };

    APP_MOUNT_HANDLE.with(move |slot| {
        // If main() is re-entered (e.g. dev/hot-reload runtime quirks), drop the old mount
        // so stale effects/signals can't keep mutating app state.
        let _old = slot.borrow_mut().take();
        let handle = mount_to(target, app::App);
        *slot.borrow_mut() = Some(Box::new(handle));
    });
}
