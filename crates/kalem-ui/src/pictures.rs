//! Pictures of image links, drawn in the line (`org-display-inline-images`):
//! decoded the first time they are drawn and again when their file
//! changes, kept as gpui images.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use gpui::RenderImage;

/// The longest side a picture is kept at, in pixels.
const MAX_SIDE: u32 = 2400;

/// A picture ready to draw: the image and its size in pixels.
pub type Picture = (Arc<RenderImage>, u32, u32);

/// Pictures by file and tint, with the time their file was changed.
type Cache = HashMap<(PathBuf, Option<[u8; 3]>), (Option<SystemTime>, Option<Picture>)>;

/// Decoded pictures, shared by the windows.
#[derive(Default)]
pub struct Pictures {
    cache: RefCell<Cache>,
}

impl std::fmt::Debug for Pictures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pictures")
            .field("cached", &self.cache.borrow().len())
            .finish()
    }
}

impl Pictures {
    /// The picture in `file`, or `None` when it cannot be read.
    pub fn get(&self, file: &Path) -> Option<Picture> {
        self.get_tinted(file, None)
    }

    /// The picture in `file`, in one color `tint` when given (a formula
    /// TeX typeset, in the text's color).
    pub fn get_tinted(&self, file: &Path, tint: Option<[u8; 3]>) -> Option<Picture> {
        let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok();
        let key = (file.to_path_buf(), tint);
        if let Some((t, p)) = self.cache.borrow().get(&key)
            && *t == modified
        {
            return p.clone();
        }
        let picture = kalem_core::images::decode(file, MAX_SIDE)
            .ok()
            .map(|mut img| {
                if let Some(rgb) = tint {
                    kalem_core::images::tint(&mut img, rgb);
                }
                img
            })
            .and_then(|img| {
                let (w, h) = img.dimensions();
                let mut bgra = img.into_raw();
                for p in bgra.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                }
                let buf = image::RgbaImage::from_raw(w, h, bgra)?;
                Some((
                    Arc::new(RenderImage::new(vec![image::Frame::new(buf)])),
                    w,
                    h,
                ))
            });
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(key, (modified, picture.clone()));
        picture
    }
}
