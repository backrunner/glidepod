use gpui::{AssetSource, SharedString};
use std::borrow::Cow;
pub struct Assets;

/// Match the 40-point sidebar mark to the window's backing pixels, including
/// when the window moves between standard and Retina displays.
pub fn sidebar_icon(dark: bool, scale_factor: f32) -> &'static str {
    match (dark, scale_factor.ceil().clamp(1., 3.) as u8) {
        (false, 1) => "sidebar-light@1x.png",
        (false, 2) => "sidebar-light@2x.png",
        (false, _) => "sidebar-light@3x.png",
        (true, 1) => "sidebar-dark@1x.png",
        (true, 2) => "sidebar-dark@2x.png",
        (true, _) => "sidebar-dark@3x.png",
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/apps.svg" => include_bytes!("../../../assets/icons/apps.svg"),
            "icons/running.svg" => include_bytes!("../../../assets/icons/running.svg"),
            "icons/android.svg" => include_bytes!("../../../assets/icons/android.svg"),
            "icons/graphics.svg" => include_bytes!("../../../assets/icons/graphics.svg"),
            "icons/widgets.svg" => include_bytes!("../../../assets/icons/widgets.svg"),
            "icons/settings.svg" => include_bytes!("../../../assets/icons/settings.svg"),
            "icons/notes.svg" => include_bytes!("../../../assets/icons/notes.svg"),
            "icons/music.svg" => include_bytes!("../../../assets/icons/music.svg"),
            "icons/game.svg" => include_bytes!("../../../assets/icons/game.svg"),
            "icons/book.svg" => include_bytes!("../../../assets/icons/book.svg"),
            "icons/sun.svg" => include_bytes!("../../../assets/icons/sun.svg"),
            "icons/canvas.svg" => include_bytes!("../../../assets/icons/canvas.svg"),
            "icons/file.svg" => include_bytes!("../../../assets/icons/file.svg"),
            "icons/search.svg" => include_bytes!("../../../assets/icons/search.svg"),
            "icons/close.svg" => include_bytes!("../../../assets/icons/close.svg"),
            "icons/more.svg" => include_bytes!("../../../assets/icons/more.svg"),
            "icons/back.svg" => include_bytes!("../../../assets/icons/back.svg"),
            "icons/rotate.svg" => include_bytes!("../../../assets/icons/rotate.svg"),
            "icons/appearance.svg" => include_bytes!("../../../assets/icons/appearance.svg"),
            "icons/plus.svg" => include_bytes!("../../../assets/icons/plus.svg"),
            "icons/check.svg" => include_bytes!("../../../assets/icons/check.svg"),
            "icons/circle-x.svg" => include_bytes!("../../../assets/icons/circle-x.svg"),
            "icons/chevron-down.svg" => include_bytes!("../../../assets/icons/chevron-down.svg"),
            "sidebar-light@1x.png" => {
                include_bytes!("../../../assets/brand/glidepod-sidebar-light@1x.png")
            }
            "sidebar-light@2x.png" => {
                include_bytes!("../../../assets/brand/glidepod-sidebar-light@2x.png")
            }
            "sidebar-light@3x.png" => {
                include_bytes!("../../../assets/brand/glidepod-sidebar-light@3x.png")
            }
            "sidebar-dark@1x.png" => {
                include_bytes!("../../../assets/brand/glidepod-sidebar-dark@1x.png")
            }
            "sidebar-dark@2x.png" => {
                include_bytes!("../../../assets/brand/glidepod-sidebar-dark@2x.png")
            }
            "sidebar-dark@3x.png" => {
                include_bytes!("../../../assets/brand/glidepod-sidebar-dark@3x.png")
            }
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
