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

/// The arguments of the text from `s`'s start: optional `[…]` and
/// mandatory `{…}` arguments, as strings, and what follows them.
fn diagram_args(mut s: &str) -> (Vec<String>, Vec<String>, &str) {
    let (mut opts, mut mands) = (Vec::new(), Vec::new());
    loop {
        let t = s.trim_start();
        if t.starts_with('[') {
            // Brackets nest in tikzcd's options (`[r, "{[x]}"]`) rarely; the
            // first `]` at depth 0 of braces closes.
            let mut depth = 0;
            let mut end = None;
            for (i, c) in t.char_indices().skip(1) {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    ']' if depth == 0 => {
                        end = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(e) = end else { break };
            opts.push(t[1..e].to_string());
            s = &t[e + 1..];
        } else if t.starts_with('{') {
            let Some((g, after)) = group(t) else { break };
            mands.push(g.to_string());
            s = after;
        } else {
            break;
        }
    }
    (opts, mands, s)
}

/// Splits `s` at `sep` outside braces and brackets.
fn split_top(s: &str, sep: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if s[i..].starts_with(sep) && depth == 0 => {
                out.push(s[start..i].to_string());
                i += sep.len();
                start = i;
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            c if depth == 0 && sep.len() == 1 && c == sep.as_bytes()[0] => {
                out.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(s[start..].to_string());
    out
}

/// An arrow of a diagram: rows and columns it goes, and its label.
struct Arrow {
    rows: i32,
    cols: i32,
    label: String,
}

/// The direction letters of an arrow (`rrd`): rows and columns.
fn steps(dirs: &str) -> (i32, i32) {
    let (mut r, mut c) = (0, 0);
    for ch in dirs.chars() {
        match ch {
            'r' => c += 1,
            'l' => c -= 1,
            'd' => r += 1,
            'u' => r -= 1,
            _ => {}
        }
    }
    (r, c)
}

/// A diagram's cells: each one's text, its arrows taken out.
fn cell_arrows(cell: &str, xy: bool) -> (String, Vec<Arrow>) {
    let mut text = String::new();
    let mut arrows = Vec::new();
    let mut rest = cell;
    loop {
        let found = ["\\arrow", "\\ar"]
            .iter()
            .filter_map(|p| {
                rest.find(p)
                    .filter(|&i| {
                        !rest[i + p.len()..].starts_with(|c: char| c.is_ascii_alphabetic())
                    })
                    .map(|i| (i, p.len()))
            })
            .min();
        let Some((i, len)) = found else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..i]);
        let mut after = &rest[i + len..];
        // xymatrix's style (`@{->>}`, `@/^/`).
        while let Some(a) = after.trim_start().strip_prefix('@') {
            after = match a.chars().next() {
                Some('{') => group(a).map_or(a, |(_, r)| r),
                Some(c) => {
                    let end = a[c.len_utf8()..]
                        .find(c)
                        .map_or(a.len(), |e| e + 2 * c.len_utf8());
                    &a[end.min(a.len())..]
                }
                None => a,
            };
        }
        let (opts, mands, mut after2) = diagram_args(after);
        let mut label = String::new();
        let (mut r, mut c) = (0, 0);
        if let Some(o) = opts.first() {
            for part in split_top(o, ",") {
                let p = part.trim();
                if let Some(q) = p.strip_prefix('"') {
                    label = q.split('"').next().unwrap_or("").to_string();
                } else if p.chars().all(|ch| "rlud".contains(ch)) && !p.is_empty() {
                    (r, c) = steps(p);
                }
            }
        }
        // tikzcd's old form `\\arrow{r}{f}`.
        if r == 0
            && c == 0
            && let Some(d) = mands.first()
        {
            (r, c) = steps(d);
            if let Some(l) = mands.get(1) {
                label = l.clone();
            }
        }
        // xymatrix's labels: `^f`, `_g`, `|h`.
        if xy {
            while let Some(t) = after2.trim_start().strip_prefix(['^', '_', '|']) {
                let t = t.trim_start();
                let (l, r2) = match group(t) {
                    Some((g, r2)) => (g.to_string(), r2),
                    None => {
                        let n = t.chars().next().map_or(0, char::len_utf8);
                        if let Some(name) = t.strip_prefix('\\') {
                            let m = name
                                .chars()
                                .take_while(char::is_ascii_alphabetic)
                                .count()
                                .max(1);
                            (t[..1 + m].to_string(), &t[1 + m..])
                        } else {
                            (t[..n].to_string(), &t[n..])
                        }
                    }
                };
                if label.is_empty() {
                    label = l;
                }
                after2 = r2;
            }
        }
        arrows.push(Arrow {
            rows: r,
            cols: c,
            label,
        });
        rest = after2;
    }
    (text.trim().to_string(), arrows)
}

/// A commutative diagram (tikz-cd's `tikzcd`, xy's `\\xymatrix{…}`) as an
/// array the renderer draws: the objects in their places, an arrow
/// between neighbours as `\\xrightarrow` or `\\downarrow` with its label,
/// a diagonal one as `\\searrow` and its kin between them.
fn diagram(body: &str, xy: bool) -> String {
    let rows: Vec<Vec<(String, Vec<Arrow>)>> = split_top(body, "\\\\")
        .iter()
        .filter(|r| !r.trim().is_empty())
        .map(|r| {
            split_top(r, "&")
                .iter()
                .map(|c| cell_arrows(c, xy))
                .collect()
        })
        .collect();
    let n = rows.len().max(1);
    let m = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let (h, w) = (2 * n - 1, 2 * m - 1);
    let mut grid = vec![vec![String::new(); w]; h];
    for (i, row) in rows.iter().enumerate() {
        for (j, (text, arrows)) in row.iter().enumerate() {
            grid[2 * i][2 * j] = text.clone();
            for a in arrows {
                let (dr, dc) = (a.rows.signum(), a.cols.signum());
                let (y, x) = (2 * i as i32 + dr, 2 * j as i32 + dc);
                if y < 0 || x < 0 || y as usize >= h || x as usize >= w || (dr == 0 && dc == 0) {
                    continue;
                }
                let l = &a.label;
                let sym = match (dr, dc) {
                    (0, 1) => format!("\\xrightarrow{{{l}}}"),
                    (0, -1) => format!("\\xleftarrow{{{l}}}"),
                    (1, 0) => format!("\\Big\\downarrow{{\\scriptstyle {l}}}"),
                    (-1, 0) => format!("\\Big\\uparrow{{\\scriptstyle {l}}}"),
                    (1, 1) => format!("\\searrow{{\\scriptstyle {l}}}"),
                    (1, -1) => format!("{{\\scriptstyle {l}}}\\swarrow"),
                    (-1, 1) => format!("\\nearrow{{\\scriptstyle {l}}}"),
                    _ => format!("{{\\scriptstyle {l}}}\\nwarrow"),
                };
                let cell = &mut grid[y as usize][x as usize];
                if cell.is_empty() {
                    *cell = sym;
                }
            }
        }
    }
    let cols = "c".repeat(w);
    let body: Vec<String> = grid.iter().map(|r| r.join(" & ")).collect();
    format!(
        "\\begin{{array}}{{{cols}}}{}\\end{{array}}",
        body.join(" \\\\ ")
    )
}

/// tikz-cd's and xy's diagrams as arrays (see [`diagram`]).
fn diagrams(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(i) = out.find("\\begin{tikzcd}") {
        let after = &out[i + "\\begin{tikzcd}".len()..];
        let (_, _, body_start) = diagram_args(after);
        let Some(end) = body_start.find("\\end{tikzcd}") else {
            break;
        };
        let drawn = diagram(&body_start[..end], false);
        let tail = body_start[end + "\\end{tikzcd}".len()..].to_string();
        out = format!("{}{drawn}{tail}", &out[..i]);
    }
    while let Some(i) = out.find("\\xymatrix") {
        let mut after = &out[i + "\\xymatrix".len()..];
        // `@C=1em`, `@R-2pc`, `@!0`: spacing.
        while let Some(a) = after.trim_start().strip_prefix('@') {
            let end = a.find('{').unwrap_or(a.len());
            let stop = a[..end].find(char::is_whitespace).unwrap_or(end);
            after = &a[stop..];
        }
        let Some((body, tail)) = group(after.trim_start()) else {
            break;
        };
        let drawn = diagram(body, true);
        let tail = tail.to_string();
        out = format!("{}{drawn}{tail}", &out[..i]);
    }
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

/// `\\begin{from}[opt]{arg}…\\end{from}` as `\\begin{to}…\\end{to}`, the
/// optional argument and `args` mandatory ones after `\\begin` left out.
fn rename_env_args(s: &str, from: &str, to: &str, args: usize) -> String {
    let begin = format!("\\begin{{{from}}}");
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(&begin) {
        out.push_str(&rest[..i]);
        out.push_str(&format!("\\begin{{{to}}}"));
        let mut after = &rest[i + begin.len()..];
        let t = after.trim_start();
        if t.starts_with('[')
            && let Some(close) = t.find(']')
        {
            after = &t[close + 1..];
        }
        for _ in 0..args {
            match group(after.trim_start()) {
                Some((_, a)) => after = a,
                None => break,
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out.replace(&format!("\\end{{{from}}}"), &format!("\\end{{{to}}}"))
}

/// empheq's `\\begin{empheq}[box]{align}…\\end{empheq}` as the environment
/// it names.
fn empheq(s: &str) -> String {
    let Some(i) = s.find("\\begin{empheq}") else {
        return s.to_string();
    };
    let mut after = &s[i + "\\begin{empheq}".len()..];
    let t = after.trim_start();
    if t.starts_with('[')
        && let Some(close) = t.find(']')
    {
        after = &t[close + 1..];
    }
    let Some((inner, after)) = group(after.trim_start()) else {
        return s.to_string();
    };
    let inner = inner.trim();
    format!("{}\\begin{{{inner}}}{after}", &s[..i])
        .replace("\\end{empheq}", &format!("\\end{{{inner}}}"))
}

/// The formula as RaTeX takes it: `\mbox` as `\text`, `multline` as
/// `gather` (decision D4), after the definitions `macros`.
pub fn prepare(latex: &str, macros: &str) -> String {
    let mut s = latex.replace("\\mbox{", "\\text{");
    s = rename_env(&s, "multline*", "gather*");
    s = rename_env(&s, "multline", "gather");
    // eqnarray's `a &=& b` as an align's columns; flalign as align.
    s = diagrams(&s);
    s = plain_array_columns(&s);
    s = math_out_of_text(&s);
    s = inner_dollars(&s);
    s = braced_delimiters(&s);
    // Environments of packages, as the renderer's: IEEEtran's
    // eqnarray with its columns, xalignat with its count, breqn's,
    // empheq with the one it names.
    for (from, to, args) in [
        ("IEEEeqnarray*", "align*", 1),
        ("IEEEeqnarray", "align", 1),
        ("xalignat*", "align*", 1),
        ("xalignat", "align", 1),
        ("xxalignat", "align*", 1),
        ("dseries*", "gather*", 0),
        ("dseries", "gather", 0),
        ("dgroup*", "gather*", 0),
        ("dgroup", "gather", 0),
        ("darray*", "align*", 0),
        ("darray", "align", 0),
    ] {
        s = rename_env_args(&s, from, to, args);
    }
    s = empheq(&s);
    for env in ["dmath*", "dmath"] {
        s = rename_env_args(&s, env, "equation*", 0);
    }
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
        "math*",
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
        // A definition's own `$…$` (`\\newcommand{\\minus}{$-$}`): math
        // already where it is used.
        format!("{}{s}", inner_dollars(macros))
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
        assert_eq!(
            prepare("\\begin{IEEEeqnarray}{rCl}a&=&b\\end{IEEEeqnarray}", ""),
            "\\begin{align}a&=&b\\end{align}"
        );
        assert_eq!(
            prepare(
                "\\begin{empheq}[left=\\empheqlbrace]{align}a&=b\\end{empheq}",
                ""
            ),
            "\\begin{align}a&=b\\end{align}"
        );
        assert_eq!(prepare("\\begin{dmath}x=1\\end{dmath}", ""), "x=1");
        assert_eq!(prepare("\\begin{math*}x\\end{math*}", ""), "x");
    }

    #[test]
    fn commutative_diagrams() {
        // tikz-cd and xy: the objects in their places, the arrows between
        // them with their labels, as an array the renderer draws.
        let t = prepare(
            "\\begin{tikzcd}[cramped] A \\arrow[r, \"f\"] \\arrow[d, \"g\"'] & B \\arrow[d] \\\\ C \\arrow[r, \"h\"] & D \\end{tikzcd}",
            "",
        );
        assert_eq!(
            t,
            "\\begin{array}{ccc}A & \\xrightarrow{f} & B \\\\ \\Big\\downarrow{\\scriptstyle g} &  & \\Big\\downarrow{\\scriptstyle } \\\\ C & \\xrightarrow{h} & D\\end{array}"
        );
        assert!(crate::check(&t).is_ok(), "{t}");
        let x = prepare(
            "\\xymatrix@C=2em{ V \\ar@{->>}[d]_p \\ar[r]^{\\phi} & W \\\\ U & }",
            "",
        );
        assert!(
            x.starts_with("\\begin{array}{ccc}V & \\xrightarrow{\\phi} & W"),
            "{x}"
        );
        assert!(crate::check(&x).is_ok(), "{x}");
    }

    #[test]
    fn dollars_in_definitions() {
        assert_eq!(
            prepare("1\\minus x", "\\def\\minus{$-$}"),
            "\\def\\minus{-}1\\minus x"
        );
    }
}
