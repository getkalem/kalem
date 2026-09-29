
# Table of Contents

1.  [Foundations](#part:foundations)
    1.  [Why plain text](#sec:plain-text)
    2.  [Measurements](#sec:measurements)
-   [References](#org5f46c53)

This book is written in Org and edited with *Kalem*. It keeps each
chapter in a file of its own; this file pulls them together with
`#+INCLUDE` and holds the bibliography.


<a id="part:foundations"></a>

# Foundations


<a id="sec:plain-text"></a>

## Why plain text

Books have been typeset from plain text for decades (Knuth, Donald E., 1984).
Org keeps the structure of a document in its text: headings, lists,
tables and formulas are all written as characters on a line, so the file
outlives every program that edits it.<sup><a id="fnr.-0-1" class="footref" href="#fn.-0-1" role="doc-backlink">1</a></sup>


<a id="sec:pipeline"></a>

### A pipeline

The text goes through one exporter for each format, as
[4](#org0711760) shows: HTML for the web, LaTeX for print, and Word
through pandoc for colleagues who ask for it (Dominik, Carsten, 2010).

![img](figures/pipeline.png "From one Org file to several formats.")

The code that builds the book can live in the book itself, as in
literate programming (Schulte, Eric and Davison, Dan and Dye, Thomas and Dominik, Carsten, 2012):

    kalem export book.org --to html
    kalem export book.org --to pdf
    kalem export book.org --to docx


<a id="sec:measurements"></a>

## Measurements

[1](#org9dc24ab) lists how long each export of this sample took, and the
total is computed by the table itself. The energy of a body at rest,
equation [1](#org71602d6), is the usual example of a displayed
formula.<sup><a id="fnr.-1-formula" class="footref" href="#fn.-1-formula" role="doc-backlink">2</a></sup>

<table id="org9dc24ab" border="2" cellspacing="0" cellpadding="6" rules="groups" frame="hsides">
<caption class="t-above"><span class="table-number">Table 1:</span> Export times of the sample, in milliseconds.</caption>

<colgroup>
<col  class="org-left" />

<col  class="org-right" />

<col  class="org-right" />

<col  class="org-right" />
</colgroup>
<thead>
<tr>
<th scope="col" class="org-left">Format</th>
<th scope="col" class="org-right">First run</th>
<th scope="col" class="org-right">Second run</th>
<th scope="col" class="org-right">Mean</th>
</tr>
</thead>
<tbody>
<tr>
<td class="org-left">HTML</td>
<td class="org-right">12</td>
<td class="org-right">10</td>
<td class="org-right">11</td>
</tr>

<tr>
<td class="org-left">LaTeX</td>
<td class="org-right">15</td>
<td class="org-right">13</td>
<td class="org-right">14</td>
</tr>

<tr>
<td class="org-left">Word</td>
<td class="org-right">420</td>
<td class="org-right">400</td>
<td class="org-right">410</td>
</tr>
</tbody>
<tbody>
<tr>
<td class="org-left">Total</td>
<td class="org-right">447</td>
<td class="org-right">423</td>
<td class="org-right">435</td>
</tr>
</tbody>
</table>

\begin{equation}
\label{org71602d6}
E = m c^2
\end{equation}

Inline mathematics works too: the mean of $n$ runs is
$\bar{x} = \frac{1}{n}\sum_{i=1}^{n} x_i$. Back in
[the first chapter](#sec:plain-text), the pipeline of
[1.1.1](#sec:pipeline) produced these files.


<a id="org5f46c53"></a>

# References

Dominik, Carsten (2010). *The Org Mode 7 Reference Manual*, Network Theory.

Knuth, Donald E. (1984). *The \TeXbook*, Addison-Wesley.

Schulte, Eric and Davison, Dan and Dye, Thomas and Dominik, Carsten (2012). *A Multi-Language Computing Environment for Literate Programming and Reproducible Research*.


# Footnotes

<sup><a id="fn.1" href="#fnr.1">1</a></sup> Plain text also diffs well, so a book can be reviewed like code.

<sup><a id="fn.2" href="#fnr.2">2</a></sup> See (Knuth, Donald E., 1984 p. 168) for how TeX sets displayed
equations.
