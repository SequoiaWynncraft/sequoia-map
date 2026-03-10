#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use js_sys::Reflect;
use wasm_bindgen::JsValue;

pub(crate) const ASSET_BASE_KEY: &str = "__SEQUOIA_ASSET_BASE__";

fn normalize_asset_base(base: &str) -> String {
    let trimmed = base.trim();
    if trimmed.is_empty() || trimmed == "/" {
        String::new()
    } else {
        format!("/{}", trimmed.trim_matches('/'))
    }
}

fn join_asset_url(base: &str, path: &str) -> String {
    let trimmed_path = path.trim().trim_start_matches('/');
    if trimmed_path.is_empty() {
        return if base.is_empty() {
            "/".to_string()
        } else {
            normalize_asset_base(base)
        };
    }

    let normalized_base = normalize_asset_base(base);
    if normalized_base.is_empty() {
        format!("/{trimmed_path}")
    } else {
        format!("{normalized_base}/{trimmed_path}")
    }
}

pub(crate) fn asset_base_path() -> String {
    web_sys::window()
        .and_then(|window| Reflect::get(window.as_ref(), &JsValue::from_str(ASSET_BASE_KEY)).ok())
        .and_then(|value| value.as_string())
        .map(|base| normalize_asset_base(&base))
        .unwrap_or_default()
}

pub(crate) fn app_asset_url(path: &str) -> String {
    join_asset_url(&asset_base_path(), path)
}

#[cfg(test)]
mod tests {
    use super::{join_asset_url, normalize_asset_base};

    #[test]
    fn normalize_asset_base_collapses_root_and_slashes() {
        assert_eq!(normalize_asset_base(""), "");
        assert_eq!(normalize_asset_base("/"), "");
        assert_eq!(normalize_asset_base("claims-app"), "/claims-app");
        assert_eq!(normalize_asset_base("/claims-app/"), "/claims-app");
    }

    #[test]
    fn join_asset_url_respects_optional_base_prefix() {
        assert_eq!(
            join_asset_url("", "icons/crown_icon.png"),
            "/icons/crown_icon.png"
        );
        assert_eq!(
            join_asset_url("/claims-app/", "/icons/crown_icon.png"),
            "/claims-app/icons/crown_icon.png"
        );
    }
}
