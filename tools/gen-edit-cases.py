#!/usr/bin/env python3
"""Generate the Org editing differential cases (tests/edit/cases.json).

Each case is a document, a cursor position and an Emacs form. Run
`tools/edit-expected.sh` afterwards to compute the Emacs results.
"""
import json, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

DOCS = {
    "outline": """#+TITLE: Outline
* First :a:
Text of first.
** Child one
*** Grandchild :deep:
** Child two                                                          :b:c:
* Second
SCHEDULED: <2026-10-01 Thu>
:PROPERTIES:
:ID: 2
:END:
Body.

* Third with blank lines


** Third's child
""",
    "tags": """* TODO A task with tags                                            :work:urgent:
** Short :x:
** A much longer headline title that pushes the tags far to the right :long:
*** 日本語の見出し :jp:
* Title with trailing spaces   
* COMMENT Commented :c:
""",
    "single": """* Only headline
Body line.
""",
    "odd": """#+STARTUP: odd
* One
*** Three
***** Five :t:
*** Three again
* One again
""",
    "noeol": """* A
text
** B
* C
last line without newline""",
    "inline": """* Task
Text before.
*************** TODO Inline task :it:
Inside.
*************** END
** Child
""",
    "siblings": """* S1
** a
body a


** b
body b
** c


* S2 :x:y:
""",
}

HEADLINE_FORMS = [
    "(org-do-promote)",
    "(org-do-demote)",
    "(org-promote-subtree)",
    "(org-demote-subtree)",
    "(org-move-subtree-up)",
    "(org-move-subtree-down)",
]


def byte_offsets_of_lines(text):
    out, pos = [], 0
    for line in text.encode("utf-8").split(b"\n"):
        out.append((pos, line))
        pos += len(line) + 1
    return out


def cases():
    out = []
    for doc_name, text in DOCS.items():
        for start, line in byte_offsets_of_lines(text):
            if not line.startswith(b"*"):
                continue
            # At the start of the headline and in its title.
            for point in (start, start + min(len(line), 4)):
                for form in HEADLINE_FORMS:
                    out.append({"name": f"{doc_name}@{point} {form}", "text": text, "point": point, "mark": None, "form": form, "cmd": form.strip("()"), "args": []})
                out.append({"name": f"{doc_name}@{point} cut", "text": text, "point": point, "mark": None, "form": "(org-cut-subtree)", "cmd": "org-cut-subtree", "args": []})
    # Copy a subtree and paste it at every line start and inside headings.
    for doc_name in ("outline", "siblings", "odd", "tags"):
        text = DOCS[doc_name]
        lines = byte_offsets_of_lines(text)
        sources = [s for s, l in lines if l.startswith(b"*")][:3]
        for src in sources:
            for dst, l in lines:
                for p in {dst, dst + min(len(l), 2)}:
                    form = f"(progn (org-copy-subtree) (goto-char (kalem-edit--pos {p})) (org-paste-subtree))"
                    out.append({"name": f"{doc_name} copy@{src} paste@{p}", "text": text, "point": src, "mark": None, "form": form, "cmd": "copy-paste", "args": [p]})
    # Level indicators: a line of stars where the tree goes.
    text = "* A\n** B\n*** C\n* D\n"
    for stars in ("*", "**", "****"):
        t2 = text + stars + "\n"
        p = len(t2.encode()) - 1
        form = f"(progn (goto-char (point-min)) (forward-line 1) (org-copy-subtree) (goto-char (kalem-edit--pos {p})) (org-paste-subtree))"
        out.append({"name": f"indicator {stars}", "text": t2, "point": 0, "mark": None, "form": form, "cmd": "copy-paste-line2", "args": [p]})
    out.extend(sort_cases())
    return out


SORT_DOCS = {
    "alpha": """#+TITLE: Sorting
* Parent
** zeta
** *Alpha* bold
** TODO beta :tag:
** [#A] Gamma
** COMMENT delta
** [[https://x.y][Epsilon]] link
** ~code~ eta
** 10 items
** 9 items
** -3 negative
** 2.5 float
** Ärger umlaut
** alpha lower
* Second parent
""",
    "dates": """* Tasks
** A
SCHEDULED: <2026-10-05 Mon> DEADLINE: <2026-10-01 Thu>
** B
[2026-09-01 Tue 10:00]
Text <2026-09-30 Wed 09:00>
** C
DEADLINE: <2026-09-29 Tue 12:00>
Created:
  [2026-08-15 Sat]
** D
no dates
*** D child <2020-01-01 Wed>
** E
<2026-09-28 Mon 11:00>
** F [2026-01-01 Thu]
""",
    "todo": """#+TODO: TODO NEXT | DONE CANCELLED
* Project
** DONE finished
** TODO first
** [#C] no kw low
** CANCELLED stop
** NEXT [#A] next
** plain
** TODO [#B] second
** TODOS not a keyword
""",
    "props": """* Items
** One
:PROPERTIES:
:ORDER: 3
:END:
** Two
:PROPERTIES:
:ORDER: 1
:END:
** Three
** Four
:PROPERTIES:
:ORDER: 10
:END:
""",
    "clock": """* Work
** Small
:LOGBOOK:
CLOCK: [2026-09-01 Tue 10:00]--[2026-09-01 Tue 10:30] =>  0:30
:END:
** Big
CLOCK: [2026-09-01 Tue 10:00]--[2026-09-01 Tue 13:00] =>  3:00
** None
** Child clocked
*** Sub
CLOCK: [2026-09-02 Wed 10:00]--[2026-09-02 Wed 11:00] =>  1:00
""",
    "top": """#+TITLE: T
Intro.
* b
text b

* a
* c :x:
text c without newline""",
    "blank": """* P
** b


** a


* Q
""",
    "above": """* P
*** deep
** a
** b
""",
    "empty": """* P
Only text.
* Q
""",
    "pretext": """Text before any heading.
""",
    "skip": """* P
*** c
*** a
* Q
**** z
**** y
""",
}

SORT_TYPES = "aAnNtTcCsSdDpPoOkK"

# The sorting types each document is meant for; every document also gets
# all of them at its start and at its first heading.
SORT_DOC_TYPES = {
    "alpha": "aAnN",
    "dates": "tTcCsSdD",
    "todo": "oOpPa",
    "props": "rRa",
    "clock": "kK",
}


def sort_form(t, with_case=False, prop=None):
    args = f"{'t' if with_case else 'nil'} ?{t}" + (f' nil nil "{prop}"' if prop else "")
    return f"(let ((org-sort-function #'org-sort-function-fallback)) (org-sort-entries {args}))"


def sort_case(name, text, point, mark, t, with_case=False):
    prop = "ORDER" if t in "rR" else None
    return {"name": name, "text": text, "point": point, "mark": mark, "form": sort_form(t, with_case, prop), "cmd": "sort", "args": [t, with_case, prop]}


def sort_cases():
    out = []
    docs = dict(SORT_DOCS)
    for k in ("outline", "siblings", "noeol", "inline"):
        docs[k] = DOCS[k]
    for doc_name, text in docs.items():
        size = len(text.encode())
        starts = [s for s, _ in byte_offsets_of_lines(text) if s <= size]
        first = next((s for s, l in byte_offsets_of_lines(text) if l.startswith(b"*")), 0)
        for point in starts:
            types = SORT_TYPES + "rR" if point in (0, first) else SORT_DOC_TYPES.get(doc_name, "aAn")
            for t in types:
                out.append(sort_case(f"{doc_name}@{point} sort {t}", text, point, None, t))
        out.append(sort_case(f"{doc_name} sort a with case", text, 0, None, "a", True))
        for i in range(0, len(starts), 3):
            for j in range(i + 1, len(starts), 4):
                out.append(sort_case(f"{doc_name} region {starts[i]}-{starts[j]} sort a", text, starts[j], starts[i], "a"))
    out.extend(random_sort_cases())
    out.extend(todo_cases())
    out.extend(repeat_cases())
    out.extend(property_cases())
    out.extend(adapt_cases())
    out.extend(tag_cases())
    out.extend(list_cases())
    out.extend(random_list_cases())
    out.extend(emphasis_cases())
    out.extend(insert_cases())
    out.extend(table_cases())
    out.extend(convert_cases())
    out.extend(random_table_cases())
    out.extend(create_table_cases())
    out.extend(typing_cases())
    out.extend(narrow_cases())
    return out


NARROW_DOC = """#+TITLE: N
Intro.
* B second
** z
** a
text
* A first :t:
- item one
- item two
  more
| x | y |
|---+---|
#+begin_quote
quoted
#+end_quote
#+begin_src sh
ls
#+end_src
*************** Inline
body
*************** END
* TODO C last"""


def narrow_cases():
    out = []
    text = NARROW_DOC
    lines = byte_offsets_of_lines(text)
    points = sorted({p for s, l in lines for p in (s, s + min(len(l), 3))})
    mark = "(let ((b (point-min)) (e (point-max))) (widen) (goto-char e) (insert \"]\") (goto-char b) (insert \"[\"))"
    for p in points:
        for kind in ("subtree", "element", "block"):
            form = f"(progn (org-narrow-to-{kind}) {mark})"
            out.append({"name": f"narrow {kind}@{p}", "text": text, "point": p, "mark": None, "form": form, "cmd": "narrow", "args": [kind]})
        for inner, args in [("(org-sort-entries nil ?a)", ["sort"]), ("(org-move-subtree-down)", ["move-down"]), ("(org-move-subtree-up)", ["move-up"]), ("(org-promote-subtree)", ["promote"]), ("(org-todo 'right)", ["todo"])]:
            form = f"(unwind-protect (progn (org-narrow-to-subtree) {inner}) (widen))"
            out.append({"name": f"narrowed {inner}@{p}", "text": text, "point": p, "mark": None, "form": form, "cmd": "narrowed", "args": args})
    return out


def random_table_cases(n=120, seed=5):
    """Random tables and random table commands."""
    import random
    rnd = random.Random(seed)
    cells = ["1", "22", "-3.5", "x", "long text", "", "[[https://a.b][ln]]", "[[t]]", "日本", "<r>", "12:30", "nan", "a b"]
    out = []
    for k in range(n):
        ind = rnd.choice(["", "", "  "])
        ncols = rnd.randint(1, 4)
        lines = ["Before."] if rnd.random() < 0.5 else []
        for r in range(rnd.randint(1, 5)):
            if rnd.random() < 0.2:
                lines.append(ind + rnd.choice(["|-", "|---+---|", "|--"]))
                continue
            row = [rnd.choice(cells) for _ in range(rnd.randint(1, ncols + 1))]
            sep = rnd.choice(["|", " | ", "| "])
            line = ind + "|" + sep.join(f" {c} " if rnd.random() < 0.7 else c for c in row)
            if rnd.random() < 0.8:
                line += "|"
            lines.append(line)
        if rnd.random() < 0.3:
            lines.append("#+TBLFM: $2=$1+1::@2$1=3::$3=remote(x, @1$1)")
        text = "\n".join(lines) + ("\n" if rnd.random() < 0.8 else "")
        raw = text.encode()
        table_lines = [(s0, l) for s0, l in byte_offsets_of_lines(text) if l.lstrip().startswith(b"|")]
        if not table_lines:
            continue
        boundaries = {len(text[:i].encode()) for i in range(len(text) + 1)}
        for _ in range(5):
            s0, l = rnd.choice(table_lines)
            p = s0 + rnd.randint(0, len(l))
            while p not in boundaries:
                p -= 1
            form, args = rnd.choice(TABLE_FORMS)
            out.append({"name": f"random-table {k}@{p} {form}", "text": text, "point": p, "mark": None, "form": form, "cmd": "table", "args": args})
    return out


TABLE_DOCS = {
    "table": """Intro text.
| Name | Qty | Price |
|------+-----+-------|
| apple | 3 | 1.5 |
| [[https://orgmode.org][Org]] | 10 | x |
| 日本 |  | -2 |
|---|
| <r> | <c> | <l5> |
| a | b | c | d |
#+TBLFM: $3=$2*2::@2$1=vsum(@3$2..@4$2)::$4=remote(t, @1$1)

  | indented | table |
  |-
  | x
Last.
""",
    "tail": """| a |bb|
|1|2""",
    "sorting": """| Name | Qty | When |
|------+-----+------|
| pear | 10 | <2026-03-01 Sun> |
| Apple | 2.5 | [2025-12-31 Wed 10:00] |
| apple | -3 | 1:30 |
| [[https://x.org][zed]] | 1e2 | 2h |
| banana |  | 0:45 |
| Äpfel | 10 | 3d |
|------+-----+------|
| total | 7 | 12:00 |
| b | x2 | <2026-01-01 Thu> |
""",
    "numbers": """| n | v |
|---+---|
| 1 | 2.5e3 |
| 0x1F | nan |
| 12:30 | 3% |
| -inf | text |
""",
}

TABLE_FORMS = [
    ("(org-table-align)", ["align"]),
    ("(org-table-insert-row)", ["insert-row", False]),
    ("(org-table-insert-row t)", ["insert-row", True]),
    ("(org-table-kill-row)", ["kill-row"]),
    ("(org-table-move-row)", ["move-row", False]),
    ("(org-table-move-row t)", ["move-row", True]),
    ("(org-table-insert-hline)", ["hline", False]),
    ("(org-table-insert-hline t)", ["hline", True]),
    ("(org-table-insert-column)", ["insert-column"]),
    ("(org-table-delete-column)", ["delete-column"]),
    ("(org-table-move-column)", ["move-column", False]),
    ("(org-table-move-column t)", ["move-column", True]),
    ("(org-table-next-field)", ["next-field"]),
    ("(org-table-previous-field)", ["previous-field"]),
    ("(org-table-next-row)", ["next-row"]),
    ("(org-table-sort-lines nil ?a)", ["sort-rows", "a"]),
    ("(org-table-sort-lines nil ?A)", ["sort-rows", "A"]),
    ("(org-table-sort-lines nil ?n)", ["sort-rows", "n"]),
    ("(org-table-sort-lines nil ?N)", ["sort-rows", "N"]),
    ("(org-table-sort-lines nil ?t)", ["sort-rows", "t"]),
    ("(org-table-sort-lines nil ?T)", ["sort-rows", "T"]),
    ("(org-table-sort-lines t ?a)", ["sort-rows", "a", True]),
]


CONVERT_TEXTS = [
    "a,b,c\n1,2,3\n",
    "x\ty\tz\n1\t\t3\n",
    "a b  c\nd e\n",
    "\"a, b\",c\n\"x \"\"y\"\"\",z\n",
    " lead , trail ,\n,,\n",
    "one line\n",
    "a,b\n\nc,d\n",
    "name  qty\tprice\nfoo  1\t2\n",
    "Before\na,b\nAfter",
    "\"unclosed,x\ny, \"q\"\n",
]

EXPORT_TABLES = [
    "| ! | a | b |\n|---+---+---|\n| # | 1, 2 | say \"hi\" |\n|   | x |  |\n| $ | p=1 | |\n|---+---+---|\n| / | <r> | 3 |\n",
    "| a | b |\n|---+---|\n| [[https://x.org][link]] | *bold* |\n| <l> | <5> |\n| 1 |\n",
    "| x |\n",
]


# Fields that keep a tab are aligned by Emacs with tab stops, which depend
# on the column the field starts at; Kalem counts a tab as one column.
TAB_WIDTH = "tabs in fields are measured with tab stops by org-table-align"


def convert_cases():
    out = []
    for k, text in enumerate(CONVERT_TEXTS):
        for sep, arg in [("nil", "auto"), ("'(4)", "comma"), ("'(16)", "tab"), ("2", "spaces2")]:
            form = f"(org-table-convert-region (point-min) (point-max) {sep})"
            case = {"name": f"convert {k} {sep}", "text": text, "point": 0, "mark": None, "form": form, "cmd": "convert", "args": [arg]}
            if "\t" in text and arg in ("comma", "spaces2"):
                case["known"] = TAB_WIDTH
            out.append(case)
    for k, text in enumerate(EXPORT_TABLES):
        for fmt in ["csv", "tsv"]:
            form = f"(let ((s (orgtbl-to-{fmt} (org-table-to-lisp) nil))) (erase-buffer) (insert s \"\\n\"))"
            out.append({"name": f"export {k} {fmt}", "text": text, "point": 2, "mark": None, "form": form, "cmd": "export", "args": [fmt]})
    return out


def table_cases():
    out = []
    for doc_name, text in TABLE_DOCS.items():
        for start, line in byte_offsets_of_lines(text):
            if not line.lstrip().startswith(b"|"):
                continue
            bars = [i for i, c in enumerate(line) if c == ord("|")]
            points = {start, start + len(line)}
            for b in bars[:3]:
                points.add(start + b + 1)
                points.add(start + min(b + 3, len(line)))
            boundaries = {len(text[:i].encode()) for i in range(len(text) + 1)}
            for p in sorted(points & boundaries):
                for form, args in TABLE_FORMS:
                    out.append({"name": f"{doc_name}@{p} {form}", "text": text, "point": p, "mark": None, "form": form, "cmd": "table", "args": args})
    return out


CREATE_TABLE_DOC = """* Heading
Some text here.

   
  indented line
| a | b |
Last line"""


# `org-table-create' in the middle of a line breaks it but inserts the rule
# before the new table and aligns from the line before it.
CREATE_TABLE_MIDLINE_BUG = "org-table-create mid-line puts the rule before the table"


def blank_before(text, p):
    raw = text.encode()
    bol = raw.rfind(b"\n", 0, p) + 1
    return raw[bol:p].strip(b" \t") == b""


def create_table_cases():
    out = []
    text = CREATE_TABLE_DOC
    boundaries = {len(text[:i].encode()) for i in range(len(text) + 1)}
    points = set()
    for start, line in byte_offsets_of_lines(text):
        for d in (0, 1, 2, 3, 4, len(line)):
            if d <= len(line):
                points.add(start + d)
    for p in sorted(points & boundaries):
        for size in ("3x2", "1x1", "2x3"):
            c, r = (int(x) for x in size.split("x"))
            case = {"name": f"create-table@{p} {size}", "text": text, "point": p, "mark": None, "form": f'(org-table-create "{size}")', "cmd": "table", "args": ["create", c, r]}
            if not blank_before(text, p):
                case["known"] = CREATE_TABLE_MIDLINE_BUG
            out.append(case)
    for t in ("", "x", "\n", "  "):
        for p in sorted({0, len(t.encode())}):
            case = {"name": f"create-table {t!r}@{p}", "text": t, "point": p, "mark": None, "form": '(org-table-create "2x2")', "cmd": "table", "args": ["create", 2, 2]}
            if not blank_before(t, p):
                case["known"] = CREATE_TABLE_MIDLINE_BUG
            out.append(case)
    return out


TYPING_DOCS = {
    "typing-table": """Intro.
| Name  | Qty | Note     |
|-------+-----+----------|
| apple |   3 | [[https://x.org][link]] |
|x|1|cramped|
  | indented | t |
- item
  | in | list |
* Heading one                                                        :tag:
* Tagged :a:b:
* Plain ü heading
Text line.
""",
}


def typing_cases():
    out = []
    for doc_name, text in TYPING_DOCS.items():
        raw = text.encode()
        boundaries = {len(text[:i].encode()) for i in range(len(text) + 1)}
        for start, line in byte_offsets_of_lines(text):
            for p in range(start, start + len(line) + 1):
                if p not in boundaries:
                    continue
                table = line.lstrip().startswith(b"|")
                forms = [
                    ("(let ((last-command-event ?x)) (org-self-insert-command 1))", ["insert", "x", False]),
                    ("(org-delete-backward-char 1)", ["backspace"]),
                    ("(org-delete-char 1)", ["delete"]),
                ]
                if table:
                    forms.append(("(let ((last-command-event ?y) (last-command 'org-cycle)) (org-self-insert-command 1))", ["insert", "y", True]))
                if p % 5 == 0:
                    forms.append(("(let ((last-command-event ?ü)) (org-self-insert-command 1))", ["insert", "ü", False]))
                for form, args in forms:
                    out.append({"name": f"{doc_name}@{p} {args[0]}{' blank' if args[-1] is True else ''}", "text": text, "point": p, "mark": None, "form": form, "cmd": "type", "args": args})
    return out


INSERT_DOC = """* Heading
Some text here.
  Indented line.
* Starred line in body
#+keyword: x
,* already escaped

Meeting <2026-09-21 Mon 10:00 +1w -2d> and [2026-09-01 Tue] and <2026-09-01 Tue>--<2026-09-03 Thu>.
Last line"""


TIMESTAMP_RANGE_BUG = ("org-timestamp next to (not on) a timestamp, on a line with a date range after "
                       "it: its range regexp spans several timestamps, and `replace-match' then "
                       "replaces the next timestamp with stale match data, adding the repeater of "
                       "another one. Kalem inserts at point")


def insert_cases():
    out = []
    text = INSERT_DOC
    raw = text.encode()
    lines = byte_offsets_of_lines(text)
    starts = [s for s, _ in lines]
    points = sorted({p for s, l in lines for p in (s, s + min(len(l), 5), s + len(l))})
    links = [("https://orgmode.org", "Org"), ("https://orgmode.org", None), ("file:a[1].org", "d]] x]"), ("<https://x.y>", None), ("c:\\dir\\", None)]
    for p in points:
        for link, desc in links:
            d = f'"{desc}"' if desc else "nil"
            ls = link.replace("\\", "\\\\").replace('"', '\\"')
            form = f'(org-insert-link nil "{ls}" {d})'
            out.append({"name": f"insert link {link}@{p}", "text": text, "point": p, "mark": None, "form": form, "cmd": "insert", "args": ["link", link, desc]})
    for i in range(0, len(starts) - 1):
        b, e = starts[i] + 2, starts[i + 1] + 1
        form = '(org-insert-link nil "https://orgmode.org" nil)'
        out.append({"name": f"insert link region {b}-{e}", "text": text, "point": e, "mark": b, "form": form, "cmd": "insert", "args": ["link", "https://orgmode.org", None]})
    for block in ("src", "quote", "SRC", "src python", "example", "export html", "center"):
        for p in points:
            form = f'(org-insert-structure-template "{block}")'
            out.append({"name": f"insert block {block}@{p}", "text": text, "point": p, "mark": None, "form": form, "cmd": "insert", "args": ["block", block]})
        for i in range(0, len(starts) - 1, 2):
            for j in (i + 1, i + 3):
                if j < len(starts):
                    b, e = starts[i], starts[j]
                    form = f'(org-insert-structure-template "{block}")'
                    out.append({"name": f"insert block {block} region {b}-{e}", "text": text, "point": e, "mark": b, "form": form, "cmd": "insert", "args": ["block", block]})
    stub = "(cl-letf (((symbol-function 'org-read-date) (lambda (&rest _) (encode-time 0 30 14 5 10 2026)))) {})"
    ts_line = [s for s, l in lines if l.startswith(b"Meeting")][0]
    ts_points = sorted(set(points) | {ts_line + k for k in (8, 15, 20, 30, 42, 50, 62, 70, 80, 93)})
    for p in ts_points:
        for arg, inactive in (("nil", False), ("'(4)", False), ("nil t", True)):
            with_time = arg == "'(4)"
            form = stub.format(f"(org-timestamp {arg})")
            case = {"name": f"insert timestamp {arg}@{p}", "text": text, "point": p, "mark": None, "form": form, "cmd": "insert", "args": ["timestamp", with_time, inactive]}
            if p in (ts_line + 42, ts_line + 62):
                case["known"] = TIMESTAMP_RANGE_BUG
            out.append(case)
    return out


EMPHASIS_DOC = """Some words here, (paren) and "quoted".
x*y and a-b; end.
*already bold* and =code=
日本語の文 mixed text
"""


def emphasis_cases():
    out = []
    text = EMPHASIS_DOC
    raw = text.encode()
    # Region boundaries at every character boundary of a few spans.
    marks = [0, 5, 6, 10, 15, 17, 23, 30, 39, 40, 41, 43, 50, 57, 72, 78, 85, 97]
    boundaries = {len(text[:i].encode()) for i in range(len(text) + 1)}
    marks = sorted({m for m in marks if m in boundaries} | {len(text[:i].encode()) for i in (84, 87, 90)})
    for c in "*/_=~+ ":
        form_char = "?\\s" if c == " " else f"?{c}"
        form = f"(org-emphasize {form_char})"
        for p in marks:
            out.append({"name": f"emph {c!r}@{p}", "text": text, "point": p, "mark": None, "form": form, "cmd": "emphasize", "args": [c]})
        for i, b in enumerate(marks):
            for e in marks[i + 1:i + 4]:
                out.append({"name": f"emph {c!r} {b}-{e}", "text": text, "point": e, "mark": b, "form": form, "cmd": "emphasize", "args": [c]})
    return out


def random_list_cases(n=150, seed=11):
    """Random nested lists with every kind of item, and random commands."""
    import random
    rnd = random.Random(seed)
    out = []
    for k in range(n):
        lines = ["* Heading [/]"] if rnd.random() < 0.6 else []
        depth_ind = [0]
        count = rnd.randint(2, 9)
        for _ in range(count):
            level = min(len(depth_ind), rnd.choice([0, 0, 1, 1, 2]))
            ind = rnd.choice([0, 0, 2]) if level == 0 else depth_ind[level - 1] + rnd.choice([2, 3, 4])
            depth_ind = depth_ind[:level] + [ind]
            bullet = rnd.choice(["-", "+", "1.", "2)", "*" if ind > 0 else "-", "10."])
            parts = [bullet]
            if bullet[0].isdigit() and rnd.random() < 0.15:
                parts.append(f"[@{rnd.randint(1, 20)}]")
            if rnd.random() < 0.5:
                parts.append(rnd.choice(["[ ]", "[X]", "[-]"]))
            text = rnd.choice(["alpha", "beta gamma", "term :: definition", "x", "long item text here"])
            parts.append(text)
            prefix = "\t" if ind >= 8 and rnd.random() < 0.5 else " " * ind
            lines.append(prefix + " ".join(parts))
            r = rnd.random()
            if r < 0.15:
                lines.append(" " * (ind + 2) + "continuation line")
            elif r < 0.22:
                lines.append("")
            elif r < 0.25:
                lines += [" " * (ind + 2) + "#+begin_example", " " * (ind + 2) + "- not an item", " " * (ind + 2) + "#+end_example"]
        if rnd.random() < 0.3:
            lines += ["", "Paragraph after."]
        text = "\n".join(lines) + "\n"
        starts = [s for s, l in byte_offsets_of_lines(text) if l.lstrip()[:1] in (b"-", b"+", b"*", b"1", b"2") and not l.startswith(b"*")]
        if not starts:
            continue
        for _ in range(4):
            form, args = rnd.choice(LIST_FORMS)
            p = rnd.choice(starts)
            p += rnd.choice([0, 2, 4])
            p = min(p, len(text.encode()) - 1)
            case = {"name": f"random-list {k}@{p} {form}", "text": text, "point": p, "mark": None, "form": form, "cmd": "list", "args": args}
            if case["name"] in KNOWN_LIST_BUGS:
                case["known"] = KNOWN_LIST_BUGS[case["name"]]
            out.append(case)
    return out


LIST_DOCS = {
    "lists": """* Lists [0/0]
- [ ] one
- [X] two
  1. sub a
  2. sub b
     text of b
- three
  + tag :: description
  + other :: desc

1. [ ] first
3. [ ] second
10. [@10] tenth
11. eleventh

- item with block
  #+begin_src sh
  - not an item
  #+end_src
- after block

    - indented top
    - indented two
""",
    "boxes": """* Checkboxes [/] [%]
- [ ] a
  - [X] a1
  - [ ] a2
- [X] b
- [-] c
  - [X] c1
* Ordered [/]
:PROPERTIES:
:ORDERED: t
:END:
- [ ] first
- [ ] second
- [ ] third
* Plain
- x
- y
""",
    "blanks": """Intro.

- a

- b
  - b1

- c
""",
}

# Cases where Emacs is wrong and Kalem does what Emacs means to do.
INSERT_BOX_BUG = ("org-list-insert-item ignores the width of a new checkbox when it shifts the "
                  "sub-items of the item it splits, and then indents the wrong lines")
CYCLE_HEADING_BUG = ("org-cycle-list-bullet can leave a `*' bullet at column 0, which makes the item "
                     "a headline; Emacs then fails with a type error while restoring the cursor "
                     "(the text is the same)")
KNOWN_LIST_BUGS = {
    "random-list 33@153 (org-cycle-list-bullet 'previous)": CYCLE_HEADING_BUG,
    "lists@244 (org-insert-item t)": INSERT_BOX_BUG,
    "lists@254 (org-insert-item t)": INSERT_BOX_BUG,
    "lists@255 (org-insert-item t)": INSERT_BOX_BUG,
}

LIST_FORMS = [
    ("(org-indent-item)", ["indent", True, True]),
    ("(org-outdent-item)", ["indent", False, True]),
    ("(org-indent-item-tree)", ["indent", True, False]),
    ("(org-outdent-item-tree)", ["indent", False, False]),
    ("(org-cycle-list-bullet)", ["bullet", "next"]),
    ("(org-cycle-list-bullet 'previous)", ["bullet", "previous"]),
    ("(org-cycle-list-bullet \"1.\")", ["bullet", "1."]),
    ("(org-toggle-checkbox)", ["checkbox", "toggle"]),
    ("(org-toggle-checkbox '(4))", ["checkbox", "presence"]),
    ("(org-toggle-checkbox '(16))", ["checkbox", "partial"]),
    ("(org-move-item-down)", ["move", True]),
    ("(org-move-item-up)", ["move", False]),
    ("(org-list-repair)", ["repair"]),
    ("(org-insert-item)", ["insert", False]),
    ("(org-insert-item t)", ["insert", True]),
]


def list_cases():
    out = []
    for doc_name, text in LIST_DOCS.items():
        lines = byte_offsets_of_lines(text)
        for start, line in lines:
            if start >= len(text.encode()):
                continue
            ws = len(line) - len(line.lstrip())
            points = {start + min(len(line), ws + 3), start + len(line)}
            if line.startswith(b"*"):
                points = {start + 2}
            for p in sorted(points):
                for form, args in LIST_FORMS:
                    case = {"name": f"{doc_name}@{p} {form}", "text": text, "point": p, "mark": None, "form": form, "cmd": "list", "args": args}
                    if case["name"] in KNOWN_LIST_BUGS:
                        case["known"] = KNOWN_LIST_BUGS[case["name"]]
                    out.append(case)
        # Regions over a few items.
        item_starts = [s for s, l in lines if l.lstrip().startswith((b"- ", b"+ ", b"1", b"3", b"10"))]
        for i in range(0, len(item_starts) - 1, 3):
            b, e = item_starts[i], item_starts[i + 1] + 1
            for form, args in [("(org-indent-item)", ["indent", True, True]), ("(org-outdent-item-tree)", ["indent", False, False]), ("(org-toggle-checkbox)", ["checkbox", "toggle"])]:
                out.append({"name": f"{doc_name} region {b}-{e} {form}", "text": text, "point": e, "mark": b, "form": form, "cmd": "list", "args": args})
    return out


TAG_DOC = """* Plain
* Tagged :a:b:
* TODO Aligned                                                        :work:
* Trailing spaces   
*
* :only:
** Deep title that is quite long and pushes the tags beyond the tags column :x:
*************** Inline :it:
*************** END
* 日本語 :jp:
* Title :not:tags :real:
"""


def tag_cases():
    out = []
    text = TAG_DOC
    forms = [
        ("(org-set-tags '(\"a\" \"b\"))", ["set", ["a", "b"]]),
        ("(org-set-tags nil)", ["set", []]),
        ("(org-set-tags \":x:y:\")", ["set", ["x", "y"]]),
        ("(org-toggle-tag \"work\")", ["toggle", "work", None]),
        ("(org-toggle-tag \"a\")", ["toggle", "a", None]),
        ("(org-toggle-tag \"a\" 'on)", ["toggle", "a", True]),
        ("(org-toggle-tag \"new\" 'off)", ["toggle", "new", False]),
    ]
    lines = byte_offsets_of_lines(text)
    for start, line in lines:
        if not line.startswith(b"*"):
            continue
        for form, args in forms:
            # `org-set-tags' works on the current line; the command goes to
            # the heading first.
            if form.startswith("(org-set-tags"):
                form = f"(save-excursion (org-back-to-heading t) {form})"
            out.append({"name": f"tags@{start} {form}", "text": text, "point": start, "mark": None, "form": form, "cmd": "tags", "args": args})
    starts = [s for s, _ in lines]
    for i in range(0, len(starts), 2):
        for j in range(i, len(starts), 3):
            for off in (False, True):
                b, e = starts[i], min(starts[j] + 2, len(text.encode()))
                form = f"(org-change-tag-in-region (kalem-edit--pos {b}) (kalem-edit--pos {e}) \"a\" {'t' if off else 'nil'})"
                out.append({"name": f"tags region {b}-{e} {off}", "text": text, "point": 0, "mark": None, "form": form, "cmd": "tags", "args": ["region", b, e, "a", off]})
    out.append({"name": "tags align all", "text": text.replace("  :work:", " :work:"), "point": 3, "mark": None, "form": "(org-align-tags t)", "cmd": "tags", "args": ["align"]})
    return out


def adapt_cases():
    """The same commands with `org-adapt-indentation' on."""
    out = []
    docs = {k: TODO_DOCS[k] for k in ("logdone", "drawer")}
    docs.update({k: REPEAT_DOCS[k] for k in ("repeat-seq",)})
    docs.update(PROPERTY_DOCS)
    for doc_name, text in docs.items():
        for start, line in byte_offsets_of_lines(text):
            if not line.startswith(b"*"):
                continue
            for form, args in [("(org-todo 'done)", {"todo": "done", "adapt": True}), ("(org-todo 'left)", {"todo": "left", "adapt": True})]:
                out.append({"name": f"{doc_name}@{start} adapt {form}", "text": text, "point": start, "mark": None, "form": f"(progn (setq-local org-adapt-indentation t) {form})", "cmd": "todo", "args": [args], "note": None})
            form = '(org-entry-put nil "NEW" "v")'
            out.append({"name": f"{doc_name}@{start} adapt {form}", "text": text, "point": start, "mark": None, "form": f"(progn (setq-local org-adapt-indentation t) {form})", "cmd": "entry-put", "args": ["NEW", "v", True]})
    return out


REPEAT_DOCS = {
    "repeat": """* TODO Weekly <2026-09-21 Mon +1w>
* TODO Scheduled weekly
SCHEDULED: <2026-09-21 Mon +1w>
* TODO Catch up
SCHEDULED: <2026-09-01 Tue ++1w>
* TODO Restart
DEADLINE: <2026-09-01 Tue .+2d> SCHEDULED: <2026-09-20 Sun>
* TODO Monthly end
SCHEDULED: <2026-01-31 Sat 10:00-11:00 +1m -2d>
* TODO Hourly
SCHEDULED: <2026-09-28 Mon 09:00 .+2h>
* TODO Hourly plus
SCHEDULED: <2026-09-28 Mon 08:00 ++3h>
* TODO Delay
DEADLINE: <2026-09-25 Fri +1d --2d>
* TODO No hour
SCHEDULED: <2026-09-28 Mon +1h>
* TODO Zero
SCHEDULED: <2026-09-28 Mon +0d>
* Plain repeat <2026-09-27 Sun +1d>
* TODO Clocked
SCHEDULED: <2026-09-21 Mon +1w>
:LOGBOOK:
CLOCK: [2026-09-20 Sun 10:00]--[2026-09-20 Sun 11:00] =>  1:00
:END:
* TODO Inactive only [2026-09-21 Mon +1w]
* TODO In src
#+begin_src org
<2026-09-21 Mon +1w>
#+end_src
* TODO Again
SCHEDULED: <2026-09-21 Mon +1w>
:PROPERTIES:
:LAST_REPEAT: [2026-09-14 Mon 10:00]
:ID:       x
:END:
* TODO Indented
  SCHEDULED: <2026-09-21 Mon +1w>
  :PROPERTIES:
  :ID:       y
  :END:
* TODO Two <2026-09-21 Mon +1w> and <2026-10-01 Thu +1y>
""",
    "repeat-seq": """#+TODO: TODO NEXT | DONE
#+STARTUP: logdone logdrawer
* NEXT Repeat
SCHEDULED: <2026-09-21 Mon +1w>
* NEXT To state
SCHEDULED: <2026-09-21 Mon +1w>
:PROPERTIES:
:REPEAT_TO_STATE: NEXT
:END:
""",
    "repeat-types": """#+TYP_TODO: Fred Sara | DONE
* Fred Repeat <2026-09-21 Mon +1w>
""",
    "repeat-log": """#+STARTUP: lognoterepeat
#+TODO: TODO | DONE(d!)
* TODO Noted
SCHEDULED: <2026-09-21 Mon +1w>
* TODO Last line <2026-09-21 Mon +1w>""",
}


def repeat_cases():
    out = []
    forms = [("(org-todo 'done)", {"todo": "done"}), ('(org-todo "DONE")', {"todo": "state:DONE"}), ("(let ((org-use-fast-todo-selection nil)) (org-todo))", {"todo": "cycle"})]
    for doc_name, text in REPEAT_DOCS.items():
        for start, line in byte_offsets_of_lines(text):
            if not line.startswith(b"*"):
                continue
            for form, args in forms:
                for note in (None, "Repeated"):
                    if note and doc_name != "repeat-log":
                        continue
                    out.append({"name": f"{doc_name}@{start} {form}{' +note' if note else ''}", "text": text, "point": start, "mark": None, "form": form, "cmd": "todo", "args": [args], "note": note})
    return out


PROPERTY_DOCS = {
    "props": """#+TITLE: P
* A
* B
SCHEDULED: <2026-10-01 Thu>
Body
* C
:PROPERTIES:
:ID:       c
:foo: old
:END:
* D
  :PROPERTIES:
  :ID: d
  :END:
* E
  SCHEDULED: <2026-10-01 Thu>
*************** Inline
*************** END
* Last""",
}


def property_cases():
    out = []
    puts = [("FOO", "bar"), ("ID", "new"), ("LONG_PROPERTY_NAME", "v"), ("EMPTY", ""), ("bad name", "x"), ("ITEM", "x")]
    for doc_name, text in PROPERTY_DOCS.items():
        for start, line in byte_offsets_of_lines(text):
            for key, value in puts:
                form = f'(org-entry-put nil "{key}" "{value}")'
                out.append({"name": f"{doc_name}@{start} {form}", "text": text, "point": start, "mark": None, "form": form, "cmd": "entry-put", "args": [key, value]})
    return out


TODO_DOCS = {
    "plain": """* Parent [1/3]
** TODO A
** DONE B
** C :tag:
** TODO  Double space
*** TODO Deep
* COMMENT Commented
* TODO [#A] With priority                                          :tag:
* TODO
* Last""",
    "keys": """#+TODO: TODO(t) NEXT(n!) WAIT(w@/!) | DONE(d!) CANCELLED(c@)
#+TODO: REPORT BUG | FIXED
* TODO First
* NEXT Second
text
* WAIT Third
:PROPERTIES:
:ID: 3
:END:
* BUG Fourth
* Fifth
""",
    "logdone": """#+STARTUP: logdone
* TODO Task
SCHEDULED: <2026-10-01 Thu>
Body
* DONE Closed task
CLOSED: [2026-09-01 Tue 10:00] SCHEDULED: <2026-09-01 Tue>
* TODO No planning
:PROPERTIES:
:X: 1
:END:
* TODO Last without newline""",
    "drawer": """#+STARTUP: logdone logdrawer
#+TODO: TODO(t!) WAIT(w@/!) | DONE(d!)
* TODO A
:LOGBOOK:
- State "TODO"       from              [2026-09-01 Tue 10:00]
:END:
* WAIT B
* TODO C
text

* TODO D
""",
    "reversed": """#+STARTUP: nologstatesreversed
#+TODO: TODO(t!) | DONE(d!)
* TODO A
- State "TODO"       from              [2026-09-01 Tue 10:00]
Text.
* TODO B
:PROPERTIES:
:LOGGING: nil
:END:
* TODO C
:PROPERTIES:
:LOGGING: logdone
:END:
""",
    "stats": """* Project [0/2] [0%]
** TODO a
** TODO b
*** TODO deep
* Recursive [/]
:PROPERTIES:
:COOKIE_DATA: todo recursive
:END:
** TODO x
*** TODO y
* Checkbox cookie [/]
:PROPERTIES:
:COOKIE_DATA: checkbox
:END:
** TODO z
""",
    "types": """#+TYP_TODO: Fred Sara Lucy | DONE
* Fred task
* Task
* DONE Finished
""",
}

TODO_FORMS = [
    ("(let ((org-use-fast-todo-selection nil)) (org-todo))", {"todo": "cycle"}),
    ("(org-todo 'right)", {"todo": "right"}),
    ("(org-todo 'left)", {"todo": "left"}),
    ("(org-todo 'done)", {"todo": "done"}),
    ("(org-todo 'none)", {"todo": "none"}),
    ("(org-todo 'nextset)", {"todo": "nextset"}),
    ("(org-todo 'previousset)", {"todo": "previousset"}),
    ('(org-todo "DONE")', {"todo": "state:DONE"}),
    ('(org-todo "WAIT")', {"todo": "state:WAIT"}),
    ('(org-todo "BOGUS")', {"todo": "state:BOGUS"}),
    ("(org-todo 3)", {"todo": "nth:3"}),
    ("(let ((org-use-fast-todo-selection nil)) (org-todo '(4)))", {"todo": "cycle", "force_note": True}),
    ("(let ((org-use-fast-todo-selection nil)) (org-todo 0))", {"todo": "cycle", "inhibit_note": True}),
    ("(org-priority 'up)", {"priority": "up"}),
    ("(org-priority 'down)", {"priority": "down"}),
    ("(org-priority ?A)", {"priority": "set:A"}),
    ("(org-priority ?c)", {"priority": "set:c"}),
    ("(org-priority ?Z)", {"priority": "set:Z"}),
    ("(org-priority 'remove)", {"priority": "remove"}),
]


def todo_cases():
    out = []
    for doc_name, text in TODO_DOCS.items():
        for start, line in byte_offsets_of_lines(text):
            if not line.startswith(b"*"):
                continue
            for form, args in TODO_FORMS:
                for note in (None, "Waiting for Bob\nsecond line"):
                    if note and not ("force_note" in args or "WAIT" in form or doc_name in ("keys", "drawer")):
                        continue
                    out.append({"name": f"{doc_name}@{start} {form}{' +note' if note else ''}", "text": text, "point": start, "mark": None, "form": form, "cmd": "todo", "args": [args], "note": note})
            # Inside the title, and in the stars.
            for p in (start + min(len(line), 7), start + 1):
                for form, args in TODO_FORMS[:2] + TODO_FORMS[13:14]:
                    out.append({"name": f"{doc_name}@{p} {form}", "text": text, "point": p, "mark": None, "form": form, "cmd": "todo", "args": [args], "note": None})
    return out


def random_sort_cases(n=100, seed=7):
    """Random entries with every kind of sort key, sorted from the parent."""
    import random
    rnd = random.Random(seed)
    words = ["apple", "Banana", "cherry", "*date*", "[[x][Elder]]", "~fig~", "10", "9", "-2", "2.5", "Érable", "zeta", "Alpha"]
    out = []
    for k in range(n):
        lines = ["#+TODO: TODO NEXT | DONE CANCELLED"] if rnd.random() < 0.5 else []
        lines.append("* P")
        for _ in range(rnd.randint(2, 7)):
            head = ["**"]
            if rnd.random() < 0.5:
                head.append(rnd.choice(["TODO", "DONE", "NEXT", "CANCELLED"]))
            if rnd.random() < 0.4:
                head.append(f"[#{rnd.choice('ABCD')}]")
            if rnd.random() < 0.1:
                head.append("COMMENT")
            head.append(" ".join(rnd.choice(words) for _ in range(rnd.randint(1, 3))))
            if rnd.random() < 0.3:
                head.append(":tag:")
            lines.append(" ".join(head))
            day = lambda: f"2026-{rnd.randint(1, 12):02d}-{rnd.randint(1, 28):02d}"
            if rnd.random() < 0.4:
                plan = []
                if rnd.random() < 0.7:
                    plan.append(f"SCHEDULED: <{day()}>")
                if rnd.random() < 0.5:
                    plan.append(f"DEADLINE: <{day()} {rnd.randint(0, 23)}:{rnd.choice(['00', '30'])}>")
                if plan:
                    lines.append(" ".join(plan))
            if rnd.random() < 0.4:
                lines += [":PROPERTIES:", f":ORDER: {rnd.choice(['1', '2', '10', 'b', 'A'])}", ":END:"]
            if rnd.random() < 0.3:
                h = rnd.randint(10, 12)
                lines.append(f"CLOCK: [2026-09-01 Tue 09:00]--[2026-09-01 Tue {h}:00] =>  {h - 9}:00")
            if rnd.random() < 0.4:
                lines.append(rnd.choice(["Body.", f"Text <{day()}>", f"[{day()}] created", f"  [{day()} Mon 9:15]"]))
            if rnd.random() < 0.2:
                lines.append("*** sub " + rnd.choice(words))
            if rnd.random() < 0.2:
                lines.append("")
        if rnd.random() < 0.5:
            lines.append("* Q")
        text = "\n".join(lines) + ("\n" if rnd.random() < 0.8 else "")
        point = text.encode().index(b"* P")
        for t in rnd.sample(SORT_TYPES + "rR", 5):
            out.append(sort_case(f"random {k} sort {t}", text, point, None, t))
    return out



DEPS_DOC = """#+TODO: TODO NEXT | DONE CANCELLED
* TODO Parent
** DONE a
** TODO b
*** TODO b1
* TODO Ordered
:PROPERTIES:
:ORDERED: t
:END:
** TODO first
** NEXT second
*** DONE s1
** DONE third
** fourth
* TODO Not ordered
:PROPERTIES:
:ORDERED: nil
:END:
** TODO x
** TODO y
* TODO Top
:PROPERTIES:
:ORDERED: t
:END:
** TODO one
** TODO two
*** TODO deep
**** DONE deeper
* TODO Free
:PROPERTIES:
:NOBLOCKING: t
:END:
** TODO child
* NEXT Boxes :home:
- [X] done
- [ ] open
  - [-] partial
* TODO Checked :x:
- [X] all
#+begin_example
- [ ] in a block
#+end_example
* Plain
** TODO under plain
** DONE done under plain
*** TODO open below done
"""

BLOCKERS = {
    "deps": ("(org-enforce-todo-dependencies t) (org-blocker-hook '(org-block-todo-from-children-or-siblings-or-parent))", {"enforce": True}),
    "boxes": ("(org-enforce-todo-checkbox-dependencies t) (org-blocker-hook '(org-block-todo-from-checkboxes))", {"enforce_checkbox": True}),
    "both": ("(org-enforce-todo-dependencies t) (org-enforce-todo-checkbox-dependencies t) (org-blocker-hook '(org-block-todo-from-children-or-siblings-or-parent org-block-todo-from-checkboxes))", {"enforce": True, "enforce_checkbox": True}),
    "triggers": ("(org-todo-state-tags-triggers '((done (\"home\") (\"closed\" . t)) (\"\" (\"x\") (\"none\" . t)) (\"NEXT\" (\"next\" . t)) (todo (\"closed\")) (\"CANCELLED\" (\"cancelled\" . t))))", {"triggers": [["done", [["home", False], ["closed", True]]], ["", [["x", False], ["none", True]]], ["NEXT", [["next", True]]], ["todo", [["closed", False]]], ["CANCELLED", [["cancelled", True]]]]}),
}

DEPS_FORMS = [
    ("(org-todo 'done)", {"todo": "done"}),
    ('(org-todo "CANCELLED")', {"todo": "state:CANCELLED"}),
    ("(org-todo 'right)", {"todo": "right"}),
    ("(org-todo 'none)", {"todo": "none"}),
    ('(org-todo "NEXT")', {"todo": "state:NEXT"}),
    ("(let ((org-use-fast-todo-selection nil)) (org-todo))", {"todo": "cycle"}),
]


def todo_dependency_cases():
    """`org-enforce-todo-dependencies', checkbox dependencies and
    `org-todo-state-tags-triggers'."""
    out = []
    for start, line in byte_offsets_of_lines(DEPS_DOC):
        if not line.startswith(b"*"):
            continue
        for key, (binding, extra) in BLOCKERS.items():
            for form, args in DEPS_FORMS:
                full = f"(let ({binding}) {form})"
                out.append({"name": f"deps {key}@{start} {form}", "text": DEPS_DOC, "point": start, "mark": None, "form": full, "cmd": "todo", "args": [dict(args, **extra)], "note": None})
    # `org-toggle-ordered-property', at the heading and inside the entry.
    for start, line in byte_offsets_of_lines(DEPS_DOC):
        out.append({"name": f"deps ordered@{start}", "text": DEPS_DOC, "point": start, "mark": None, "form": "(org-toggle-ordered-property)", "cmd": "toggle-ordered", "args": []})
    for text in ("* A\n:PROPERTIES:\n:ORDERED: t\n:ordered+: x\n:END:\nBody\n", "* A\nSCHEDULED: <2026-09-28 Mon>\n  :PROPERTIES:\n  :ID: 1\n  :ORDERED:  t\n  :END:\n", "* A\n:PROPERTIES:\n:ORDERED: nil\n:END:", "* A"):
        out.append({"name": f"ordered {text!r}", "text": text, "point": 2, "mark": None, "form": "(org-toggle-ordered-property)", "cmd": "toggle-ordered", "args": []})
    return out


FOOTNOTE_DOCS = [
    "#+TITLE: Notes\n\n* Intro\nText one[fn:1] and two[fn:note] and inline[fn:: anon def] and named[fn:inl: inline def].\nMore text[fn:3].\n\n[fn:1] First.\n\n* Second\nRefers again[fn:1] and new[fn:2].\n\n[fn:note] Named.\n[fn:3] Third, defined late.\n\n* Footnotes\n\n[fn:2] Two in section.\n",
    "Para[fn:3] then[fn:1].\nAnother[fn:2].\n\n[fn:1] One.\n[fn:2] Two.\n[fn:3] Three.\n[fn:9] Unreferenced.\n",
    "* A\nx[fn:a].\n\n[fn:a] See also[fn:b].\n\n[fn:b] Nested.\n* B\ny[fn:c] and missing[fn:zz].\n\n[fn:c] C def.\n",
    "Just text here.\n",
    "* H\nText",
    "* Head :tag:\nBody text.\n** Sub\n| a | b |\n|---+---|\n| 1 | 2 |\n- item one\n- item two\n#+begin_src sh\necho hi\n#+end_src\n: fixed\nEnd.\n",
    "Intro[fn:1].\n\n* Footnotes\n:PROPERTIES:\n:ID: x\n:END:\nSome text.\n\n[fn:1] Existing.\n* After\nMore[fn:7].\n",
]

FOOTNOTE_SECTIONS = [
    ("", "section"),
    ("(org-footnote-section nil)", "local"),
]


def footnote_cases():
    """`org-footnote.el': new, renumber, sort, normalize, delete and
    moving between references and definitions."""
    out = []
    for d, doc in enumerate(FOOTNOTE_DOCS):
        data = doc.encode()
        for binding, key in FOOTNOTE_SECTIONS:
            wrap = lambda form: f"(let ({binding}) {form})" if binding else form
            for form, cmd in (("(org-footnote-renumber-fn:N)", "fn-renumber"),
                              ("(org-footnote-sort)", "fn-sort"),
                              ("(org-footnote-normalize)", "fn-normalize")):
                if cmd == "fn-renumber" and binding:
                    continue
                out.append({"name": f"fn {d} {key} {cmd}", "text": doc, "point": 0, "mark": None, "form": wrap(form), "cmd": cmd, "args": [key]})
            # New footnotes: the start, middle and end of every line.
            for start, line in byte_offsets_of_lines(doc):
                for off in sorted({0, len(line) // 2, max(len(line) - 1, 0), len(line)}):
                    p = start + off
                    if p > len(data):
                        continue
                    while p < len(data) and (data[p] & 0xC0) == 0x80:
                        p += 1
                    out.append({"name": f"fn {d} {key} new@{p}", "text": doc, "point": p, "mark": None, "form": wrap("(org-footnote-new)"), "cmd": "fn-new", "args": [key]})
        # Delete and move, at every footnote.
        i = 0
        while True:
            i = data.find(b"[fn:", i)
            if i < 0:
                break
            for p in (i, i + 3):
                out.append({"name": f"fn {d} delete@{p}", "text": doc, "point": p, "mark": None, "form": "(org-footnote-delete)", "cmd": "fn-delete", "args": []})
                if b"[fn:zz]" != data[i:i + 7]:
                    out.append({"name": f"fn {d} action@{p}", "text": doc, "point": p, "mark": None, "form": "(org-footnote-action)", "cmd": "fn-action", "args": []})
            i += 1
    return out


# Cases where Emacs fails (book/part-2/org-known-differences.org, Footnotes).
KNOWN_FOOTNOTE_BUGS = {
    "fnr 14 local fn-normalize": "org-footnote-normalize takes the last anonymous footnote of the document for one nested in a definition, and extracts text past the end (Args out of range)",
    "fnr 14 section fn-normalize": "org-footnote-normalize takes the last anonymous footnote of the document for one nested in a definition, and extracts text past the end (Args out of range)",
    "fnr 51 local fn-normalize": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 51 local fn-sort": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 51 renumber": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 51 section fn-normalize": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 51 section fn-sort": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 52 local fn-normalize": "org-footnote-normalize takes the last anonymous footnote of the document for one nested in a definition, and extracts text past the end (Args out of range)",
    "fnr 52 section fn-normalize": "org-footnote-normalize takes the last anonymous footnote of the document for one nested in a definition, and extracts text past the end (Args out of range)",
    "fnr 69 local fn-normalize": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 69 local fn-sort": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 69 renumber": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 69 section fn-normalize": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)",
    "fnr 69 section fn-sort": "org-footnote--collect-references recurses without end on a definition that refers to itself (Lisp nesting exceeds max-lisp-eval-depth)"
}


def random_footnote_cases(n=90, seed=13):
    """Random documents of headings, paragraphs, references and
    definitions, for the footnote commands."""
    import random
    rnd = random.Random(seed)
    labels = ["1", "2", "3", "7", "a", "note", "x-y"]
    def para():
        words = []
        for _ in range(rnd.randint(1, 6)):
            r = rnd.random()
            w = rnd.choice(["word", "text", "more", "é", "end."])
            if r < 0.25:
                w += f"[fn:{rnd.choice(labels)}]"
            elif r < 0.3:
                w += "[fn:: inline " + rnd.choice(["one", "two"]) + "]"
            elif r < 0.33:
                w += f"[fn:{rnd.choice(labels)}: named]"
            words.append(w)
        return " ".join(words)
    out = []
    for i in range(n):
        lines = []
        for _ in range(rnd.randint(1, 9)):
            r = rnd.random()
            if r < 0.2:
                lines.append("*" * rnd.randint(1, 3) + " " + rnd.choice(["H", "Head", "Footnotes", "Notes"]))
            elif r < 0.45:
                lines.append(f"[fn:{rnd.choice(labels)}] Def {para()}")
            elif r < 0.6:
                lines.append("")
            else:
                lines.append(para())
        text = "\n".join(lines) + rnd.choice(["\n", "", "\n\n"])
        data = text.encode()
        for binding, key in FOOTNOTE_SECTIONS:
            wrap = lambda form: f"(let ({binding}) {form})" if binding else form
            for form, cmd in (("(org-footnote-sort)", "fn-sort"), ("(org-footnote-normalize)", "fn-normalize")):
                out.append({"name": f"fnr {i} {key} {cmd}", "text": text, "point": 0, "mark": None, "form": wrap(form), "cmd": cmd, "args": [key]})
            p = rnd.randint(0, len(data))
            while p < len(data) and (data[p] & 0xC0) == 0x80:
                p += 1
            out.append({"name": f"fnr {i} {key} new@{p}", "text": text, "point": p, "mark": None, "form": wrap("(org-footnote-new)"), "cmd": "fn-new", "args": [key]})
        out.append({"name": f"fnr {i} renumber", "text": text, "point": 0, "mark": None, "form": "(org-footnote-renumber-fn:N)", "cmd": "fn-renumber", "args": ["section"]})
        j = data.find(b"[fn:")
        if j >= 0:
            out.append({"name": f"fnr {i} delete@{j+2}", "text": text, "point": j + 2, "mark": None, "form": "(org-footnote-delete)", "cmd": "fn-delete", "args": []})
    for c in out:
        if c["name"] in KNOWN_FOOTNOTE_BUGS:
            c["known"] = KNOWN_FOOTNOTE_BUGS[c["name"]]
    return out


PLANNING_DOCS = [
    "* TODO Task\nBody\n",
    "* TODO Task\nSCHEDULED: <2026-10-01 Thu +1w>\nBody\n",
    "* DONE Task\nCLOSED: [2026-09-20 Sun 10:00] SCHEDULED: <2026-09-19 Sat>\n",
    "* Task :tag:\n  DEADLINE: <2026-10-10 Sat -2d> SCHEDULED: <2026-10-01 Thu .+2d/3d>\n  :PROPERTIES:\n  :ID: 1\n  :END:\nText\n* Next\n",
    "Before\n* A\n** B\nSCHEDULED: <2026-10-01 Thu>\nText SCHEDULED: <2026-10-02 Fri>\n",
    "* A",
    "* A\n:PROPERTIES:\n:X: 1\n:END:\nDEADLINE: <2026-11-01 Sun>\n",
]

PLANNING_FORMS = [
    ("(org-schedule nil \"2026-10-05\")", {"kind": "scheduled", "date": "2026-10-05", "time": False}),
    ("(org-schedule nil \"2026-10-05 14:30\")", {"kind": "scheduled", "date": "2026-10-05 14:30", "time": True}),
    ("(org-schedule nil \"<2026-10-05 Mon +1w>\")", {"kind": "scheduled", "date": "2026-10-05", "time": False, "repeater": "+1w"}),
    ("(org-deadline nil \"2026-12-24\")", {"kind": "deadline", "date": "2026-12-24", "time": False}),
    ("(org-deadline nil \"2026-12-24 09:00\")", {"kind": "deadline", "date": "2026-12-24 09:00", "time": True}),
    ("(org-schedule '(4))", {"kind": "scheduled", "remove": True}),
    ("(org-deadline '(4))", {"kind": "deadline", "remove": True}),
]


def planning_cases():
    """`org-schedule' and `org-deadline': setting and removing."""
    out = []
    for d, doc in enumerate(PLANNING_DOCS):
        for binding in ("", "(org-adapt-indentation t)"):
            for start, line in byte_offsets_of_lines(doc):
                for form, args in PLANNING_FORMS:
                    full = f"(let ({binding}) {form})" if binding else form
                    a = dict(args, adapt=bool(binding))
                    out.append({"name": f"plan {d} {bool(binding)}@{start} {form}", "text": doc, "point": start, "mark": None, "form": full, "cmd": "schedule", "args": [a]})
    return out


DRAWER_DOCS = [
    "* A\nSome text.\nMore text.\n\nLast.\n",
    "Plain line",
    "* A\n\n  Indented.\n\n* B\nx\n",
    "é text\nand more é\n",
]


# Regions Emacs re-indents (book/part-2/org-known-differences.org, Drawers).
KNOWN_DRAWER_DIFFERENCES = {
    "drawer 2 17-18": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 18-19": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 6-17": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 6-18": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 6-19": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 6-7": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 7-17": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 7-18": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce",
    "drawer 2 7-19": "org-insert-drawer indents the region with indent-for-tab-command while it is still active; a region starting or ending inside a line's indentation has its lines re-indented by org-indent-region, which Kalem does not reproduce"
}


def drawer_cases():
    """`org-insert-drawer' with a name, at point and around a region."""
    out = []
    for d, doc in enumerate(DRAWER_DOCS):
        data = doc.encode()
        offsets = [s for s, _ in byte_offsets_of_lines(doc)]
        points = sorted({p for s in offsets for p in (s, min(s + 2, len(data)))})
        points = [p for p in points if p <= len(data) and (p == len(data) or (data[p] & 0xC0) != 0x80)]
        for p in points:
            out.append({"name": f"drawer {d}@{p}", "text": doc, "point": p, "mark": None, "form": "(org-insert-drawer nil \"NOTES\")", "cmd": "drawer", "args": ["NOTES"]})
        for a in points:
            for b in points:
                if b > a:
                    out.append({"name": f"drawer {d} {a}-{b}", "text": doc, "point": b, "mark": a, "form": "(progn (transient-mark-mode 1) (activate-mark) (org-insert-drawer nil \"LOGBOOK\"))", "cmd": "drawer", "args": ["LOGBOOK"]})
    for c in out:
        if c["name"] in KNOWN_DRAWER_DIFFERENCES:
            c["known"] = KNOWN_DRAWER_DIFFERENCES[c["name"]]
    out.append({"name": "drawer bad name", "text": "x\n", "point": 1, "mark": None, "form": "(org-insert-drawer nil \"a b\")", "cmd": "drawer", "args": ["a b"]})
    return out

ARCHIVE_DOCS = [
    "* P [0/2]\n** TODO a\nbody a\n** TODO b\n* Q\ntext\n",
    "* Top\n** one\n** Archive :ARCHIVE:\n*** old\n** two :x:\n\n* Other\n",
    "Intro\n* A :tag:\n:PROPERTIES:\n:ID: 1\n:END:\nx\n* B\n\n\n* C\nno newline",
    "* Proj [%]\n** DONE x\n\n** TODO y\n*** sub\n\n** NEXT z\n",
    "* A\n** B\n*** C\nc\n** D\n* E\n",
    "* é one\n** Archive :ARCHIVE:\n** é two\ntext é\n",
]


def archive_cases():
    """`org-toggle-archive-tag', `org-archive-to-archive-sibling' and
    `org-refile' within the buffer."""
    out = []
    for d, doc in enumerate(ARCHIVE_DOCS):
        data = doc.encode()
        lines = byte_offsets_of_lines(doc)
        points = sorted({p for s, _ in lines for p in (s, min(s + 2, len(data)))})
        points = [p for p in points if p <= len(data) and (p == len(data) or (data[p] & 0xC0) != 0x80)]
        heads = [s for s, l in lines if l.startswith(b"*")]
        for p in points:
            out.append({"name": f"archive-tag {d}@{p}", "text": doc, "point": p, "mark": None, "form": "(org-toggle-archive-tag)", "cmd": "archive-tag", "args": []})
            out.append({"name": f"archive-sibling {d}@{p}", "text": doc, "point": p, "mark": None, "form": "(org-archive-to-archive-sibling)", "cmd": "archive-sibling", "args": []})
        for p in sorted({h + k for h in heads for k in (0, 2)} | {points[-1]}):
            for t in heads:
                form = ("(let ((f (make-temp-file \"kalem-refile\" nil \".org\")) (org-bookmark-names-plist nil))"
                        " (write-region nil nil f nil 'silent) (set-visited-file-name f t t)"
                        f" (unwind-protect (org-refile nil nil (list \"T\" f nil (kalem-edit--pos {t})))"
                        " (set-visited-file-name nil t) (delete-file f)))")
                out.append({"name": f"refile {d}@{p}->{t}", "text": doc, "point": p, "mark": None, "form": form, "cmd": "refile", "args": [t]})
    return out

HEADING_DOCS = [
    """#+TITLE: H
Intro text.
* TODO First :a:
Body one.
** Child
* DONE Second   :b:c:
Body two.
* Third""",
    """* Spaced

Body.

* Next

** Sub

* Last
""",
    """Text only, no heading.
More text.
""",
    """Before.

* A
text
""",
    """#+TODO: NEXT WAIT | DONE CANCELLED
* NEXT [#A] Task title here :x:
** WAIT Sub task
** Plain sub
*** Deep
* CANCELLED Gone
""",
    """* Task
*************** TODO Inline
inside
*************** END
after inline
""",
    """* A
- item
- [ ] box
* B""",
]

HEADING_FORMS = [
    ("(org-insert-heading)", ["here"]),
    ("(org-insert-heading '(4))", ["after"]),
    ("(org-insert-heading '(16))", ["parent"]),
    ("(org-insert-subheading nil)", ["sub"]),
    ("(org-insert-todo-heading nil)", ["todo", "here", False]),
    ("(org-insert-todo-heading '(4))", ["todo", "here", True]),
    ("(org-insert-todo-heading-respect-content)", ["todo", "after", False]),
]


def heading_cases():
    """`org-insert-heading' (M-RET, C-RET), `org-insert-subheading' and
    `org-insert-todo-heading' at the start, middle and end of each line."""
    out = []
    for d, doc in enumerate(HEADING_DOCS):
        data = doc.encode()
        points = set()
        for s, l in byte_offsets_of_lines(doc):
            for k in (0, 1, 2, 4, 7, len(l) // 2, len(l) - 4, len(l)):
                if 0 <= k <= len(l):
                    points.add(s + k)
        points = sorted(p for p in points if p <= len(data) and (p == len(data) or (data[p] & 0xC0) != 0x80))
        for p in points:
            for form, args in HEADING_FORMS:
                out.append({"name": f"heading {d}@{p} {form}", "text": doc, "point": p, "mark": None, "form": form, "cmd": "insert-heading", "args": args})
    return out

if __name__ == "__main__":
    path = os.path.join(ROOT, "tests/edit/cases.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump(cases() + todo_dependency_cases() + footnote_cases() + random_footnote_cases() + planning_cases() + drawer_cases() + archive_cases() + heading_cases(), f, ensure_ascii=False, indent=1)
        f.write("\n")
    print(f"{len(cases())} cases -> {path}")
