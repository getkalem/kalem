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
        | "bibliographystyle" | "date" | "thanks" | "phantom" | "hphantom" | "vphantom"
        | "intertext" | "sout" | "uline" | "appendixname" | "keywords" | "email"
        | "affiliation" | "address" | "subtitle" | "vref" | "Vref" | "cpageref"
        | "refstepcounter" | "stepcounter" | "subfile" | "graphicspath" | "IEEEauthorblockN"
        | "IEEEauthorblockA" | "IEEEmembership" | "institution" | "department" | "city"
        | "state" | "country" | "streetaddress" | "postcode" | "orcid" | "institute" | "inst"
        | "pacs" => "m",
        "IEEEPARstart" => "mm",
        // algorithmicx's (algpseudocode) and algorithmic's statements.
        "If" | "ElsIf" | "For" | "ForAll" | "While" | "Until" | "Comment" | "IF" | "ELSIF"
        | "FOR" | "FORALL" | "WHILE" | "UNTIL" | "COMMENT" => "m",
        "Procedure" | "Function" | "Call" => "mm",
        "index" => "om",
        "nomenclature" => "omm",
        "glossary" | "ensuremath" | "indexsee" => "m",
        "ccsdesc" | "ead" | "affil" => "om",
        "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "dbinom" | "tbinom" | "stackrel"
        | "overset" | "underset" | "setlength" | "setcounter" | "addtocounter"
        | "texorpdfstring" | "import" | "subimport" => "mm",
        "tag" => "*m",
        "numberwithin" => "omm",
        "counterwithin" | "counterwithout" => "*mm",
        "captionof" => "*mom",
        // siunitx.
        "num" | "si" | "unit" | "ang" | "numlist" => "om",
        "qty" | "numrange" => "omm",
        "SI" => "omom",
        "SIrange" | "qtyrange" => "ommm",
        "addcontentsline" => "mmm",
        "declaretheorem" => "omo",
        // subfig's `\subfloat`, the subfigure package's `\subfigure` and
        // `\subtable` (as commands; subcaption's are environments).
        "subfloat" | "subfigure" | "subtable" => "oom",
        // Glossaries and acronyms (glossaries, acronym, acro): entries
        // and their uses.
        "newglossaryentry" | "DeclareAcronym" => "mm",
        "longnewglossaryentry" => "mmm",
        "newacronym" => "ommm",
        "acro" | "acrodef" | "newacro" => "mom",
        "gls" | "Gls" | "GLS" | "glspl" | "Glspl" | "GLSpl" | "glssymbol" | "glsentryname"
        | "glsentrytext" | "glsentryshort" | "glsentrylong" | "acrshort" | "acrlong"
        | "acrfull" | "Acrshort" | "Acrlong" | "Acrfull" | "acrshortpl" | "acrlongpl"
        | "acrfullpl" | "glsxtrshort" | "glsxtrlong" | "glsxtrfull" => "om",
        "ac" | "Ac" | "acs" | "acl" | "Acl" | "acf" | "Acf" | "acp" | "Acp" | "acsp" | "aclp"
        | "Aclp" | "acfp" | "Acfp" | "acused" => "sm",
        "bibitem" | "hyperref" | "includepdf" => "om",
        // amsart's `\author[short]{name}`.
        "author" => "om",
        "newcounter" => "mo",
        "footnotemark" => "o",
        "enquote" => "*m",
        "hspace" | "vspace" | "operatorname" => "*m",
        "raisebox" => "moom",
        "parbox" => "ooomm",
        "title" | "caption" | "footnote" | "footnotetext" | "color" | "usepackage"
        | "RequirePackage" | "documentclass" | "addbibresource" | "xrightarrow" | "xleftarrow"
        | "shortauthor" => "om",
        "sqrt" => "om",
        "textcolor" | "colorbox" => "omm",
        "includegraphics" => "*om",
        "cite" | "citep" | "citet" | "parencite" | "textcite" | "autocite" | "footcite"
        | "citeauthor" | "citeyear" | "citealt" | "citealp" | "citetalias" | "citepalias"
        | "nocite" | "smartcite" | "Cite" | "Citep" | "Citet" | "Parencite" | "Textcite"
        | "Autocite" | "citeyearpar" | "citenum" | "Citeauthor" | "citetitle" | "fullcite"
        | "supercite" | "footcitetext" | "cites" | "parencites" | "textcites" | "autocites"
        | "footcites" | "Cites" | "Parencites" | "Textcites" | "Autocites" | "citealt*"
        | "Citealt" | "Citealp" => "*oom",
        "newcommand" | "renewcommand" | "providecommand" | "DeclareRobustCommand" => "*moom",
        "NewDocumentCommand"
        | "RenewDocumentCommand"
        | "ProvideDocumentCommand"
        | "DeclareDocumentCommand" => "mmm",
        "newenvironment" | "renewenvironment" => "*moomm",
        "newtheorem" => "*momo",
        "DeclareMathOperator" => "*mm",
        "DeclarePairedDelimiter" => "mmm",
        "item" => "o",
        "\\" => "*o",
        "makebox" | "framebox" => "oom",
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
        "thebibliography" | "multicols" | "multicols*" | "alignat" | "alignat*" | "xalignat"
        | "xxalignat" => "m",
        "IEEEeqnarray" | "IEEEeqnarray*" | "empheq" => "om",
        "dmath" | "dmath*" => "o",
        "multlined" => "oo",
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
            | "xalignat"
            | "xxalignat"
            | "IEEEeqnarray"
            | "IEEEeqnarraybox"
            | "dmath"
            | "dseries"
            | "dgroup"
            | "darray"
            | "empheq"
            | "multlined"
            | "rcases"
            | "drcases"
            | "CD"
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
