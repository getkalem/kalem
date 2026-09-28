//! From Org to the engine: a fragment's delimiters removed, what RaTeX
//! lacks mapped to what it has, and the document's `\newcommand`s.

/// The formula of a LaTeX fragment or environment, without delimiters, and
/// whether it is displayed: `$x$`, `$$x$$`, `\(x\)`, `\[x\]`, or a whole
/// `\begin{env}…\end{env}` (kept, displayed).
pub fn body(fragment: &str) -> (&str, bool) {
    let t = fragment.trim();
    if let Some(b) = t.strip_prefix("$$").and_then(|s| s.strip_suffix("$$")) {
        return (b, true);
    }
    if let Some(b) = t.strip_prefix("\\[").and_then(|s| s.strip_suffix("\\]")) {
        return (b, true);
    }
    if let Some(b) = t.strip_prefix("\\(").and_then(|s| s.strip_suffix("\\)")) {
        return (b, false);
    }
    if let Some(b) = t.strip_prefix('$').and_then(|s| s.strip_suffix('$')) {
        return (b, false);
    }
    (t, t.starts_with("\\begin{"))
}

/// Replaces environment `from` with `to` (`\begin{from}` and
/// `\end{from}`).
fn rename_env(s: &str, from: &str, to: &str) -> String {
    s.replace(&format!("\\begin{{{from}}}"), &format!("\\begin{{{to}}}"))
        .replace(&format!("\\end{{{from}}}"), &format!("\\end{{{to}}}"))
}

/// The formula as RaTeX takes it: `\mbox` as `\text`, `multline` as
/// `gather` (decision D4), after the definitions `macros`.
pub fn prepare(latex: &str, macros: &str) -> String {
    let mut s = latex.replace("\\mbox{", "\\text{");
    s = rename_env(&s, "multline*", "gather*");
    s = rename_env(&s, "multline", "gather");
    // Environments that are not math in themselves.
    for env in ["equation", "equation*", "displaymath", "math"] {
        let begin = format!("\\begin{{{env}}}");
        let end = format!("\\end{{{env}}}");
        if let Some(inner) = s
            .trim()
            .strip_prefix(&begin)
            .and_then(|t| t.strip_suffix(&end))
        {
            s = inner.to_string();
            break;
        }
    }
    if macros.is_empty() {
        s
    } else {
        format!("{macros}{s}")
    }
}

/// The `\newcommand`, `\renewcommand`, `\def` and `\DeclareMathOperator`
/// definitions among `#+LATEX_HEADER` values, as one string to put before
/// each formula (§9.2: a subset; packages and other commands are
/// ignored).
pub fn macros(headers: &[String]) -> String {
    let mut out = String::new();
    for h in headers {
        let t = h.trim();
        for cmd in [
            "\\newcommand",
            "\\renewcommand",
            "\\providecommand",
            "\\def",
            "\\DeclareMathOperator",
        ] {
            if let Some(rest) = t.strip_prefix(cmd)
                && rest.starts_with(['{', '\\', '*', '['])
            {
                let def = match cmd {
                    // `\DeclareMathOperator{\op}{text}` as a command.
                    "\\DeclareMathOperator" => declare_operator(rest),
                    "\\def" | "\\providecommand" => Some(format!("{cmd}{rest}")),
                    // KaTeX predefines some names LaTeX leaves free (`\R`),
                    // and refuses to redefine them with `\newcommand`: the
                    // definition is made with `\def`.
                    _ => as_def(rest).or_else(|| Some(format!("{cmd}{rest}"))),
                };
                if let Some(d) = def {
                    out.push_str(&d);
                }
                break;
            }
        }
    }
    out
}

/// `{\name}[n]{body}` (the arguments of `\newcommand`) as
/// `\def\name#1…#n{body}`; `None` with an optional argument's default,
/// which `\def` cannot express.
fn as_def(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix('*').unwrap_or(rest);
    let (name, rest) = match group(rest) {
        Some((n, r)) => (n.trim(), r),
        None => {
            // `\newcommand\name…`
            let body = rest.strip_prefix('\\')?;
            let len = body
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .count()
                .max(1);
            (&rest[..1 + len], &rest[1 + len..])
        }
    };
    if !name.starts_with('\\') {
        return None;
    }
    let mut rest = rest.trim_start();
    let mut n = 0;
    if let Some(r) = rest.strip_prefix('[') {
        let (count, r) = r.split_once(']')?;
        n = count.trim().parse::<usize>().ok()?;
        rest = r.trim_start();
    }
    if rest.starts_with('[') {
        return None;
    }
    let (body, _) = group(rest)?;
    let params: String = (1..=n).map(|i| format!("#{i}")).collect();
    Some(format!("\\def{name}{params}{{{body}}}"))
}

/// `\DeclareMathOperator{\op}{text}` (or `*`) as `\newcommand{\op}{\operatorname{text}}`.
fn declare_operator(rest: &str) -> Option<String> {
    let (star, rest) = match rest.strip_prefix('*') {
        Some(r) => ("*", r),
        None => ("", rest),
    };
    let (name, rest) = group(rest)?;
    let (text, _) = group(rest.trim_start())?;
    Some(format!("\\def{name}{{\\operatorname{star}{{{text}}}}}"))
}

/// A `{…}` group at the start of `s`, balanced: its inside and what
/// follows.
fn group(s: &str) -> Option<(&str, &str)> {
    let s = s.strip_prefix('{')?;
    let mut depth = 1;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        assert_eq!(body("$x^2$"), ("x^2", false));
        assert_eq!(body("$$x$$"), ("x", true));
        assert_eq!(body("\\(a\\)"), ("a", false));
        assert_eq!(body("\\[ a \\]"), (" a ", true));
        assert_eq!(
            body("\\begin{align}a\\end{align}"),
            ("\\begin{align}a\\end{align}", true)
        );
    }

    #[test]
    fn preparing() {
        assert_eq!(prepare("\\mbox{if } x", ""), "\\text{if } x");
        assert_eq!(
            prepare("\\begin{multline}a\\\\b\\end{multline}", ""),
            "\\begin{gather}a\\\\b\\end{gather}"
        );
        assert_eq!(
            prepare("\\begin{equation}E=mc^2\\end{equation}", ""),
            "E=mc^2"
        );
        let m = macros(&[
            "\\usepackage{amsmath}".into(),
            "\\newcommand{\\R}{\\mathbb{R}}".into(),
            "\\DeclareMathOperator{\\tr}{tr}".into(),
        ]);
        assert_eq!(m, "\\def\\R{\\mathbb{R}}\\def\\tr{\\operatorname{tr}}");
        assert_eq!(
            macros(&["\\newcommand{\\norm}[1]{\\lVert #1 \\rVert}".into()]),
            "\\def\\norm#1{\\lVert #1 \\rVert}"
        );
        // An optional argument's default stays with \newcommand.
        assert_eq!(
            macros(&["\\newcommand{\\v}[2][x]{#1_#2}".into()]),
            "\\newcommand{\\v}[2][x]{#1_#2}"
        );
        assert_eq!(prepare("x \\in \\R", &m), format!("{m}x \\in \\R"));
    }
}
