#!/usr/bin/env python3
"""What pdflatex typesets for each line after `\\clearpage` in a .tex file,
one line each: tests/latex/references/references.expected from
references.tex.

Each such line is typeset in a box (`\\showbox`) and its glyphs are read
back from the log, so no PDF text extraction is needed.

    tools/latex-typeset.py tests/latex/references/references.tex
"""
import os, re, shutil, subprocess, sys, tempfile

src = sys.argv[1]
text = open(src).read()
head, body = text.split('\\clearpage', 1)
probes = [l for l in body.split('\n') if l.strip() and '\\end{document}' not in l]
defs = ('\\showboxdepth=100 \\showboxbreadth=100000 \\scrollmode\n'
        '\\newcommand\\probe[2]{\\typeout{PROBE{#1}}\\setbox0\\hbox{#2}\\showbox0}\n')
doc = (head.replace('\\begin{document}', defs + '\\begin{document}') + '\\clearpage\n'
       + '\n'.join('\\probe{%d}{%s}' % (i, p) for i, p in enumerate(probes))
       + '\n\\end{document}\n')
tmp = tempfile.mkdtemp()
with open(os.path.join(tmp, 'p.tex'), 'w') as f:
    f.write(doc)
# The bibliography files beside the document, for bibtex.
here = os.path.dirname(os.path.abspath(src))
for name in os.listdir(here):
    if name.endswith('.bib'):
        shutil.copy(os.path.join(here, name), tmp)
run = lambda *cmd: subprocess.run(list(cmd), cwd=tmp, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL)
run('pdflatex', '-interaction=nonstopmode', 'p.tex')
if '\\bibliography{' in doc:
    run('bibtex', 'p')
for _ in range(2):
    run('pdflatex', '-interaction=nonstopmode', 'p.tex')
log = open(os.path.join(tmp, 'p.log'), encoding='latin-1').read()
shutil.rmtree(tmp)
out = []
for m in re.finditer(r'PROBE\{(\d+)\}\s*\n(.*?)\n! OK', log, re.S):
    s = ''
    for line in m.group(2).split('\n'):
        g = re.match(r'^\.+\\[A-Z0-9]+/[^ ]+ (.*)$', line)
        if g:
            ch = g.group(1)
            s += 'fi' if ch.startswith('^^L') else ch
        elif re.match(r'^\.+\\glue', line) and 'skip' not in line:
            s += ' '
    out.append(re.sub(' +', ' ', s).strip())
dst = re.sub(r'\.tex$', '.expected', src)
with open(dst, 'w') as f:
    f.write('\n'.join(out) + '\n')
print(dst)
