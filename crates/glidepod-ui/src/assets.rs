use gpui::{AssetSource, SharedString};
use std::borrow::Cow;
pub struct Assets;
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
            "glidepod-icon.svg" => include_bytes!("../../../assets/glidepod-icon.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
