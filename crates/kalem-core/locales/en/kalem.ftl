# Kalem's user interface in English: the source strings.
# Keys are Fluent message IDs; translations live in ../<language>/kalem.ftl.

## Commands: titles in the palette and menus, by command ID.

cmd-app-save = Save
cmd-app-saveAs = Save As
cmd-app-quit = Quit
cmd-app-settings = Settings
cmd-app-revert = Revert to Saved
cmd-edit-copy = Copy
cmd-edit-cut = Cut
cmd-edit-paste = Paste
cmd-edit-pastePlain = Paste as Plain Text
cmd-edit-selectAll = Select All
cmd-org-link-open = Open Link
cmd-edit-enter = New Line or Item
cmd-edit-newline = Line Break
cmd-view-palette = Command Palette
cmd-find-open = Find
cmd-find-replace = Find and Replace
cmd-view-outline = Outline
cmd-view-toggleSource = Toggle Source View
cmd-view-split = Split View
cmd-view-focus = Focus Mode
cmd-view-toggleWrap = Toggle Soft Wrap
cmd-view-toggleMath = Toggle Formula Preview
cmd-format-font = Font
cmd-format-size = Font Size
cmd-format-grow = Grow Font
cmd-format-shrink = Shrink Font
cmd-format-color = Text Color
cmd-format-highlight = Highlight
cmd-format-clear = Clear Formatting
cmd-format-align = Align
cmd-format-alignLeft = Align Left
cmd-format-alignCenter = Center
cmd-format-alignRight = Align Right
cmd-format-justify = Justify
cmd-format-documentFont = Document Font
cmd-format-documentSize = Document Font Size
cmd-format-lineSpacing = Line Spacing
cmd-file-open = Open File
cmd-file-new = New Document
cmd-file-close = Close Document
cmd-file-next = Next Document
cmd-file-previous = Previous Document
cmd-file-switch = Switch Document
cmd-file-recent = Open Recent File
cmd-view-openFiles = Open Files
cmd-project-switch = Switch Project
cmd-project-findFile = Find File in Project
cmd-project-search = Search in Project
cmd-project-recentFiles = Recent Files in Project
cmd-project-switchDocument = Switch Document in Project
cmd-project-add = Add Project
cmd-project-remove = Remove Project
cmd-project-rename = Rename Project
cmd-project-refresh = Refresh Project Files
cmd-project-saveDocuments = Save Project Documents
cmd-project-closeDocuments = Close Project Documents
cmd-view-setMode = Set Document Mode
cmd-view-fold = Fold or Unfold
cmd-view-foldAll = Fold or Unfold All
cmd-edit-undo = Undo
cmd-edit-redo = Redo
cmd-org-headline-promote = Promote Headline
cmd-org-headline-demote = Demote Headline
cmd-org-headline-promoteSubtree = Promote Subtree
cmd-org-headline-demoteSubtree = Demote Subtree
cmd-org-headline-moveSubtreeUp = Move Subtree Up
cmd-org-headline-moveSubtreeDown = Move Subtree Down
cmd-org-headline-setLevel = Heading Level
cmd-org-headline-cutSubtree = Cut Subtree
cmd-org-headline-copySubtree = Copy Subtree
cmd-org-headline-pasteSubtree = Paste Subtree
cmd-org-headline-sort = Sort Entries
cmd-org-todo-cycle = Cycle TODO State
cmd-org-todo-next = Next TODO Keyword
cmd-org-todo-previous = Previous TODO Keyword
cmd-org-todo-set = Set TODO State
cmd-org-todo-done = Mark Done
cmd-org-todo-nextSet = Next Keyword Set
cmd-org-todo-previousSet = Previous Keyword Set
cmd-org-priority-up = Raise Priority
cmd-org-priority-down = Lower Priority
cmd-org-priority-set = Set Priority
cmd-org-priority-remove = Remove Priority
cmd-org-property-set = Set Property
cmd-org-tags-set = Set Tags
cmd-org-tags-toggle = Toggle Tag
cmd-org-tags-alignAll = Align All Tags
cmd-list-indent = Indent Item
cmd-list-outdent = Outdent Item
cmd-list-indentTree = Indent Item and Children
cmd-list-outdentTree = Outdent Item and Children
cmd-list-cycleBullet = Cycle Bullet
cmd-list-toggleCheckbox = Toggle Checkbox
cmd-list-moveUp = Move Item Up
cmd-list-moveDown = Move Item Down
cmd-list-insertItem = Insert Item
cmd-list-repair = Repair List
cmd-org-emphasis-bold = Bold
cmd-org-emphasis-italic = Italic
cmd-org-emphasis-underline = Underline
cmd-org-emphasis-strikeThrough = Strike Through
cmd-org-emphasis-code = Code
cmd-org-emphasis-verbatim = Verbatim
cmd-org-insert-link = Insert Link
cmd-org-insert-block = Insert Block
cmd-org-insert-timestamp = Insert Timestamp
cmd-org-insert-date = Insert Date
cmd-org-insert-horizontalRule = Insert Horizontal Rule
cmd-table-create = Insert Table
cmd-table-align = Align Table
cmd-table-recalculate = Recalculate Table
cmd-table-setFormula = Edit Formula
cmd-table-sortRows = Sort Rows
cmd-table-import = Import Table…
cmd-table-export = Export Table…
cmd-table-insertRow = Insert Row
cmd-table-killRow = Delete Row
cmd-table-moveRowUp = Move Row Up
cmd-table-moveRowDown = Move Row Down
cmd-table-insertHline = Insert Horizontal Rule
cmd-table-insertColumn = Insert Column
cmd-table-deleteColumn = Delete Column
cmd-table-moveColumnLeft = Move Column Left
cmd-table-moveColumnRight = Move Column Right
cmd-table-nextField = Next Field
cmd-table-previousField = Previous Field
cmd-table-nextRow = Next Row
cmd-view-narrowToSubtree = Narrow to Subtree
cmd-view-narrowToElement = Narrow to Element
cmd-view-narrowToBlock = Narrow to Block
cmd-view-widen = Widen

## Command categories.

category-edit = Edit
category-file = File
category-find = Find
category-format = Format
category-headlines = Headlines
category-insert = Insert
category-links = Links
category-lists = Lists
category-properties = Properties
category-todo = TODO
category-table = Table
category-tags = Tags
category-view = View

## Menus.

menu-file = File
menu-project = Project
menu-add-project-folder = Add Project Folder…
menu-edit = Edit
menu-format = Format
menu-insert = Insert
menu-view = View
menu-settings = Settings…
menu-quit = Quit Kalem
menu-open = Open…
menu-save-as = Save As…
menu-heading = Heading { $level }
menu-body-text = Body Text
menu-link = Link…
menu-table = Table
menu-timestamp = Timestamp
menu-date = Date…
menu-source-block = Source Block
menu-horizontal-rule = Horizontal Rule
menu-source-view = Source View

## The status bar.

status-modified = Modified
status-saved = Saved
status-position = Ln { $line }, Col { $column }
status-formula-lisp = Emacs Lisp formula, not computed
status-formula-error = #ERROR: { $why }
# $count is the number for plural rules; $shown is the same number with digit groups.
status-words = { $count ->
    [one] { $shown } word
   *[other] { $shown } words
}
status-words-section = { $words }, { $section } in section
number-group-separator = ,
mode-text = Text

## Messages.

msg-saved = Saved
msg-saved-as = Saved { $path }
msg-not-saved = Not saved: { $reason }
msg-copied = Copied
msg-exported = Exported to { $path }
msg-cannot-write = Cannot write { $path }: { $reason }
msg-lisp-formulas = Emacs Lisp formulas are kept but not computed: { $lhs }
msg-reloaded = Reloaded: the file changed on disk
msg-reloaded-from-disk = Reloaded from disk
msg-disk-conflict = The file changed on disk: saving keeps your version, Revert to Saved takes the other
msg-deleted-on-disk = The file was deleted on disk
msg-cannot-read = Cannot read the file: { $error }
msg-cannot-reload = Cannot reload: { $error }
msg-unknown-command = Unknown command { $id }
msg-does-not-apply = { $command } does not apply here
msg-not-bound = { $keys } is not bound
msg-no-match-for = No match for { $target }
msg-no-link = No link here
msg-opened = Opened { $target }
msg-cannot-open = Cannot open { $target }: { $error }
msg-replaced = Replaced { $count }
msg-no-matches = No matches
msg-nothing-selected = Nothing selected
msg-cancelled = Cancelled
msg-no-file-name = No file name
msg-unknown-mode = Unknown document mode: { $mode }
msg-mode-set = Document mode: { $mode }
msg-wrap-on = Long lines wrap
msg-wrap-off = Long lines do not wrap
msg-math-on = Formulas shown rendered
msg-math-off = Formulas shown as source
msg-focus-on = Focus mode: only this section shows
msg-focus-off = Focus mode off
msg-source-view = Source view
msg-rich-view = Rich view
msg-split-in-gui = Split view is in the graphical editor
msg-settings-in = Settings are in { $path }
msg-no-settings-dir = No settings directory
msg-setting-saved = Saved { $key }
msg-visibility-overview = Overview
msg-visibility-contents = Contents
msg-visibility-all = Show all
msg-formatting-removed = { $format } removed: its marker was deleted
msg-missing-argument = Missing argument `{ $name }`
msg-argument-number = { $name } must be a number
msg-no-document = No document
msg-not-org = Not an Org document
msg-nothing-to-undo = Nothing to undo
msg-nothing-to-redo = Nothing to redo
msg-not-a-date = Not a date: { $input }
msg-invalid-sort = Invalid sorting type
msg-config-problems = { $count ->
    [one] 1 problem
   *[other] { $count } problems
} in the settings or keymap; see the log
msg-empty-priority = Empty priority

## Formatting names, for messages.

format-bold = Bold
format-italic = Italic
format-underline = Underline
format-strike-through = Strike-through
format-code = Code
format-verbatim = Verbatim
format-link = Link

## Prompts and dialogs.

prompt-argument = { $command } { $name }:{" "}
prompt-save-as = Save as:{" "}
prompt-quit = Save changes before quitting? (y)es, (n)o, Esc to cancel{" "}
prompt-reload = The file changed on disk. Reload and lose your changes? (y/n){" "}
prompt-overwrite = The file changed on disk. Overwrite it? (y/n){" "}
answer-yes = y
answer-no = n
dialog-changed-on-disk = The file changed on disk.
dialog-overwrite-detail = Overwrite the changes made by another program?
dialog-overwrite = Overwrite
dialog-cancel = Cancel
dialog-save-before-quit = Save changes before quitting?
dialog-save-before-close = Save changes to { $name } before closing?
dialog-save-all-before-quit = { $count } documents have unsaved changes. Save them before quitting?
dialog-save = Save
dialog-dont-save = Don't Save

## Panels.

find-label = Find
find-regex-label = Find (regex)
replace-label = Replace
copy-button = Copy
settings-title = Settings
settings-theme = Theme
settings-theme-system = System
settings-theme-light = Light
settings-theme-dark = Dark
settings-keys = Keys
settings-keys-word = Word-like
settings-keys-vim = Vim
settings-size = Text size
settings-font = Font
settings-system-font = System font
settings-language = Language
settings-language-auto = System
settings-open-file = Open settings.toml

## The date picker.

weekday-short = Mo Tu We Th Fr Sa Su
month-1 = January
month-2 = February
month-3 = March
month-4 = April
month-5 = May
month-6 = June
month-7 = July
month-8 = August
month-9 = September
month-10 = October
month-11 = November
month-12 = December

## Vim keys.

vim-normal = NORMAL
vim-insert = -- INSERT --
vim-visual = -- VISUAL --
vim-visual-line = -- VISUAL LINE --
vim-replace = -- REPLACE --
vim-not-found = Pattern not found: { $pattern }
vim-not-a-command = Not an editor command: { $command }

## Open documents and projects.

category-project = Project
pick-documents = Open documents
pick-recent = Recent files
pick-projects = Projects
pick-remove-project = Remove which project
pick-project-files = Files of { $project }
pick-project-recent = Recent files of { $project }
pick-no-projects = No projects yet: Add Project adds the document's folder
pick-searching = searching…
pick-walking = listing files…
project-missing = missing
open-files = Open files
untitled = Untitled
search-project = Search { $project }
search-case = Aa
search-word = W
search-regex = .*
search-results = { $count } matches
search-error = Not a regular expression: { $error }
msg-project-added = Project { $name } added
msg-project-removed = Project { $name } removed from the list
msg-project-renamed = Project renamed { $name }
msg-no-project = Not in a project
msg-project-missing = The folder of project { $name } is gone
msg-cannot-save-projects = Cannot save the project list: { $error }
msg-cannot-open = Cannot open { $path }: { $error }
msg-saved-count = { $count } documents saved
prompt-close = Save { $name } before closing? (y)es, (n)o, Esc to cancel{" "}
prompt-quit-many = { $count } documents have unsaved changes. Save them before quitting? (y)es, (n)o, Esc to cancel{" "}
prompt-open = Open file
prompt-project-add = Add project folder
prompt-project-rename = Project name
status-commands = commands
msg-select-text = Select some text first
msg-cannot-format-here = Text here cannot be formatted (a table or a block)
msg-not-a-color = Not a color: { $color } (a name such as red, or #rrggbb)
msg-not-a-size = Not a font size: { $size }
msg-not-an-alignment = Not an alignment: { $align } (left, center, right or justify)
msg-not-a-paragraph = Not in a paragraph
toolbar-font = Font
toolbar-size = Size
toolbar-color = Text color
toolbar-highlight = Highlight color
toolbar-automatic = Automatic
toolbar-none = None
msg-not-a-spacing = Not a line spacing: { $spacing } (1, 1.15, 1.5, 2…)
toolbar-spacing = Line spacing

## The file manager (design §2.7).

category-files = Files
mode-directory = Files
cmd-dired-jump = File Manager
cmd-dired-projectRoot = Project Folder in the File Manager
cmd-dired-projects = Projects View
cmd-dired-cancel = Stop File Operations
cmd-dired-open = Open
cmd-dired-up = Parent Folder
cmd-dired-next = Next Line
cmd-dired-first = First Entry
cmd-dired-previous = Previous Line
cmd-dired-refresh = Read Again
cmd-dired-close = Close File Manager
cmd-dired-toggleDetails = Show or Hide Details
cmd-dired-toggleHidden = Show or Hide Dot Files
cmd-dired-sort = Sort By Next Order
cmd-dired-reverse = Reverse Order
cmd-dired-filter = Filter by Name
cmd-dired-mark = Mark
cmd-dired-unmark = Unmark
cmd-dired-flag = Flag for Deletion
cmd-dired-unmarkAll = Unmark All
cmd-dired-toggleMarks = Toggle Marks
cmd-dired-markRegexp = Mark by Regular Expression
cmd-dired-markDirectories = Mark Folders
cmd-dired-markExtension = Mark by Extension
cmd-dired-executeFlagged = Delete Flagged
cmd-dired-delete = Move to Trash
cmd-dired-deletePermanently = Delete for Good
cmd-dired-copy = Copy To
cmd-dired-move = Rename or Move To
cmd-dired-mkdir = New Folder
cmd-dired-newFile = New File
cmd-dired-symlink = Symbolic Link
cmd-dired-chmod = Change Permissions
cmd-dired-touch = Touch
cmd-dired-copyName = Copy Names
cmd-dired-copyPath = Copy Paths
menu-file-manager = File Manager
menu-projects-view = Projects
fm-projects = Projects
fm-no-projects = No projects yet: add a folder with Add Project Folder (Space p a with Vim keys).
fm-filter = names with “{ $text }”
fm-items = { $count ->
    [one] 1 item
   *[other] { $count } items
}
fm-nothing = Nothing to act on here
fm-no-target = No destination given
fm-not-listing = Not in the file manager
fm-in-projects = Open a project first
fm-confirm-trash = Move { $what } to the trash?
fm-confirm-delete = Delete { $what } for good? This cannot be undone.
fm-conflict = { $name } is already there.
fm-conflict-keys = o overwrite · s skip · k keep both · O S K for all · Esc cancel{" "}
fm-confirm-keys = (y/n){" "}
fm-answer-overwrite = Overwrite
fm-answer-skip = Skip
fm-answer-keep-both = Keep Both
fm-answer-overwrite-all = Overwrite All
fm-answer-skip-all = Skip All
fm-answer-keep-both-all = Keep Both for All
fm-answer-yes = Yes
fm-answer-no = No
fm-copying = Copying { $count ->
    [one] 1 item
   *[other] { $count } items
}… { $percent }%
fm-moving = Moving { $count ->
    [one] 1 item
   *[other] { $count } items
}… { $percent }%
fm-trashing = Moving { $count ->
    [one] 1 item
   *[other] { $count } items
} to the trash… { $percent }%
fm-deleting = Deleting { $count ->
    [one] 1 item
   *[other] { $count } items
}… { $percent }%
fm-copied = { $count ->
    [one] Copied 1 item
   *[other] Copied { $count } items
}
fm-moved = { $count ->
    [one] Moved 1 item
   *[other] Moved { $count } items
}
fm-trashed = { $count ->
    [one] Moved 1 item to the trash
   *[other] Moved { $count } items to the trash
}
fm-deleted = { $count ->
    [one] Deleted 1 item
   *[other] Deleted { $count } items
}
fm-skipped = { $count } skipped
fm-cancelled = stopped
fm-failed = { $name }: { $error }{ $count ->
    [one] {""}
   *[other] {" "}(and { $count } errors in all)
}
fm-marked = { $count ->
    [one] 1 marked
   *[other] { $count } marked
}
fm-hidden-shown = Dot files shown
fm-hidden-hidden = Dot files hidden
fm-sorted = Sorted by { $order }
fm-sort-name = name
fm-sort-time = time
fm-sort-size = size
fm-sort-extension = extension
fm-exists = { $name } exists already
fm-bad-mode = Permissions are three or four octal digits, such as 644

## Export.

category-export = Export
cmd-export-html = Export as HTML
cmd-export-markdown = Export as Markdown
cmd-export-htmlSubtree = Export Subtree as HTML
cmd-export-markdownSubtree = Export Subtree as Markdown
msg-exported = Exported to { $path }
msg-export-needs-file = Save the document to a file first; the export goes beside it
msg-no-other-document = No other document to go back to
status-files = files
