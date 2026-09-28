//! Formulas in the terminal: a one-line Unicode approximation of LaTeX
//! (`x^2` as `x²`, `\alpha` as `α`, `\frac{a}{b}` as `a/b`).

use crate::view::math_body as body;

const SYMBOLS: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ϵ"),
    ("varepsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("vartheta", "ϑ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("rho", "ρ"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "ϕ"),
    ("varphi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    ("cdot", "⋅"),
    ("times", "×"),
    ("div", "÷"),
    ("pm", "±"),
    ("mp", "∓"),
    ("le", "≤"),
    ("leq", "≤"),
    ("ge", "≥"),
    ("geq", "≥"),
    ("ne", "≠"),
    ("neq", "≠"),
    ("approx", "≈"),
    ("equiv", "≡"),
    ("sim", "∼"),
    ("propto", "∝"),
    ("infty", "∞"),
    ("sum", "∑"),
    ("prod", "∏"),
    ("int", "∫"),
    ("oint", "∮"),
    ("partial", "∂"),
    ("nabla", "∇"),
    ("to", "→"),
    ("rightarrow", "→"),
    ("leftarrow", "←"),
    ("Rightarrow", "⇒"),
    ("Leftarrow", "⇐"),
    ("iff", "⇔"),
    ("implies", "⇒"),
    ("mapsto", "↦"),
    ("in", "∈"),
    ("notin", "∉"),
    ("subset", "⊂"),
    ("subseteq", "⊆"),
    ("supset", "⊃"),
    ("cup", "∪"),
    ("cap", "∩"),
    ("forall", "∀"),
    ("exists", "∃"),
    ("neg", "¬"),
    ("land", "∧"),
    ("lor", "∨"),
    ("emptyset", "∅"),
    ("ldots", "…"),
    ("cdots", "⋯"),
    ("dots", "…"),
    ("hbar", "ℏ"),
    ("ell", "ℓ"),
    ("langle", "⟨"),
    ("rangle", "⟩"),
    ("mid", "∣"),
    ("circ", "∘"),
    ("star", "⋆"),
    ("prime", "′"),
    ("lim", "lim"),
    ("sin", "sin"),
    ("cos", "cos"),
    ("tan", "tan"),
    ("log", "log"),
    ("ln", "ln"),
    ("exp", "exp"),
    ("max", "max"),
    ("min", "min"),
    ("det", "det"),
    (",", " "),
    (";", " "),
    (":", " "),
    ("quad", "  "),
    ("qquad", "    "),
    ("!", ""),
    ("{", "{"),
    ("}", "}"),
    ("left", ""),
    ("right", ""),
];

const SUP: &[(char, char)] = &[
    ('0', '⁰'),
    ('1', '¹'),
    ('2', '²'),
    ('3', '³'),
    ('4', '⁴'),
    ('5', '⁵'),
    ('6', '⁶'),
    ('7', '⁷'),
    ('8', '⁸'),
    ('9', '⁹'),
    ('+', '⁺'),
    ('-', '⁻'),
    ('=', '⁼'),
    ('(', '⁽'),
    (')', '⁾'),
    ('n', 'ⁿ'),
    ('i', 'ⁱ'),
    ('T', 'ᵀ'),
];
const SUB: &[(char, char)] = &[
    ('0', '₀'),
    ('1', '₁'),
    ('2', '₂'),
    ('3', '₃'),
    ('4', '₄'),
    ('5', '₅'),
    ('6', '₆'),
    ('7', '₇'),
    ('8', '₈'),
    ('9', '₉'),
    ('+', '₊'),
    ('-', '₋'),
    ('=', '₌'),
    ('(', '₍'),
    (')', '₎'),
    ('a', 'ₐ'),
    ('e', 'ₑ'),
    ('i', 'ᵢ'),
    ('j', 'ⱼ'),
    ('k', 'ₖ'),
    ('n', 'ₙ'),
    ('o', 'ₒ'),
    ('x', 'ₓ'),
    ('m', 'ₘ'),
    ('t', 'ₜ'),
];

fn script(s: &str, table: &[(char, char)], mark: char) -> String {
    let mapped: Option<String> = s
        .chars()
        .map(|c| table.iter().find(|(a, _)| *a == c).map(|(_, b)| *b))
        .collect();
    match mapped {
        Some(m) if !m.is_empty() => m,
        _ if s.chars().count() == 1 => format!("{mark}{s}"),
        _ => format!("{mark}({s})"),
    }
}

/// Reads a `{group}` or a single character or command at `i`.
fn arg(s: &str, i: &mut usize) -> String {
    let b = s.as_bytes();
    while *i < b.len() && b[*i] == b' ' {
        *i += 1;
    }
    if *i >= b.len() {
        return String::new();
    }
    if b[*i] == b'{' {
        let mut depth = 0;
        let start = *i + 1;
        while *i < b.len() {
            match b[*i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        *i += 1;
                        return unicode_inner(&s[start..*i - 1]);
                    }
                }
                _ => {}
            }
            *i += 1;
        }
        return unicode_inner(&s[start..]);
    }
    if b[*i] == b'\\' {
        let start = *i;
        *i += 1;
        while *i < b.len() && b[*i].is_ascii_alphabetic() {
            *i += 1;
        }
        if *i == start + 1 && *i < b.len() {
            *i += 1;
        }
        return unicode_inner(&s[start..*i]);
    }
    let c = s[*i..].chars().next().unwrap();
    *i += c.len_utf8();
    c.to_string()
}

fn unicode_inner(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if i == start && i < b.len() {
                    i += 1;
                }
                let name = &s[start..i];
                match name {
                    "frac" | "dfrac" | "tfrac" => {
                        let (n, d) = (arg(s, &mut i), arg(s, &mut i));
                        let wrap = |x: String| {
                            if x.chars().count() > 1 {
                                format!("({x})")
                            } else {
                                x
                            }
                        };
                        out.push_str(&format!("{}/{}", wrap(n), wrap(d)));
                    }
                    "sqrt" => {
                        let x = arg(s, &mut i);
                        out.push_str(&if x.chars().count() > 1 {
                            format!("√({x})")
                        } else {
                            format!("√{x}")
                        });
                    }
                    "text" | "mathrm" | "textrm" | "mathit" | "operatorname" | "mathbf"
                    | "boldsymbol" => out.push_str(&arg(s, &mut i)),
                    "mathbb" => {
                        let x = arg(s, &mut i);
                        let m = match x.as_str() {
                            "R" => "ℝ",
                            "N" => "ℕ",
                            "Z" => "ℤ",
                            "Q" => "ℚ",
                            "C" => "ℂ",
                            "E" => "𝔼",
                            "P" => "ℙ",
                            _ => "",
                        };
                        out.push_str(if m.is_empty() { &x } else { m });
                    }
                    "hat" | "bar" | "vec" | "tilde" | "dot" | "overline" => {
                        let x = arg(s, &mut i);
                        let mark = match name {
                            "hat" => '\u{302}',
                            "bar" | "overline" => '\u{304}',
                            "vec" => '\u{20D7}',
                            "tilde" => '\u{303}',
                            _ => '\u{307}',
                        };
                        out.push_str(&x);
                        if x.chars().count() == 1 {
                            out.push(mark);
                        }
                    }
                    _ => match SYMBOLS.iter().find(|(k, _)| *k == name) {
                        Some((_, v)) => out.push_str(v),
                        None => {
                            out.push('\\');
                            out.push_str(name);
                        }
                    },
                }
            }
            b'^' => {
                i += 1;
                let x = arg(s, &mut i);
                out.push_str(&script(&x, SUP, '^'));
            }
            b'_' => {
                i += 1;
                let x = arg(s, &mut i);
                out.push_str(&script(&x, SUB, '_'));
            }
            b'{' | b'}' => i += 1,
            _ => {
                let c = s[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// A one-line Unicode approximation of the fragment `src`.
pub fn unicode(src: &str) -> String {
    unicode_inner(body(src).unwrap_or(src).trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_approximations() {
        assert_eq!(unicode("$x^2 + y^2 = z^2$"), "x² + y² = z²");
        assert_eq!(unicode("$a_i + b_{ij}$"), "aᵢ + bᵢⱼ");
        assert_eq!(unicode("\\(\\alpha \\le \\beta\\)"), "α ≤ β");
        assert_eq!(unicode("$\\frac{1}{2}$"), "1/2");
        assert_eq!(unicode("$\\frac{a+b}{c}$"), "(a+b)/c");
        assert_eq!(unicode("$\\sqrt{x}$"), "√x");
        assert_eq!(unicode("$\\sum_{i=1}^{n} i$"), "∑ᵢ₌₁ⁿ i");
        assert_eq!(unicode("$\\mathbb{R}^n$"), "ℝⁿ");
        assert_eq!(unicode("$e^{i\\pi}$"), "e^(iπ)");
    }
}
