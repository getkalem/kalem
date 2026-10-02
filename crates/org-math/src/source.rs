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

/// The column specifications of `array`s without what RaTeX does not
/// take: material between columns (`@{}`, `!{}`) and before or after a
/// column's cells (`>{}`, `<{}`).
fn plain_array_columns(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("\\begin{array}") {
        let after = i + "\\begin{array}".len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        // The optional position, then the specification.
        let trimmed = rest.trim_start();
        let mut skip = rest.len() - trimmed.len();
        if trimmed.starts_with('[')
            && let Some(close) = trimmed.find(']')
        {
            skip += close + 1;
        }
        // The position (`[c]`) left out: the renderer does not take it.
        rest = rest[skip..].trim_start();
        if let Some((spec, after)) = group(rest) {
            let mut cleaned = String::new();
            let mut chars = spec.chars().peekable();
            while let Some(c) = chars.next() {
                if matches!(c, '@' | '!' | '>' | '<') && chars.peek() == Some(&'{') {
                    let mut depth = 0;
                    for d in chars.by_ref() {
                        match d {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    cleaned.push(c);
                }
            }
            out.push('{');
            out.push_str(&repeat_columns(&cleaned));
            out.push('}');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `*{3}{c}` in a column specification as `ccc`.
fn repeat_columns(spec: &str) -> String {
    let mut out = String::new();
    let mut rest = spec;
    while let Some(i) = rest.find("*{") {
        out.push_str(&rest[..i]);
        let Some((n, after)) = group(&rest[i + 1..]) else {
            out.push_str(&rest[i..]);
            return out;
        };
        let Some((cols, after)) = group(after) else {
            out.push_str(&rest[i..]);
            return out;
        };
        let n: usize = n.trim().parse().unwrap_or(1);
        out.push_str(&repeat_columns(cols).repeat(n.min(64)));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Math delimiters inside a formula (`\\ch{->[ $\\mu$ ]}`, left after the
/// text is taken care of): the formula is math already.
fn inner_dollars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('(' | ')') => {
                    chars.next();
                }
                Some(&d) => {
                    out.push(c);
                    out.push(d);
                    chars.next();
                }
                None => out.push(c),
            },
            '$' => {}
            _ => out.push(c),
        }
    }
    out
}

/// `\\Big{(}`, as old papers write it: `\\Big(`.
fn braced_delimiters(s: &str) -> String {
    let mut out = s.to_string();
    for size in [
        "\\bigl", "\\bigr", "\\Bigl", "\\Bigr", "\\biggl", "\\biggr", "\\Biggl", "\\Biggr",
        "\\bigg", "\\Bigg", "\\big", "\\Big",
    ] {
        for d in [
            "(", ")", "[", "]", "|", ".", "/", "\\{", "\\}", "\\|", "\\langle", "\\rangle",
        ] {
            out = out.replace(&format!("{size}{{{d}}}"), &format!("{size}{d}"));
        }
    }
    out
}

/// `\\text{a $x$ b}` as `\\text{a }x\\text{ b}`: the renderer does not take
/// math inside its text (`$…$`, `\\(…\\)`).
fn math_out_of_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = ["\\text{", "\\textrm{", "\\textit{", "\\textbf{", "\\mbox{"]
        .iter()
        .filter_map(|p| rest.find(p).map(|i| (i, p.len())))
        .min()
    {
        let (at, len) = i;
        let name_end = at + len - 1;
        out.push_str(&rest[..name_end]);
        let Some((inner, after)) = group(&rest[name_end..]) else {
            out.push_str(&rest[name_end..]);
            return out;
        };
        let name = &rest[at..name_end];
        let mut text = String::new();
        let mut chars = inner.char_indices().peekable();
        let mut changed = false;
        let mut pieces = String::new();
        while let Some((k, c)) = chars.next() {
            let close = if c == '$' {
                Some("$")
            } else if c == '\\' && inner[k..].starts_with("\\(") {
                chars.next();
                Some("\\)")
            } else if c == '\\' && inner[k..].starts_with("\\ensuremath{") {
                // `\\ensuremath{x}` in text: x as math.
                for _ in 0.."\\ensuremath".len() - 1 {
                    chars.next();
                }
                Some("}")
            } else {
                None
            };
            match close {
                Some(close) => {
                    let start = k + match close {
                        "$" => 1,
                        "}" => "\\ensuremath{".len(),
                        _ => 2,
                    };
                    match inner[start..].find(close) {
                        Some(e) => {
                            pieces
                                .push_str(&format!("{{{text}}}{}{name}", &inner[start..start + e]));
                            text.clear();
                            changed = true;
                            let stop = start + e + close.len();
                            while chars.peek().is_some_and(|(j, _)| *j < stop) {
                                chars.next();
                            }
                        }
                        None => text.push(c),
                    }
                }
                None => {
                    if c == '\\' {
                        // A control sequence stays whole (`\\$` is a dollar).
                        text.push(c);
                        if let Some((_, d)) = chars.next() {
                            text.push(d);
                        }
                    } else {
                        text.push(c);
                    }
                }
            }
        }
        if changed {
            out.push_str(&pieces);
            out.push_str(&format!("{{{text}}}"));
        } else {
            out.push_str(&format!("{{{inner}}}"));
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The formula as RaTeX takes it: `\mbox` as `\text`, `multline` as
/// `gather` (decision D4), after the definitions `macros`.
pub fn prepare(latex: &str, macros: &str) -> String {
    let mut s = latex.replace("\\mbox{", "\\text{");
    s = rename_env(&s, "multline*", "gather*");
    s = rename_env(&s, "multline", "gather");
    // eqnarray's `a &=& b` as an align's columns; flalign as align.
    s = plain_array_columns(&s);
    s = math_out_of_text(&s);
    s = inner_dollars(&s);
    s = braced_delimiters(&s);
    s = rename_env(&s, "eqnarray*", "align*");
    s = rename_env(&s, "eqnarray", "align");
    s = rename_env(&s, "flalign*", "align*");
    s = rename_env(&s, "flalign", "align");
    // Environments that are not math in themselves.
    for env in [
        "equation",
        "equation*",
        "displaymath",
        "displaymath*",
        "math",
    ] {
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

#[cfg(test)]
mod prepare_tests {
    use super::*;

    #[test]
    fn what_the_renderer_does_not_take() {
        assert_eq!(
            prepare("\\text{if $x>0$ and \\(y\\)}", ""),
            "\\text{if }x>0\\text{ and }y\\text{}"
        );
        assert_eq!(prepare("\\text{costs \\$5}", ""), "\\text{costs \\$5}");
        assert_eq!(
            prepare("\\begin{array}{@{}c@{\\quad}>{\\bf}l@{}}a\\end{array}", ""),
            "\\begin{array}{cl}a\\end{array}"
        );
        assert_eq!(
            prepare("\\begin{eqnarray*}a&=&b\\end{eqnarray*}", ""),
            "\\begin{align*}a&=&b\\end{align*}"
        );
    }
}
