//! Org's structure motions for the Vim layer, as evil-org and Doom Emacs
//! bind them in Org documents: `gj` and `gk` by element, `gh` up to the
//! element around, `gl` into it, and `]h` and `[h` by heading of the same
//! level.

use crate::document::DocumentState;

/// Where `gj`, `gk`, `gh` or `gl` (`c`) go `n` times, or Org's message
/// when they cannot.
pub(super) fn element_motion(
    doc: &mut DocumentState,
    from: usize,
    c: char,
    n: usize,
) -> Result<usize, String> {
    let model = doc
        .model()
        .ok_or_else(|| "Not an Org document".to_string())?;
    let text = model.parse().syntax().to_string();
    let ctx = model.parse().context();
    let mut pos = from.min(text.len());
    for _ in 0..n.max(1) {
        let r = match c {
            'j' => org_edit::motion::forward_element(&text, pos, ctx),
            'k' => org_edit::motion::backward_element(&text, pos, ctx),
            'h' => org_edit::motion::up_element(&text, pos, ctx),
            _ => org_edit::motion::down_element(&text, pos, ctx),
        };
        pos = r.map_err(|e| e.message)?;
    }
    Ok(pos)
}

/// Where `]h` (`forward`) or `[h` go: the heading `n` siblings away.
pub(super) fn heading_motion(
    doc: &mut DocumentState,
    from: usize,
    forward: bool,
    n: usize,
) -> Option<usize> {
    let model = doc.model()?;
    let text = model.parse().syntax().to_string();
    let n = i64::try_from(n.max(1)).unwrap_or(1);
    let pos = from.min(text.len());
    Some(org_edit::motion::heading_same_level(
        &text,
        pos,
        if forward { n } else { -n },
        model.parse().context(),
    ))
}

/// Where `]l` and `[l` (links) or `]c` and `[c` (source blocks) go,
/// `n` times, as Doom Emacs has them in Org.
pub(super) fn next_motion(
    doc: &mut DocumentState,
    from: usize,
    what: char,
    forward: bool,
    n: usize,
) -> Option<usize> {
    let model = doc.model()?;
    let text = model.parse().syntax().to_string();
    let ctx = model.parse().context();
    let mut pos = from.min(text.len());
    for _ in 0..n.max(1) {
        pos = if what == 'l' {
            org_edit::motion::next_link(&text, pos, !forward, ctx)?
        } else {
            org_edit::motion::next_src_block(&text, pos, !forward, ctx)?
        };
    }
    Some(pos)
}
