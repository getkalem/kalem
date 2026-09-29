//! The arguments of the commands and environments the parser knows: `*`
//! an optional star, `o` an optional argument `[…]`, `m` a mandatory one
//! (a group or a single token). Unknown commands take no arguments; what
//! follows them is parsed as text, which keeps it visible.

/// The arguments of command `name`.
pub fn command(name: &str) -> &'static str {
    match name {
        "part" | "chapter" | "section" | "subsection" | "subsubsection" | "paragraph"
        | "subparagraph" => "*om",
        "textbf" | "textit" | "texttt" | "textsc" | "textsf" | "textrm" | "textup" | "textmd"
        | "textsl" | "textnormal" | "emph" | "underline" | "textsuperscript" | "textsubscript"
        | "mbox" | "fbox" | "text" | "mathrm" | "mathbf" | "mathit" | "mathcal" | "mathbb"
        | "mathsf" | "mathtt" | "mathfrak" | "mathscr" | "boldsymbol" | "overline" | "hat"
        | "bar" | "tilde" | "vec" | "dot" | "ddot" | "check" | "breve" | "acute" | "grave"
        | "widehat" | "widetilde" | "overbrace" | "underbrace" | "overrightarrow"
        | "overleftarrow" | "label" | "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref"
        | "nameref" | "input" | "include" | "includeonly" | "bibliography"
        | "bibliographystyle" | "author" | "date" | "thanks" | "tag" | "phantom" | "hphantom"
        | "vphantom" | "intertext" | "sout" | "uline" | "enquote" | "appendixname" | "keywords"
        | "email" | "affiliation" | "address" | "subtitle" => "m",
        "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "dbinom" | "tbinom" | "stackrel"
        | "overset" | "underset" | "setlength" | "setcounter" | "addtocounter" | "newcounter"
        | "renewcommand*" => "mm",
        "title" | "caption" | "footnote" | "footnotetext" | "color" | "usepackage"
        | "RequirePackage" | "documentclass" | "addbibresource" | "xrightarrow" | "xleftarrow"
        | "shortauthor" => "om",
        "sqrt" => "om",
        "textcolor" | "colorbox" => "omm",
        "hspace" | "vspace" | "operatorname" | "includegraphics" => "*om",
        "cite" | "citep" | "citet" | "parencite" | "textcite" | "autocite" | "footcite"
        | "citeauthor" | "citeyear" | "citealt" | "citealp" | "nocite" | "smartcite" | "Cite"
        | "Citep" | "Citet" | "Parencite" | "Textcite" | "Autocite" => "*oom",
        "newcommand" | "renewcommand" | "providecommand" => "*moom",
        "newenvironment" | "renewenvironment" => "*moomm",
        "newtheorem" => "*momo",
        "DeclareMathOperator" => "*mm",
        "item" => "o",
        "\\" => "*o",
        "makebox" | "framebox" | "parbox" | "raisebox" => "oom",
        "href" => "mm",
        _ => "",
    }
}

/// The arguments of environment `name` after `\begin{name}`.
pub fn environment(name: &str) -> &'static str {
    match name {
        "tabular" | "array" | "longtable" | "subfigure" | "minipage" | "subtable" => "om",
        "tabularx" | "tabulary" => "mom",
        "tabular*" => "mom",
        "wrapfigure" | "wraptable" => "omom",
        "figure" | "figure*" | "table" | "table*" | "itemize" | "enumerate" | "description"
        | "theorem" | "lemma" | "proof" | "definition" | "corollary" | "proposition"
        | "example" | "remark" | "lstlisting" | "Verbatim" | "BVerbatim" => "o",
        "minted" => "om",
        "thebibliography" | "multicols" | "multicols*" | "alignat" | "alignat*" => "m",
        _ => "",
    }
}

/// Whether environment `name` keeps its body as it is.
pub fn is_verbatim(name: &str) -> bool {
    matches!(
        name,
        "verbatim"
            | "verbatim*"
            | "Verbatim"
            | "Verbatim*"
            | "BVerbatim"
            | "lstlisting"
            | "minted"
            | "comment"
            | "filecontents"
            | "filecontents*"
    )
}

/// Whether the body of environment `name` is math.
pub fn is_math(name: &str) -> bool {
    matches!(
        name.trim_end_matches('*'),
        "equation"
            | "align"
            | "gather"
            | "multline"
            | "eqnarray"
            | "alignat"
            | "flalign"
            | "math"
            | "displaymath"
            | "split"
            | "aligned"
            | "gathered"
            | "alignedat"
            | "cases"
            | "dcases"
            | "matrix"
            | "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "smallmatrix"
            | "array"
    )
}

/// Whether command `name` is a sectioning command: an environment left
/// open ends at the next one.
pub fn is_sectioning(name: &str) -> bool {
    matches!(
        name,
        "part"
            | "chapter"
            | "section"
            | "subsection"
            | "subsubsection"
            | "paragraph"
            | "subparagraph"
    )
}

/// Whether the arguments of command `name` are text even in math.
pub fn text_arguments(name: &str) -> bool {
    matches!(
        name,
        "text"
            | "textrm"
            | "textbf"
            | "textit"
            | "textsf"
            | "texttt"
            | "textnormal"
            | "mbox"
            | "hbox"
            | "intertext"
            | "emph"
            | "textup"
    )
}
