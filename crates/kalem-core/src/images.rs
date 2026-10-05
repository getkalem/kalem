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
    let raw = resolve_raw(path, base);
    // A Markdown link percent-encodes (`my%20pic.png`, as VS Code and
    // Obsidian write it): decoded when only that names a file.
    if !raw.exists()
        && path.contains('%')
        && let Some(decoded) = crate::dired::percent_decode(path)
    {
        let p = resolve_raw(&decoded, base);
        if p.exists() {
            return p;
        }
    }
    raw
}

/// `s` with every byte but ASCII letters, digits and `-._~/` as `%XX`, as
/// editors write a link to a file with spaces or letters outside ASCII.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn resolve_raw(path: &str, base: Option<&Path>) -> PathBuf {
    let path = path.strip_prefix("file:").unwrap_or(path);
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    let p = PathBuf::from(path);
    // `/x/b.png` is not relative on Windows either: the drive's root.
    match base {
        Some(b) if !p.has_root() => b.join(p),
        _ => p,
    }
}

static ASSETS_DIR: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Where a Markdown document's pictures go, from `markdown.assets_dir`.
static MD_ASSETS_DIR: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Sets where Markdown documents' pictures go: a folder name relative to
/// the document's folder (`images` by default), `{name}` its name.
pub fn set_markdown_assets_dir(pattern: &str) {
    if let Ok(mut p) = MD_ASSETS_DIR.write() {
        *p = pattern.to_string();
    }
}

/// How a picture is linked: Org's `[[file:…]]` or Markdown's `![…](…)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStyle {
    /// `[[file:a.png]]`, the pictures in `org.assets_dir`.
    Org,
    /// `![a](images/a.png)`, the pictures in `markdown.assets_dir`.
    Markdown,
}

impl LinkStyle {
    /// The style of a document of `mode`, if it links pictures.
    pub fn of(mode: &crate::DocumentMode) -> Option<LinkStyle> {
        match mode {
            crate::DocumentMode::Org => Some(LinkStyle::Org),
            crate::DocumentMode::Markdown => Some(LinkStyle::Markdown),
            _ => None,
        }
    }
}

fn dir_for(document: &Path, style: LinkStyle) -> Option<PathBuf> {
    match style {
        LinkStyle::Org => assets_dir(document),
        LinkStyle::Markdown => {
            let pattern = MD_ASSETS_DIR.read().map(|p| p.clone()).unwrap_or_default();
            let pattern = if pattern.trim().is_empty() {
                "images".to_string()
            } else {
                pattern
            };
            assets_dir_with(document, &pattern)
        }
    }
}

/// The link to `file` from `document` in `style`: relative when it is in
/// the document's folder or below.
fn link_in(document: &Path, file: &Path, style: LinkStyle) -> String {
    if style == LinkStyle::Org {
        return link(document, file);
    }
    let dir = document.parent().unwrap_or(Path::new(""));
    let rel = file.strip_prefix(dir).unwrap_or(file);
    let s = rel.to_string_lossy().replace('\\', "/");
    let alt = file
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if s.contains([' ', '(', ')']) {
        format!("![{alt}](<{s}>)")
    } else {
        format!("![{alt}]({s})")
    }
}

/// [`import`] for a document linking in `style`.
pub fn import_as(document: &Path, file: &Path, style: LinkStyle) -> Result<String, String> {
    // Both as the file system names them (`/var` is `/private/var` on
    // macOS), so a file beside the document is seen to be.
    let canonical = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let document = &match (document.parent(), document.file_name()) {
        (Some(d), Some(n)) if !d.as_os_str().is_empty() => canonical(d).join(n),
        _ => document.to_path_buf(),
    };
    let file = &canonical(file);
    let dir = document.parent().unwrap_or(Path::new(""));
    if file.starts_with(dir) && !dir.as_os_str().is_empty() {
        return Ok(link_in(document, file, style));
    }
    let assets = dir_for(document, style).ok_or("no document name")?;
    std::fs::create_dir_all(&assets).map_err(|e| format!("{}: {e}", assets.display()))?;
    let name = file
        .file_name()
        .map_or_else(|| "image.png".into(), |n| n.to_string_lossy().into_owned());
    let target = free_name(&assets, &name);
    std::fs::copy(file, &target).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(link_in(document, &target, style))
}

/// The pictures in `document`'s pictures folder (for `style`) that no
/// document links any more: a picture is in use while its file name
/// appears in `text` (the document as it is in the editor) or in any
/// Org, Markdown or LaTeX file of the document's folder and the folders
/// below it, so a picture another document shares is kept.
pub fn unused(document: &Path, text: &str, style: LinkStyle) -> Vec<PathBuf> {
    let Some(assets) = dir_for(document, style) else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(&assets) else {
        return Vec::new();
    };
    let mut pictures: Vec<(PathBuf, String)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image(p))
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            Some((p, name))
        })
        .collect();
    // A name as written, with `%20` for its spaces, or percent-encoded
    // whole (`%C3%A7` for `ç`).
    let used = |t: &str, name: &str| {
        t.contains(name)
            || t.contains(&name.replace(' ', "%20"))
            || t.contains(&percent_encode(name))
    };
    pictures.retain(|(_, n)| !used(text, n));
    let root = document.parent().unwrap_or(Path::new(""));
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if pictures.is_empty() {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let hidden = p
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if p.is_dir() {
                if depth < 8 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let linking = p.extension().is_some_and(|x| {
                matches!(
                    x.to_string_lossy().to_lowercase().as_str(),
                    "org" | "md" | "markdown" | "tex" | "html"
                )
            });
            if !linking || p == document {
                continue;
            }
            if let Ok(t) = std::fs::read_to_string(&p) {
                pictures.retain(|(_, n)| !used(&t, n));
            }
        }
    }
    let mut out: Vec<PathBuf> = pictures.into_iter().map(|(p, _)| p).collect();
    out.sort();
    out
}

/// [`import_all`] for a document linking in `style`.
pub fn import_all_as(
    document: &Path,
    files: &[PathBuf],
    style: LinkStyle,
) -> Result<String, String> {
    let links: Result<Vec<String>, String> = files
        .iter()
        .map(|f| import_as(document, f, style))
        .collect();
    Ok(links?.join("\n"))
}

/// [`save`] for a document linking in `style`.
pub fn save_as(
    document: &Path,
    data: &[u8],
    extension: &str,
    style: LinkStyle,
) -> Result<String, String> {
    let assets = dir_for(document, style).ok_or("no document name")?;
    std::fs::create_dir_all(&assets).map_err(|e| format!("{}: {e}", assets.display()))?;
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
    let target = free_name(&assets, &format!("pasted-{stamp}.{extension}"));
    std::fs::write(&target, data).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(link_in(document, &target, style))
}

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
        .unwrap_or_else(|| dir.join(format!("{stem}-{}{ext}", std::process::id())))
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

/// Kalem's logo (`assets/kalem.svg`): the application's icon.
pub const LOGO_SVG: &[u8] = include_bytes!("../../../assets/kalem.svg");

/// An SVG drawn as a PNG `size` pixels square (the application's icon).
pub fn svg_png(svg: &[u8], size: u32) -> Result<Vec<u8>, String> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())
        .map_err(|e| e.to_string())?;
    let sz = tree.size();
    let k = size as f32 / sz.width().max(sz.height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("empty picture")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// `img` in one color, `rgb`, its shading kept as transparency: a formula
/// TeX typeset in black, drawn in the text's color.
pub fn tint(img: &mut image::RgbaImage, rgb: [u8; 3]) {
    for p in img.pixels_mut() {
        // Dark ink is opaque; white paper, none.
        let ink = 255 - ((u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3) as u8;
        let a = (u32::from(p[3]) * u32::from(ink) / 255) as u8;
        *p = image::Rgba([rgb[0], rgb[1], rgb[2], a]);
    }
}

/// The first page of a PDF as SVG.
pub fn pdf_svg(data: Vec<u8>) -> Result<String, String> {
    use hayro_svg::hayro_syntax::Pdf;
    let pdf = Pdf::new(data).map_err(|e| format!("{e:?}"))?;
    let page = pdf.pages().iter().next().ok_or("a PDF without pages")?;
    let cache = hayro_svg::RenderCache::new();
    Ok(hayro_svg::convert(
        page,
        &cache,
        &hayro_svg::hayro_interpret::InterpreterSettings::default(),
        &hayro_svg::SvgRenderSettings::default(),
    ))
}

/// The system's fonts, for the text of SVG pictures: read once, when a
/// picture first has text. Each generic family (`serif`, `sans-serif`,
/// `monospace`) names a font that is there, the serif one also standing
/// for any family missing.
fn system_fonts() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            let first = |names: &[&str]| {
                names
                    .iter()
                    .find(|n| {
                        db.faces()
                            .any(|f| f.families.iter().any(|(family, _)| family == *n))
                    })
                    .map(|n| n.to_string())
            };
            let serif = first(&[
                "Times New Roman",
                "Times",
                "DejaVu Serif",
                "Liberation Serif",
                "Noto Serif",
            ])
            .or_else(|| {
                db.faces()
                    .next()
                    .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            });
            let sans = first(&[
                "Arial",
                "Helvetica",
                "DejaVu Sans",
                "Liberation Sans",
                "Noto Sans",
            ]);
            let mono = first(&[
                "Courier New",
                "Menlo",
                "DejaVu Sans Mono",
                "Liberation Mono",
                "Noto Sans Mono",
            ]);
            if let Some(f) = serif {
                db.set_serif_family(f);
            }
            if let Some(f) = sans {
                db.set_sans_serif_family(f);
            }
            if let Some(f) = mono {
                db.set_monospace_family(f);
            }
            std::sync::Arc::new(db)
        })
        .clone()
}

/// An EPS or PostScript picture as PDF: converted once by Ghostscript
/// (which `epstopdf` and LaTeX use too), found where TeX's programs are,
/// and kept in the pictures' cache by the file's path, size and time.
pub fn eps_pdf(file: &Path) -> Result<Vec<u8>, String> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(file).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok().hash(&mut h);
    let cache = std::env::temp_dir().join("kalem-pictures");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let pdf = cache.join(format!("eps-{:016x}.pdf", h.finish()));
    if !pdf.is_file() {
        let search = crate::pdf::tex_search_path();
        let gs = ["gs", "gswin64c", "gswin32c"]
            .iter()
            .find_map(|p| crate::pdf::find(p, &search))
            .ok_or("Ghostscript is not installed")?;
        // Written under another name and moved in place when whole.
        let part = pdf.with_extension("part");
        let ok = std::process::Command::new(gs)
            .args([
                "-q",
                "-dSAFER",
                "-dBATCH",
                "-dNOPAUSE",
                "-dEPSCrop",
                "-sDEVICE=pdfwrite",
            ])
            .arg(format!("-sOutputFile={}", part.display()))
            .arg(file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok || std::fs::rename(&part, &pdf).is_err() {
            let _ = std::fs::remove_file(&part);
            return Err("Ghostscript could not convert the picture".into());
        }
    }
    std::fs::read(&pdf).map_err(|e| e.to_string())
}

/// The pixels of the picture `file`, at most `max` pixels on its longer
/// side (larger pictures are scaled down, keeping their shape); SVG is
/// drawn at its own size.
pub fn decode(file: &Path, max: u32) -> Result<image::RgbaImage, String> {
    let ext = |x: &str| file.extension().is_some_and(|e| e.eq_ignore_ascii_case(x));
    let postscript = ext("eps") || ext("ps");
    let is_svg = ext("svg") || ext("pdf") || postscript;
    let img = if is_svg {
        // An EPS figure (older papers'): as PDF, by Ghostscript.
        let mut data = if postscript {
            eps_pdf(file)?
        } else {
            std::fs::read(file).map_err(|e| e.to_string())?
        };
        // A PDF (LaTeX's figures): its first page, as vectors.
        if ext("pdf") || postscript {
            data = pdf_svg(data)?.into_bytes();
        }
        let mut opts = resvg::usvg::Options {
            resources_dir: file.parent().map(Path::to_path_buf),
            ..Default::default()
        };
        if data.windows(5).any(|w| w == b"<text") {
            opts.fontdb = system_fonts();
        }
        let tree = resvg::usvg::Tree::from_data(&data, &opts).map_err(|e| e.to_string())?;
        let sz = tree.size();
        // A formula TeX typeset (10pt) at the size of the editor's text.
        let grow = if crate::tex_pictures::is_formula(file) {
            1.5
        } else {
            1.0
        };
        let k = (max as f32 / sz.width().max(sz.height())).min(grow);
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
    #[test]
    fn eps_figures_drawn_through_ghostscript() {
        let search = crate::pdf::tex_search_path();
        if crate::pdf::find("gs", &search).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kalem-eps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let eps = dir.join("square.eps");
        std::fs::write(
            &eps,
            "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 40 20\n0 0 1 setrgbcolor\nnewpath 0 0 moveto 40 0 lineto 40 20 lineto 0 20 lineto closepath fill\nshowpage\n",
        )
        .unwrap();
        let img = decode(&eps, 2400).expect("drawn");
        let (w, h) = img.dimensions();
        assert!(w > h && h > 0, "{w}x{h}");
        // Blue where the square is.
        let p = img.get_pixel(w / 2, h / 2);
        assert!(p[2] > 200 && p[0] < 60 && p[3] > 200, "{p:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    use super::*;

    #[test]
    fn unused_pictures() {
        let dir = std::env::temp_dir().join(format!("kalem-unused-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("images")).unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for n in ["a.png", "b c.png", "shared.jpg", "gone.png", "notes.txt"] {
            std::fs::write(dir.join("images").join(n), "").unwrap();
        }
        let doc = dir.join("doc.md");
        std::fs::write(dir.join("sub/other.org"), "[[file:../images/shared.jpg]]\n").unwrap();
        // The text in the editor, not the file on disk, says what is used.
        std::fs::write(&doc, "![](images/gone.png)\n").unwrap();
        let text = "![a](images/a.png)\n![b](<images/b%20c.png>)\n";
        assert_eq!(
            unused(&doc, text, LinkStyle::Markdown),
            [dir.join("images/gone.png")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    /// A one-page PDF of `w`×`h` points with a black square in it.
    fn square_pdf(w: u32, h: u32) -> Vec<u8> {
        let content = "0 0 0 rg 10 10 30 30 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R >>"),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn pdf_pictures() {
        // LaTeX's figures are PDFs: drawn at their size, the square black.
        let dir = std::env::temp_dir().join(format!("kalem-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("fig.pdf");
        std::fs::write(&f, square_pdf(50, 40)).unwrap();
        let img = decode(&f, 1000).unwrap();
        assert_eq!(img.dimensions(), (50, 40));
        // PDF's origin is at the bottom: the square is at 10..40 up from it.
        assert_eq!(img.get_pixel(20, 20).0[3], 255);
        assert_eq!(img.get_pixel(45, 5).0[3], 0);
        std::fs::remove_dir_all(&dir).ok();
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

    #[test]
    fn svg_text_and_embedded_pictures_are_drawn() {
        let dir = std::env::temp_dir().join(format!("kalem-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A PNG and a JPEG beside the SVG; a PDF figure's come as data.
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255]))
            .save(dir.join("blue.png"))
            .unwrap();
        image::RgbImage::from_pixel(8, 8, image::Rgb([0, 255, 0]))
            .save(dir.join("green.jpg"))
            .unwrap();
        let svg = dir.join("figure.svg");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="90"><image x="0" y="0" width="40" height="40" href="blue.png"/><image x="0" y="50" width="40" height="40" href="green.jpg"/><text x="60" y="40" font-family="sans-serif" font-size="32">Kalem</text></svg>"#,
        )
        .unwrap();
        let img = decode(&svg, 1000).unwrap();
        assert_eq!(img.get_pixel(20, 20).0, [0, 0, 255, 255]);
        let g = img.get_pixel(20, 70).0;
        assert!(g[1] > 200 && g[0] < 60 && g[2] < 60 && g[3] == 255, "{g:?}");
        // The text, where the system has a font.
        if !system_fonts().is_empty() {
            let inked = (60..200)
                .flat_map(|x| (0..60).map(move |y| (x, y)))
                .filter(|&(x, y)| img.get_pixel(x, y).0[3] > 0)
                .count();
            assert!(inked > 100, "{inked} pixels of text");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod logo_tests {
    #[test]
    fn the_logo_draws() {
        let png = super::svg_png(super::LOGO_SVG, 256).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        if let Some(out) = std::env::var_os("KALEM_LOGO_PNG") {
            std::fs::write(out, &png).unwrap();
        }
    }
}
