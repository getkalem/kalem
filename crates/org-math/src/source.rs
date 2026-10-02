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
                if matches!(c, '@' | '!' | '>' | '<' | 'p' | 'm' | 'b')
                    && chars.peek() == Some(&'{')
                {
                    // A paragraph column (`p{3cm}`): left aligned.
                    if matches!(c, 'p' | 'm' | 'b') {
                        cleaned.push('l');
                    }
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
                    // A column type of a package or the document's own
                    // (`\\newcolumntype{L}`, nicematrix's lines): the
                    // nearest the renderer has.
                    cleaned.push(match c {
                        'l' | 'c' | 'r' | '|' | ':' | '*' | ' ' | '{' | '}' => c,
                        '0'..='9' => c,
                        'L' | 'X' | 'J' => 'l',
                        'R' => 'r',
                        'I' => '|',
                        _ => 'c',
                    });
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
    while let Some(i) = out.find("\\Qcircuit") {
        let mut after = &out[i + "\\Qcircuit".len()..];
        while let Some(a) = after.trim_start().strip_prefix('@') {
            let end = a.find('{').unwrap_or(a.len());
            let stop = a[..end].find(char::is_whitespace).unwrap_or(end);
            after = &a[stop..];
        }
        let Some((body, tail)) = group(after.trim_start()) else {
            break;
        };
        let drawn = circuit(body);
        let tail = tail.to_string();
        out = format!("{}{drawn}{tail}", &out[..i]);
    }
    out
}

/// qcircuit's circuit as an array: gates boxed, wires as rules, controls
/// as dots and targets as ⊕.
fn circuit(body: &str) -> String {
    let rows = split_top(body, "\\\\");
    let rows: Vec<Vec<String>> = rows
        .iter()
        .filter(|r| !r.trim().is_empty())
        .map(|r| {
            split_top(r, "&")
                .iter()
                .map(|cell| {
                    let mut c = cell.trim().to_string();
                    for (name, args) in [
                        ("\\gate", 1),
                        ("\\multigate", 2),
                        ("\\ghost", 1),
                        ("\\lstick", 1),
                        ("\\rstick", 1),
                        ("\\ctrl", 1),
                        ("\\ctrlo", 1),
                        ("\\targ", 0),
                        ("\\meter", 0),
                        ("\\measureD", 1),
                        ("\\qw", 0),
                        ("\\qwx", 0),
                        ("\\cw", 0),
                        ("\\cwx", 0),
                        ("\\push", 1),
                    ] {
                        c = replace_args(&c, name, args, |a| match name {
                            "\\gate" | "\\measureD" => format!("\\boxed{{{}}}", a[0]),
                            "\\multigate" => format!("\\boxed{{{}}}", a[1]),
                            "\\lstick" | "\\rstick" | "\\push" => a[0].clone(),
                            "\\ctrl" => "\\bullet".into(),
                            "\\ctrlo" => "\\circ".into(),
                            "\\targ" => "\\oplus".into(),
                            "\\meter" => "\\boxed{\\nearrow}".into(),
                            "\\qw" => "\\text{\u{2014}}".into(),
                            "\\cw" => "=".into(),
                            _ => String::new(),
                        });
                    }
                    c
                })
                .collect()
        })
        .collect();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let body: Vec<String> = rows.iter().map(|r| r.join(" & ")).collect();
    format!(
        "\\begin{{array}}{{{}}}{}\\end{{array}}",
        "c".repeat(cols),
        body.join(" \\\\ ")
    )
}

/// Each `name` with `args` arguments (groups or single tokens) in `s` as
/// `f(arguments)`.
fn replace_args(s: &str, name: &str, args: usize, f: impl Fn(&[String]) -> String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = command_at(rest, name, 0) {
        out.push_str(&rest[..i]);
        let mut after = &rest[i + name.len()..];
        let mut got = Vec::new();
        for _ in 0..args {
            let t = after.trim_start();
            if let Some((g, r)) = group(t) {
                got.push(g.to_string());
                after = r;
            } else if let Some(c) = t.chars().next() {
                got.push(c.to_string());
                after = &t[c.len_utf8()..];
            }
        }
        while got.len() < args {
            got.push(String::new());
        }
        out.push_str(&f(&got));
        rest = after;
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
    while let Some(i) = [
        "\\text{",
        "\\textrm{",
        "\\textit{",
        "\\textbf{",
        "\\textsf{",
        "\\texttt{",
        "\\textup{",
        "\\textnormal{",
        "\\mbox{",
        "\\hbox{",
        "\\intertext{",
        "\\shortintertext{",
    ]
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
                    // `\ensuremath{…}` ends at its matching brace.
                    let end = if close == "}" {
                        group(&inner[start - 1..]).map(|(g, _)| g.len())
                    } else {
                        inner[start..].find(close)
                    };
                    match end {
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

/// The places of control word `name` (with its backslash) in `s`, not a
/// longer word's start.
fn command_at(s: &str, name: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(i) = s[at..].find(name) {
        let i = at + i;
        let end = i + name.len();
        if !s[end..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            return Some(i);
        }
        at = end;
    }
    None
}

/// Each `name{arg}` in `s` (spaces allowed before the group) as `f(arg)`.
fn replace_command(s: &str, name: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = command_at(rest, name, 0) {
        let after = &rest[i + name.len()..];
        match group(after.trim_start()) {
            Some((arg, tail)) => {
                out.push_str(&rest[..i]);
                out.push_str(&f(arg));
                rest = tail;
            }
            None => {
                out.push_str(&rest[..i + name.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A dimension at the start of `s` (`-2pt`, `.5ex`, `-\nulldelimiterspace`,
/// `0.3\baselineskip`): its length in bytes, 0 when there is none.
fn dimension(s: &str) -> usize {
    let t = s.trim_start();
    let lead = s.len() - t.len();
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b'-' | b'+' | b' ') {
        i += 1;
    }
    let digits = i;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b',') {
        i += 1;
    }
    let has_number = i > digits;
    let r = &t[i..];
    let r2 = r.trim_start();
    let gap = r.len() - r2.len();
    if let Some(reg) = r2.strip_prefix('\\') {
        let n = reg.chars().take_while(char::is_ascii_alphabetic).count();
        if n > 0 {
            return lead + i + gap + 1 + n;
        }
    }
    if has_number {
        for unit in [
            "pt", "em", "ex", "mm", "cm", "in", "bp", "mu", "sp", "pc", "dd", "cc",
        ] {
            if r2.starts_with(unit) {
                return lead + i + gap + unit.len();
            }
        }
        // `\penalty 0`: a number alone.
        return lead + i;
    }
    0
}

/// The rows of a plain TeX alignment (`a & b \cr c & d`) as LaTeX's.
fn plain_rows(body: &str) -> String {
    let b = body.trim_end();
    let b = b.strip_suffix("\\cr").unwrap_or(b);
    b.replace("\\crcr", "\\\\").replace("\\cr", "\\\\")
}

/// Plain TeX and packages the renderer does not have, as what it has:
/// `\aligned…\endaligned`, `\matrix{…}`, `\cases{…}`, `\buildrel…\over`,
/// spacing and penalties by registers, ytableau, a `tabular` inside the
/// formula and others.
fn plain_tex(s: &str) -> String {
    let mut s = s.to_string();
    for env in ["aligned", "gathered", "split"] {
        let begin = format!("\\{env}");
        let end = format!("\\end{env}");
        if command_at(&s, &end, 0).is_some() {
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = command_at(rest, &end, 0) {
                out.push_str(&rest[..i]);
                out.push_str(&format!("\\end{{{env}}}"));
                rest = &rest[i + end.len()..];
            }
            out.push_str(rest);
            s = out;
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = command_at(rest, &begin, 0) {
                out.push_str(&rest[..i]);
                out.push_str(&format!("\\begin{{{env}}}"));
                rest = &rest[i + begin.len()..];
            }
            out.push_str(rest);
            s = out;
        }
    }
    for (name, env) in [
        ("\\matrix", "matrix"),
        ("\\pmatrix", "pmatrix"),
        ("\\bordermatrix", "matrix"),
        ("\\kbordermatrix", "matrix"),
        ("\\bbordermatrix", "matrix"),
    ] {
        s = replace_command(&s, name, |b| {
            format!("\\begin{{{env}}}{}\\end{{{env}}}", plain_rows(b))
        });
    }
    // Plain TeX's cases: the second column is text.
    s = replace_command(&s, "\\cases", |b| {
        let rows: Vec<String> = split_top(&plain_rows(b), "\\\\")
            .into_iter()
            .map(|row| match row.split_once('&') {
                Some((a, t)) if !t.trim().is_empty() => format!("{a}&\\text{{{}}}", t.trim()),
                _ => row,
            })
            .collect();
        format!("\\begin{{cases}}{}\\end{{cases}}", rows.join("\\\\"))
    });
    // `\buildrel a \over =`: `\overset{a}{=}`.
    while let Some(i) = command_at(&s, "\\buildrel", 0) {
        let after = i + "\\buildrel".len();
        let Some(o) = command_at(&s, "\\over", after) else {
            break;
        };
        let top = s[after..o].trim().to_string();
        let rest = &s[o + "\\over".len()..];
        let t = rest.trim_start();
        let (base, tail) = if let Some((g, tail)) = group(t) {
            (g.to_string(), tail)
        } else if let Some(cs) = t.strip_prefix('\\') {
            let n = cs
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .count()
                .max(1);
            let n = cs.char_indices().nth(n).map_or(cs.len(), |(k, _)| k);
            (t[..1 + n].to_string(), &cs[n..])
        } else {
            let n = t.chars().next().map_or(0, char::len_utf8);
            (t[..n].to_string(), &t[n..])
        };
        let tail = tail.to_string();
        s = format!("{}\\overset{{{top}}}{{{base}}}{tail}", &s[..i]);
    }
    // Spacing by a register (`\kern-\nulldelimiterspace`), boxes moved
    // up or down, penalties: nothing to draw.
    for name in [
        "\\kern",
        "\\mkern",
        "\\hskip",
        "\\mskip",
        "\\lower",
        "\\raise",
        "\\penalty",
        "\\vskip",
    ] {
        let mut out = String::new();
        let mut rest = s.as_str();
        while let Some(i) = command_at(rest, name, 0) {
            let after = &rest[i + name.len()..];
            let n = dimension(after);
            let register = after[..n].contains('\\');
            if n > 0 && (register || !matches!(name, "\\kern" | "\\mkern" | "\\hskip" | "\\mskip"))
            {
                out.push_str(&rest[..i]);
                rest = &after[n..];
            } else {
                out.push_str(&rest[..i + name.len()]);
                rest = after;
            }
        }
        out.push_str(rest);
        s = out;
    }
    for name in [
        "\\noalign",
        "\\cline",
        "\\hhline",
        "\\NiceMatrixOptions",
        "\\ytableausetup",
    ] {
        s = replace_command(&s, name, |_| String::new());
    }
    for (from, to) in [
        ("cases*", "cases"),
        ("dcases*", "dcases"),
        ("NiceMatrix", "matrix"),
        ("pNiceMatrix", "pmatrix"),
        ("bNiceMatrix", "bmatrix"),
        ("vNiceMatrix", "vmatrix"),
        ("NiceArray", "array"),
    ] {
        s = rename_env(&s, from, to);
    }
    // nicematrix's arrays with delimiters.
    for (env, l, r) in [
        ("pNiceArray", "(", ")"),
        ("bNiceArray", "[", "]"),
        ("BNiceArray", "\\{", "\\}"),
        ("vNiceArray", "|", "|"),
        ("VNiceArray", "\\|", "\\|"),
    ] {
        s = s
            .replace(
                &format!("\\begin{{{env}}}"),
                &format!("\\left{l}\\begin{{array}}"),
            )
            .replace(
                &format!("\\end{{{env}}}"),
                &format!("\\end{{array}}\\right{r}"),
            );
    }
    s = rename_env_args(&s, "multlined", "gathered", 0);
    s = ytableau(&s);
    s = tabular_in_math(&s);
    s
}

/// ytableau's diagrams and tableaux as arrays of boxes.
fn ytableau(s: &str) -> String {
    let mut s = replace_command(s, "\\ydiagram", |rows| {
        let rows: Vec<String> = rows
            .split(',')
            .map(|n| {
                let n = n.trim().rsplit('+').next().unwrap_or("0");
                vec!["\\boxed{\\phantom{0}}"; n.trim().parse().unwrap_or(0)].join("&")
            })
            .collect();
        format!("\\begin{{matrix}}{}\\end{{matrix}}", rows.join("\\\\"))
    });
    let (begin, end) = ("\\begin{ytableau}", "\\end{ytableau}");
    while let Some(i) = s.find(begin) {
        let Some(e) = s[i..].find(end).map(|e| i + e) else {
            break;
        };
        let body = &s[i + begin.len()..e];
        let rows: Vec<String> = split_top(body, "\\\\")
            .iter()
            .map(|row| {
                split_top(row, "&")
                    .iter()
                    .map(|cell| {
                        let mut c = cell.trim();
                        // `*(color)`: the cell's color.
                        if let Some(r) = c.strip_prefix("*(")
                            && let Some(k) = r.find(')')
                        {
                            c = r[k + 1..].trim();
                        }
                        if c == "\\none" {
                            String::new()
                        } else if c.is_empty() {
                            "\\boxed{\\phantom{0}}".to_string()
                        } else {
                            format!("\\boxed{{{c}}}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("&")
            })
            .collect();
        s = format!(
            "{}\\begin{{matrix}}{}\\end{{matrix}}{}",
            &s[..i],
            rows.join("\\\\"),
            &s[e + end.len()..]
        );
    }
    s
}

/// A `tabular` inside a formula as an `array` of text cells.
fn tabular_in_math(s: &str) -> String {
    let (begin, end) = ("\\begin{tabular}", "\\end{tabular}");
    let mut s = s.to_string();
    while let Some(i) = s.find(begin) {
        let Some(e) = s[i..].find(end).map(|e| i + e) else {
            break;
        };
        let mut body = s[i + begin.len()..e].trim_start();
        if body.starts_with('[')
            && let Some(k) = body.find(']')
        {
            body = body[k + 1..].trim_start();
        }
        let Some((spec, body)) = group(body) else {
            break;
        };
        let rows: Vec<String> = split_top(body, "\\\\")
            .iter()
            .map(|row| {
                let mut row = row.trim();
                let mut lines = String::new();
                while let Some(r) = row.strip_prefix("\\hline") {
                    lines.push_str("\\hline ");
                    row = r.trim_start();
                }
                let cells: Vec<String> = split_top(row, "&")
                    .iter()
                    .map(|c| {
                        let c = c.trim();
                        if c.is_empty() {
                            String::new()
                        } else {
                            format!("\\text{{{c}}}")
                        }
                    })
                    .collect();
                format!("{lines}{}", cells.join("&"))
            })
            .collect();
        s = format!(
            "{}\\begin{{array}}{{{spec}}}{}\\end{{array}}{}",
            &s[..i],
            rows.join("\\\\"),
            &s[e + end.len()..]
        );
    }
    s
}

/// `s` without TeX's comments (`%` to the end of the line, not `\\%`).
fn without_comments(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('%') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(d) = chars.next() {
                    out.push(d);
                }
            }
            '%' => {
                // To the end of the line and the line end (TeX takes it).
                for d in chars.by_ref() {
                    if d == '\n' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// `s` without TeX's italic correction `\\/` (not the `/` after `\\\\`).
fn no_italic_correction(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('/') => {
                    chars.next();
                    continue;
                }
                Some(&d) => {
                    out.push(c);
                    out.push(d);
                    chars.next();
                    continue;
                }
                None => {}
            }
        }
        out.push(c);
    }
    out
}

/// The formula as RaTeX takes it: `\mbox` as `\text`, `multline` as
/// `gather` (decision D4), after the definitions `macros`.
pub fn prepare(latex: &str, macros: &str) -> String {
    // Italic correction: nothing to draw here.
    let mut s = no_italic_correction(&without_comments(latex).replace("\\mbox{", "\\text{"));
    s = rename_env(&s, "multline*", "gather*");
    s = rename_env(&s, "multline", "gather");
    // eqnarray's `a &=& b` as an align's columns; flalign as align.
    s = diagrams(&s);
    s = plain_tex(&s);
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
        format!("{}{s}", prepared_macros(macros))
    }
}

/// The definitions as the renderer takes them, remembered: the same
/// definitions come before each formula of a document.
fn prepared_macros(macros: &str) -> std::sync::Arc<str> {
    use std::cell::RefCell;
    use std::sync::Arc;
    thread_local! {
        static LAST: RefCell<Option<(String, Arc<str>)>> = const { RefCell::new(None) };
    }
    LAST.with(|last| {
        if let Some((m, out)) = last.borrow().as_ref()
            && m == macros
        {
            return out.clone();
        }
        // A definition's own `$…$` (`\newcommand{\minus}{$-$}`): math
        // already where it is used.
        let out: Arc<str> = inner_dollars(&plain_tex(&math_out_of_text(&no_italic_correction(
            &without_comments(macros),
        ))))
        .into();
        *last.borrow_mut() = Some((macros.to_string(), out.clone()));
        out
    })
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

    #[test]
    fn plain_tex_and_packages() {
        // What TeX and its packages read and the renderer does not: each
        // as what it has.
        for (f, want) in [
            (
                "\\left\\{\\aligned a &= b \\endaligned\\right.",
                "\\begin{aligned} a",
            ),
            (
                "\\pmatrix{a & b \\cr c & d}",
                "\\begin{pmatrix}a & b \\\\ c & d\\end{pmatrix}",
            ),
            ("\\cases{a & if b \\cr c & else}", "&\\text{if b}"),
            ("\\buildrel a \\over =", "\\overset{a}{=}"),
            (
                "\\left.\\kern-\\nulldelimiterspace f\\right|",
                "\\left. f\\right|",
            ),
            ("\\penalty 0 x", " x"),
            (
                "\\begin{cases*} a \\end{cases*}",
                "\\begin{cases} a \\end{cases}",
            ),
            ("\\begin{array}{|l|p{4cm}|}a&b\\end{array}", "{|l|l|}"),
            ("p ~\\hbox{ [i.e., $p^{x}$]}", "\\hbox{ [i.e., }p^{x}"),
            (
                "\\begin{ytableau} *(red) a & \\none \\\\ b \\end{ytableau}",
                "\\boxed{a}&",
            ),
            (
                "\\begin{tabular}{@{}l@{}l@{}} $0$ & if $s$ \\end{tabular}",
                "\\begin{array}{ll}",
            ),
        ] {
            let t = prepare(f, "");
            assert!(t.contains(want), "{f}: {t}");
            assert!(crate::check(&t).is_ok(), "{f}: {t}");
        }
        let q = prepare(
            "\\Qcircuit @C=2.3em @R=0.7em { & \\lstick{\\ket{0}} & \\gate{H} & \\ctrl{1} & \\qw \\\\ & & \\qw & \\targ & \\meter }",
            "\\def\\ket#1{|#1\\rangle}",
        );
        assert!(q.contains("\\boxed{H}"), "{q}");
        assert!(crate::check(&q).is_ok(), "{q}");
        // In the definitions too.
        let t = prepare(
            "x \\eqdefa y",
            "\\def\\eqdefa{\\buildrel\\hbox{def}\\over =}",
        );
        assert!(crate::check(&t).is_ok(), "{t}");
    }
}
