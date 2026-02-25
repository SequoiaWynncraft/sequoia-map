use leptos::prelude::*;
use web_sys::HtmlImageElement;

#[derive(Clone)]
pub struct ResourceIcons {
    pub emerald: HtmlImageElement,
    pub ore: HtmlImageElement,
    pub crops: HtmlImageElement,
    pub fish: HtmlImageElement,
    pub wood: HtmlImageElement,
    pub rainbow: HtmlImageElement,
}

pub fn load_resource_icons(signal: RwSignal<Option<ResourceIcons>>) {
    wasm_bindgen_futures::spawn_local(async move {
        let names = ["emerald", "ore", "crops", "fish", "wood", "rainbow"];
        let mut images: Vec<HtmlImageElement> = Vec::with_capacity(6);

        for name in &names {
            let Ok(img) = HtmlImageElement::new() else {
                web_sys::console::warn_1(
                    &format!("Failed to create icon element for {name}").into(),
                );
                return;
            };
            img.set_src(&format!("/icons/{}.svg", name));
            match wasm_bindgen_futures::JsFuture::from(img.decode()).await {
                Ok(_) => images.push(img),
                Err(e) => {
                    web_sys::console::warn_1(
                        &format!("Failed to load icon {}: {:?}", name, e).into(),
                    );
                    return;
                }
            }
        }

        signal.set(Some(ResourceIcons {
            emerald: images.remove(0),
            ore: images.remove(0),
            crops: images.remove(0),
            fish: images.remove(0),
            wood: images.remove(0),
            rainbow: images.remove(0),
        }));
    });
}
