//! What the tests of the built-in components share: the components as
//! viewers, and files made for them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kalem_script::viewer::ComponentViewer;

/// The built-in component `id` (`org.kalem.xlsx`).
pub(crate) fn built_in(id: &str) -> &'static kalem_components::Component {
    kalem_components::components()
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("{id}: not built in"))
}

/// The built-in component `id` as a viewer, with its manifest's name,
/// extensions and limits, as Kalem registers it.
pub(crate) fn viewer(id: &str) -> Arc<ComponentViewer> {
    kalem_components::viewer(id).unwrap_or_else(|| panic!("{id}: not built in"))
}

/// A folder of its own under the temporary one, emptied.
pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kalem-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The terminal editor's fixtures (`crates/kalem-tui/tests/data`).
pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kalem-tui/tests/data")
        .join(name)
}

/// A picture of 3,000 × 2,000 pixels, in shades.
pub(crate) fn big_picture(dir: &Path) -> PathBuf {
    let img = image::RgbImage::from_fn(3000, 2000, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let path = dir.join("big.png");
    img.save(&path).unwrap();
    path
}

/// A PDF of `pages` pages of text in a standard font, the second page
/// linking to the first.
pub(crate) fn long_pdf(dir: &Path, pages: usize) -> PathBuf {
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    let mut kids = Vec::new();
    for p in 0..pages {
        let mut text = String::from("BT /F1 10 Tf 12 TL 56 780 Td\n");
        for line in 0..60 {
            text += &format!(
                "(Page {} line {}: the quick brown fox jumps over the lazy dog, again and again.) '\n",
                p + 1,
                line + 1
            );
        }
        text += "ET";
        let content = objects.len() + 1;
        objects.push(format!(
            "<< /Length {} >>\nstream\n{text}\nendstream",
            text.len()
        ));
        let page = objects.len() + 1;
        kids.push(format!("{page} 0 R"));
        // Object 5 is the first page.
        let annots = if p == 1 {
            " /Annots [<< /Type /Annot /Subtype /Link /Rect [56 760 300 790] \
             /Border [0 0 0] /Dest [5 0 R /Fit] >>]"
        } else {
            ""
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {content} 0 R{annots} >>"
        ));
    }
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    );
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf += &format!("{} 0 obj\n{o}\nendobj\n", i + 1);
    }
    let xref = pdf.len();
    pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in offsets {
        pdf += &format!("{o:010} 00000 n \n");
    }
    pdf += &format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    let path = dir.join(format!("long-{pages}.pdf"));
    std::fs::write(&path, pdf).unwrap();
    path
}
