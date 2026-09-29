//! Templates for new LaTeX documents (T2.7h.27): the standard classes, a
//! presentation, a letter, a CV, a thesis and a Turkish thesis laid out as
//! the Council of Higher Education (YÖK) guidelines ask (A4, margins, one
//! and a half spacing, ÖZET and ABSTRACT pages). Each compiles with a
//! basic TeX Live; Kalem writes a copy, never the template itself.

/// A template: its name, what it is, and its text.
#[derive(Debug, Clone, Copy)]
pub struct Template {
    /// The name, also the file name offered.
    pub name: &'static str,
    /// What it is, for the picker.
    pub title: &'static str,
    /// The text.
    pub text: &'static str,
}

/// The templates.
pub const TEMPLATES: &[Template] = &[
    Template {
        name: "article",
        title: "Article",
        text: r"\documentclass[11pt,a4paper]{article}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage{amsmath,amssymb}
\usepackage{graphicx}
\usepackage{hyperref}

\title{Title}
\author{Author}
\date{\today}

\begin{document}
\maketitle

\begin{abstract}
  A summary of the article.
\end{abstract}

\section{Introduction}
\label{sec:introduction}

Text.

\end{document}
",
    },
    Template {
        name: "report",
        title: "Report",
        text: r"\documentclass[11pt,a4paper]{report}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage{amsmath,amssymb}
\usepackage{graphicx}
\usepackage{hyperref}

\title{Title}
\author{Author}
\date{\today}

\begin{document}
\maketitle
\tableofcontents

\chapter{Introduction}
\label{ch:introduction}

Text.

\end{document}
",
    },
    Template {
        name: "book",
        title: "Book",
        text: r"\documentclass[11pt,a4paper]{book}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage{amsmath,amssymb}
\usepackage{graphicx}
\usepackage{hyperref}

\title{Title}
\author{Author}
\date{\today}

\begin{document}
\frontmatter
\maketitle
\tableofcontents

\mainmatter
\chapter{First Chapter}
\label{ch:first}

Text.

\backmatter
\end{document}
",
    },
    Template {
        name: "beamer",
        title: "Presentation (beamer)",
        text: r"\documentclass{beamer}
\usepackage[utf8]{inputenc}

\title{Title}
\author{Author}
\date{\today}

\begin{document}

\begin{frame}
  \titlepage
\end{frame}

\begin{frame}{First Slide}
  \begin{itemize}
    \item A point
  \end{itemize}
\end{frame}

\end{document}
",
    },
    Template {
        name: "letter",
        title: "Letter",
        text: r"\documentclass[11pt,a4paper]{letter}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}

\signature{Your Name}
\address{Street \\ City}

\begin{document}

\begin{letter}{Recipient \\ Street \\ City}
\opening{Dear Sir or Madam,}

Text of the letter.

\closing{Yours faithfully,}
\end{letter}

\end{document}
",
    },
    Template {
        name: "cv",
        title: "Curriculum vitae",
        text: r"\documentclass[11pt,a4paper]{article}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage[margin=2cm]{geometry}
\usepackage{hyperref}
\pagestyle{empty}
\setlength{\parindent}{0pt}

\begin{document}

{\LARGE\textbf{Your Name}}\\[2pt]
email@example.org \quad +00 000 000 00 00

\section*{Education}
\textbf{Degree}, University \hfill 2020--2024

\section*{Experience}
\textbf{Position}, Company \hfill 2024--
\begin{itemize}
  \item What you did
\end{itemize}

\section*{Skills}
Languages, tools.

\end{document}
",
    },
    Template {
        name: "thesis",
        title: "Thesis",
        text: r"\documentclass[12pt,a4paper,oneside]{report}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage[margin=2.5cm]{geometry}
\usepackage{setspace}
\usepackage{amsmath,amssymb,amsthm}
\usepackage{graphicx}
\usepackage{hyperref}
\onehalfspacing

\newtheorem{theorem}{Theorem}[chapter]

\title{Title of the Thesis}
\author{Author}
\date{\today}

\begin{document}
\maketitle

\begin{abstract}
  A summary of the thesis.
\end{abstract}

\tableofcontents

\chapter{Introduction}
\label{ch:introduction}

Text.

\chapter{Conclusion}
\label{ch:conclusion}

Text.

\end{document}
",
    },
    Template {
        name: "tez",
        title: "Tez (YÖK biçimi)",
        text: r"\documentclass[12pt,a4paper,oneside]{report}
\usepackage[utf8]{inputenc}
\usepackage[T1]{fontenc}
\usepackage[turkish]{babel}
\usepackage[top=3cm,bottom=2.5cm,left=3.5cm,right=2.5cm]{geometry}
\usepackage{setspace}
\usepackage{amsmath,amssymb}
\usepackage{graphicx}
\usepackage{hyperref}
\onehalfspacing

\begin{document}

\begin{titlepage}
  \centering
  {\large T.C.\\ ÜNİVERSİTE ADI\\ FEN BİLİMLERİ ENSTİTÜSÜ\par}
  \vspace{3cm}
  {\Large\textbf{TEZİN BAŞLIĞI}\par}
  \vspace{2cm}
  {\large YÜKSEK LİSANS TEZİ\par}
  \vspace{1cm}
  {\large Yazarın Adı SOYADI\par}
  \vfill
  {\large Anabilim Dalı\\ Danışman: Unvan Adı SOYADI\par}
  \vspace{1cm}
  {\large Şehir, Yıl\par}
\end{titlepage}

\chapter*{ÖZET}
\addcontentsline{toc}{chapter}{ÖZET}
Tezin Türkçe özeti.

\textbf{Anahtar Kelimeler:} kelime, kelime

\chapter*{ABSTRACT}
\addcontentsline{toc}{chapter}{ABSTRACT}
The abstract of the thesis in English.

\textbf{Keywords:} word, word

\tableofcontents

\chapter{GİRİŞ}
\label{bol:giris}

Metin.

\chapter{SONUÇ}
\label{bol:sonuc}

Metin.

\end{document}
",
    },
];

/// The template named `name`.
pub fn find(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

/// A file for a new document from `template` in `dir`: `article.tex`, or
/// `article-2.tex` when that is taken.
pub fn free_path(dir: &std::path::Path, template: &str) -> std::path::PathBuf {
    let first = dir.join(format!("{template}.tex"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{template}-{n}.tex")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse_cleanly() {
        for t in TEMPLATES {
            let p = latex_syntax::parse(t.text);
            assert!(
                p.diagnostics().is_empty(),
                "{}: {:?}",
                t.name,
                p.diagnostics()
            );
            let m = latex_model::Model::new(&p);
            assert!(m.class.is_some() && m.body.is_some(), "{}", t.name);
        }
    }

    #[test]
    fn templates_compile() {
        let search = std::env::var_os("PATH").unwrap_or_default();
        if crate::pdf::detect(crate::pdf::Engine::PdfLatex, &search).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kalem-templates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Babel's Turkish is not in every TeX installation.
        let turkish = std::process::Command::new("kpsewhich")
            .arg("turkish.ldf")
            .output()
            .is_ok_and(|o| !o.stdout.is_empty());
        for t in TEMPLATES.iter().filter(|t| turkish || t.name != "tez") {
            let f = dir.join(format!("{}.tex", t.name));
            std::fs::write(&f, t.text).unwrap();
            let built = crate::latex_build::build(&f, crate::pdf::Engine::PdfLatex, None).unwrap();
            let errors: Vec<_> = built
                .problems
                .iter()
                .filter(|p| p.severity == crate::latex_build::Severity::Error)
                .collect();
            assert!(
                errors.is_empty() && built.pdf.is_some(),
                "{}: {errors:?}",
                t.name
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
