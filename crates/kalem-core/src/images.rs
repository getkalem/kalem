//! Pictures in documents: where an image link's file is, and pictures
//! pasted or dropped into a document, copied into the folder beside it
//! (`NAME_assets/`, where `kalem import` puts a document's pictures too)
//! and linked.

use std::path::{Path, PathBuf};

/// The extensions of the pictures the editors show.
pub const EXTENSIONS: &[&str] = &[
    "png", "jpeg", "jpg", "gif", "webp", "bmp", "tif", "tiff", "svg", "ico",
];

/// Whether `path` names a picture, by its extension.
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The file an image link names: `~/` is the home folder, a relative path
/// starts at `base` (the document's folder).
pub fn resolve(path: &str, base: Option<&Path>) -> PathBuf {
    let path = path.strip_prefix("file:").unwrap_or(path);
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    let p = PathBuf::from(path);
    match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p,
    }
}

static ASSETS_DIR: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Sets where pictures go, from `org.assets_dir`: a folder name in which
/// `{name}` is the document's name, relative to the document's folder.
pub fn set_assets_dir(pattern: &str) {
    if let Ok(mut p) = ASSETS_DIR.write() {
        *p = pattern.to_string();
    }
}

/// The folder pictures of `document` go into: `org.assets_dir` beside it,
/// `NAME_assets` by default.
pub fn assets_dir(document: &Path) -> Option<PathBuf> {
    let pattern = ASSETS_DIR.read().map(|p| p.clone()).unwrap_or_default();
    assets_dir_with(document, &pattern)
}

fn assets_dir_with(document: &Path, pattern: &str) -> Option<PathBuf> {
    let stem = document.file_stem()?.to_string_lossy().into_owned();
    let pattern = if pattern.trim().is_empty() {
        "{name}_assets"
    } else {
        pattern.trim()
    };
    let dir = document.parent().unwrap_or(Path::new(""));
    Some(dir.join(pattern.replace("{name}", &stem)))
}

/// A name in `dir` like `name` that no file has yet (`a.png`, `a-2.png`…).
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}{ext}")))
        .find(|p| !p.exists())
        .expect("a free name")
}

/// The link to `file` from `document`: relative when it is in the
/// document's folder or below.
fn link(document: &Path, file: &Path) -> String {
    let dir = document.parent().unwrap_or(Path::new(""));
    let rel = file.strip_prefix(dir).unwrap_or(file);
    let s = rel.to_string_lossy().replace('\\', "/");
    format!("[[file:{s}]]")
}

/// Copies the picture `file` into `document`'s assets folder (unless it is
/// in the document's folder already) and gives the link to write.
pub fn import(document: &Path, file: &Path) -> Result<String, String> {
    let dir = document.parent().unwrap_or(Path::new(""));
    if file.starts_with(dir) && !dir.as_os_str().is_empty() {
        return Ok(link(document, file));
    }
    let assets = assets_dir(document).ok_or("no document name")?;
    std::fs::create_dir_all(&assets).map_err(|e| format!("{}: {e}", assets.display()))?;
    let name = file
        .file_name()
        .map_or_else(|| "image.png".into(), |n| n.to_string_lossy().into_owned());
    let target = free_name(&assets, &name);
    std::fs::copy(file, &target).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(link(document, &target))
}

/// Saves pasted picture data (`extension` as `png`) in `document`'s assets
/// folder and gives the link to write.
pub fn save(document: &Path, data: &[u8], extension: &str) -> Result<String, String> {
    let assets = assets_dir(document).ok_or("no document name")?;
    std::fs::create_dir_all(&assets).map_err(|e| format!("{}: {e}", assets.display()))?;
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
    let target = free_name(&assets, &format!("pasted-{stamp}.{extension}"));
    std::fs::write(&target, data).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(link(document, &target))
}

/// Pasted text that is only the paths of existing pictures, one a line
/// (a terminal pastes a dropped file's path): the files.
pub fn pasted_paths(text: &str) -> Option<Vec<PathBuf>> {
    let files: Vec<PathBuf> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            // Quoted or with escaped spaces, as terminals paste them.
            let l = l
                .strip_prefix('\'')
                .and_then(|l| l.strip_suffix('\''))
                .or_else(|| l.strip_prefix('"').and_then(|l| l.strip_suffix('"')))
                .map_or_else(|| l.replace("\\ ", " "), str::to_string);
            let l = l.strip_prefix("file://").map_or(l.clone(), str::to_string);
            resolve(&l, None)
        })
        .collect();
    (!files.is_empty()
        && files
            .iter()
            .all(|f| f.is_absolute() && is_image(f) && f.is_file()))
    .then_some(files)
}

/// The links for pictures pasted or dropped into `document`, copied into
/// its assets folder, one a line.
pub fn import_all(document: &Path, files: &[PathBuf]) -> Result<String, String> {
    let links: Result<Vec<String>, String> = files.iter().map(|f| import(document, f)).collect();
    Ok(links?.join("\n"))
}

/// The pixels of the picture `file`, at most `max` pixels on its longer
/// side (larger pictures are scaled down, keeping their shape); SVG is
/// drawn at its own size.
pub fn decode(file: &Path, max: u32) -> Result<image::RgbaImage, String> {
    let is_svg = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let img = if is_svg {
        let data = std::fs::read(file).map_err(|e| e.to_string())?;
        let opts = resvg::usvg::Options {
            resources_dir: file.parent().map(Path::to_path_buf),
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_data(&data, &opts).map_err(|e| e.to_string())?;
        let sz = tree.size();
        let k = (max as f32 / sz.width().max(sz.height())).min(1.0);
        let (w, h) = (
            (sz.width() * k).ceil().max(1.0) as u32,
            (sz.height() * k).ceil().max(1.0) as u32,
        );
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("empty picture")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(k, k),
            &mut pixmap.as_mut(),
        );
        // tiny-skia keeps premultiplied alpha.
        let mut rgba = pixmap.take();
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = p[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        image::RgbaImage::from_raw(w, h, rgba).ok_or("bad picture")?
    } else {
        image::ImageReader::open(file)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?
            .into_rgba8()
    };
    let (w, h) = img.dimensions();
    if w.max(h) <= max {
        return Ok(img);
    }
    let k = max as f64 / w.max(h) as f64;
    let (nw, nh) = (
        ((w as f64 * k).round() as u32).max(1),
        ((h as f64 * k).round() as u32).max(1),
    );
    Ok(image::imageops::resize(
        &img,
        nw,
        nh,
        image::imageops::FilterType::Triangle,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_dir_setting() {
        let doc = Path::new("/w/notes.org");
        assert_eq!(
            assets_dir_with(doc, ""),
            Some(PathBuf::from("/w/notes_assets"))
        );
        assert_eq!(
            assets_dir_with(doc, "img/{name}"),
            Some(PathBuf::from("/w/img/notes"))
        );
        assert_eq!(assets_dir_with(doc, "/pics"), Some(PathBuf::from("/pics")));
    }

    #[test]
    fn importing_pictures() {
        let root = std::env::temp_dir().join(format!("kalem-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let docs = root.join("docs");
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let doc = docs.join("notes.org");
        let pic = elsewhere.join("cat.png");
        std::fs::write(&pic, b"png").unwrap();
        // Copied beside the document, under a free name the second time.
        assert_eq!(import(&doc, &pic).unwrap(), "[[file:notes_assets/cat.png]]");
        assert_eq!(
            import(&doc, &pic).unwrap(),
            "[[file:notes_assets/cat-2.png]]"
        );
        // A picture in the document's folder is linked where it is.
        let local = docs.join("local.jpg");
        std::fs::write(&local, b"jpg").unwrap();
        assert_eq!(import(&doc, &local).unwrap(), "[[file:local.jpg]]");
        // Pasted data.
        let l = save(&doc, b"data", "png").unwrap();
        assert!(l.starts_with("[[file:notes_assets/pasted-"), "{l}");
        // Pasted paths.
        let text = format!("'{}'\n{}\n", pic.display(), local.display());
        assert_eq!(pasted_paths(&text), Some(vec![pic.clone(), local.clone()]));
        assert_eq!(pasted_paths("just text"), None);
        assert_eq!(pasted_paths(&format!("{} and more", pic.display())), None);
        assert_eq!(resolve("a/b.png", Some(&docs)), docs.join("a/b.png"));
        assert_eq!(resolve("/x/b.png", Some(&docs)), PathBuf::from("/x/b.png"));
    }

    #[test]
    fn decoding() {
        let dir = std::env::temp_dir().join(format!("kalem-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("big.png");
        image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 20, 30, 255]))
            .save(&png)
            .unwrap();
        let img = decode(&png, 100).unwrap();
        assert_eq!(img.dimensions(), (100, 50));
        assert_eq!(img.get_pixel(5, 5).0, [10, 20, 30, 255]);
        let svg = dir.join("box.svg");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#,
        )
        .unwrap();
        let img = decode(&svg, 1000).unwrap();
        assert_eq!(img.dimensions(), (40, 20));
        assert_eq!(img.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert!(decode(&dir.join("missing.png"), 100).is_err());
    }
}
