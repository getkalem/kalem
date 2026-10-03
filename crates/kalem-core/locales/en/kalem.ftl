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
cmd-link-store = Store Link
cmd-org-link-insertStored = Insert Stored Link
cmd-org-link-open = Open Link
cmd-edit-enter = New Line or Item
cmd-edit-newline = Line Break
cmd-view-palette = Command Palette
cmd-view-menus = Menus
cmd-find-open = Find
cmd-find-replace = Find and Replace
cmd-view-outline = Outline
cmd-view-toggleSource = Toggle Source View
cmd-view-split = Split View
cmd-view-focus = Focus Mode
cmd-view-toggleWrap = Toggle Soft Wrap
cmd-view-toggleMath = Toggle Formula Preview
cmd-file-open = Open File
cmd-file-new = New Document
cmd-file-close = Close Document
cmd-file-next = Next Document
cmd-edit-repairDocument = Repair Document
msg-klm-ill-formed = Not formatted: { $count } problems in the file; Repair Document fixes them
msg-klm-repaired = Repaired { $problems } problems, { $lines } lines changed (Undo takes it back)
cmd-edit-trimTrailingBlankLines = Delete Trailing Blank Lines
cmd-edit-formatDocument = Format Document
cmd-settings-set = Set a Setting
cmd-help-theme = Choose the Theme
cmd-help-mode = Describe This Document
cmd-help-char = Describe the Character
cmd-help-bindings = All Key Bindings
cmd-help-describeKey = Describe Key
cmd-help-reload = Reload Settings and Keys
theme-system = The system's
theme-light = Light
theme-dark = Dark
help-no-file = no file
help-no-char = No character at the cursor
help-mode = { $file }: { $mode } mode, text type { $text }{ $kind ->
    [none] {""}
   *[other] , { $kind }
}
help-press-key = Press the keys to describe
help-key = { $keys } runs { $title } ({ $id })
help-key-none = { $keys } runs no command here
msg-reloaded-settings = Settings and keys read again
cmd-project-browseOther = Browse Another Project
cmd-project-findFileOther = Find File in Another Project
cmd-project-shellCommand = Shell Command at the Project
cmd-project-todos = Project TODOs
cmd-file-other = Other File
msg-no-other-file = No other file with this name here
cmd-export-htmlBrowser = Export as HTML and Open
cmd-app-terminal = Open a Terminal Here
cmd-app-newWindow = New Window
msg-no-terminal = No terminal could be started: { $error }
msg-one-window = The terminal editor has one window
cmd-view-toggleLineNumbers = Toggle Line Numbers
cmd-view-toggleSourceMarkers = Toggle Markup Characters
cmd-view-bigText = Toggle Big Text
cmd-view-toggleReadOnly = Toggle Read-Only
cmd-view-fullScreen = Toggle Full Screen
cmd-view-zen = Zen Mode
msg-read-only-on = Read-only: edits are refused
msg-read-only-off = Editable again
msg-full-screen-terminal = The terminal decides about full screen
cmd-search-lines = Search Lines
search-lines = Lines
search-lines-count = { $count ->
    [one] 1 line
   *[other] { $count } lines
}
cmd-search-folder = Search in Folder
cmd-search-online = Search Online
cmd-project-searchOther = Search in Another Project
msg-not-a-folder = Not a folder: { $path }
cmd-file-delete = Delete This File
cmd-file-rename = Rename or Move This File
cmd-file-copy = Copy This File To
cmd-file-copyPath = Copy This File's Path
cmd-file-copyRelativePath = Copy This File's Path from the Project
cmd-file-openSettingsFolder = Open the Settings Folder
cmd-file-openKeymap = Open Your Keymap
cmd-file-openWorkspaceSettings = Open the Workspace Settings
msg-no-file = This document has no file
msg-no-config-dir = No settings folder (set KALEM_CONFIG_DIR)
cmd-file-saveAll = Save All
cmd-file-closeOthers = Close Other Documents
cmd-file-closeAll = Close All Documents
cmd-file-last = Last Document
cmd-file-bury = Move Document to the End
cmd-file-scratch = Scratch Document
cmd-file-copyText = Copy the Whole Document
msg-no-state-dir = No folder for Kalem's state (set KALEM_STATE_DIR)
msg-no-last-document = No document was shown before this one
cmd-file-previous = Previous Document
cmd-file-switch = Switch Document
cmd-file-recent = Open Recent File
cmd-view-openFiles = Open Files
cmd-view-toggleFolderTree = Toggle Folder Tree
cmd-view-revealInTree = Reveal in Folder Tree
folder-tree = Folders
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
cmd-org-property-delete = Delete Property
cmd-org-todo-toggleOrdered = Toggle Ordered Subtasks
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
cmd-org-cite-insert = Insert Citation
cmd-org-insert-drawer = Insert Drawer
cmd-edit-complete = Complete
cmd-lines-duplicate = Duplicate Lines
cmd-lines-moveUp = Move Lines Up
cmd-lines-moveDown = Move Lines Down
cmd-lines-join = Join Lines
cmd-lines-sort = Sort Lines
cmd-edit-trimTrailingWhitespace = Trim Trailing Whitespace
cmd-edit-selectWord = Select Word
cmd-edit-expandSelection = Expand Selection
cmd-edit-shrinkSelection = Shrink Selection
cmd-edit-toggleComment = Toggle Comment
cmd-edit-gotoBracket = Go to Matching Bracket
cmd-cursor-addBelow = Add Cursor Below
cmd-cursor-addAbove = Add Cursor Above
cmd-selection-addNextOccurrence = Add Next Occurrence
cmd-selection-allOccurrences = Select All Occurrences
cmd-selection-columnDown = Column Selection Down
cmd-selection-columnUp = Column Selection Up
cmd-cursor-clearExtra = Single Cursor
cmd-file-reopenWithEncoding = Reopen with Encoding
cmd-file-saveWithEncoding = Save with Encoding
cmd-edit-gotoLine = Go to Line
cmd-stats-chapters = Word Count by Chapter
cmd-stats-setDocumentTarget = Set Document Word Target
cmd-stats-setSectionTarget = Set Section Word Target
cmd-edit-copyRichText = Copy as Rich Text
cmd-edit-copyHtml = Copy as HTML
cmd-org-archive-toggleTag = Toggle Archive Tag
cmd-org-archive-sibling = Archive to Sibling
cmd-org-refile = Refile
cmd-org-caption-set = Set Caption
cmd-org-name-set = Set Name
cmd-org-insert-reference = Insert Cross Reference
cmd-org-property-edit = Edit Properties
cmd-org-schedule = Schedule
cmd-org-deadline = Set Deadline
cmd-org-schedule-remove = Remove Schedule
cmd-org-deadline-remove = Remove Deadline
cmd-org-note-add = Add Note
cmd-org-footnote-new = New Footnote
cmd-org-footnote-action = Go to Footnote Definition or Reference
cmd-org-footnote-renumber = Renumber Footnotes
cmd-org-footnote-sort = Sort Footnote Definitions
cmd-org-footnote-normalize = Normalize Footnotes
cmd-org-footnote-delete = Delete Footnote
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
category-bibtex = BibTeX
category-csv = CSV
category-tags = Tags
category-view = View
category-code = Code
category-plugins = Plugins

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
menu-citation = Citation…
menu-bibtex = BibTeX
menu-sort-author = Sort by Author
menu-sort-year = Sort by Year
menu-sort-title = Sort by Title
menu-sort-key = Sort by Key
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
status-table-count = Count: { $count }
status-problems = { $count ->
    [one] One problem
   *[other] { $count } problems
}
status-csv-filter = Filtered: { $matched } of { $total } rows (“{ $filter }”)
status-bib = { $count } entries
status-bib-sorted = { $count } entries, sorted by { $column } ({ $order })
status-table-numbers = Sum: { $sum }   Average: { $average }   Min: { $min }   Max: { $max }
status-formula-error = #ERROR: { $why }
# $count is the number for plural rules; $shown is the same number with digit groups.
status-words = { $count ->
    [one] { $shown } word
   *[other] { $shown } words
}
status-words-section = { $words }, { $section } in section
status-words-target = { $words } words
status-words-progress = { $shown } of { $target } ({ $percent }%)
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
msg-no-property = No { $key } property here
msg-no-bibliography = No bibliography: add #+BIBLIOGRAPHY: with a .bib or .json file
msg-no-completions = No completions here
msg-no-comment-style = This language has no comment marker Kalem knows
msg-no-bracket = No bracket at the cursor, or it has no match
msg-no-more-occurrences = No more occurrences
msg-occurrences-selected = { $count ->
    [one] { $count } occurrence selected
   *[other] { $count } occurrences selected
}
msg-unknown-encoding = Unknown encoding: { $name }
msg-reopen-modified = Save or undo the changes first: reopening reads the file again
msg-reopened = Reopened as { $encoding }
msg-unencodable = “{ $ch }” cannot be written in { $encoding }: Save with Encoding UTF-8 keeps it
msg-opened-as = Not UTF-8: opened as { $encoding } (Reopen with Encoding chooses another)
msg-opened-lossy = Some bytes are not { $encoding }: they show as � and saving writes � in their place (Reopen with Encoding chooses another)
msg-bad-line = Not a line number
msg-no-headings = The document has no headings
msg-bad-word-target = Not a word count: write 80000, 80,000 or 80k
msg-no-word-target = No word target here
msg-rich-copy-plain = Copied as plain text: this clipboard takes no HTML here (Copy as HTML copies the markup)
msg-archived = Subtree archived
msg-unarchived = Subtree unarchived
msg-not-in-subtree = Not in a subtree: put the cursor on a heading or under one
msg-no-references = Nothing to refer to: name a table or a block, or add a heading
msg-picture-needs-file = Save the document first: pictures go into a folder beside it
msg-md-missing-file = No such file: { $path }
msg-no-unused-images = No unused images
msg-unused-images-trashed = { $count ->
    [one] Moved an unused image to the Trash: { $names }
   *[other] Moved { $count } unused images to the Trash: { $names }
}
msg-ordered-on = Subtasks must be completed in sequence
msg-ordered-off = Subtasks can be completed in any order
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
toc-title = Contents
toc-empty = Contents: no headings
footnotes-title = Notes
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
vim-visual-block = -- VISUAL BLOCK --
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
msg-cannot-open-file = Cannot open { $path }: { $error }
msg-saved-count = { $count } documents saved
prompt-close = Save { $name } before closing? (y)es, (n)o, Esc to cancel{" "}
prompt-quit-many = { $count } documents have unsaved changes. Save them before quitting? (y)es, (n)o, Esc to cancel{" "}
prompt-open = Open file
prompt-project-add = Add project folder
prompt-project-rename = Project name
status-commands = commands

## The file manager (design §2.7).

category-files = Files
mode-directory = Files
cmd-dired-jump = File Manager
cmd-dired-projectRoot = Project Folder in the File Manager
cmd-dired-projects = Projects View
cmd-dired-cancel = Stop File Operations
cmd-dired-open = Open
cmd-dired-up = Parent Folder
cmd-dired-insertSubdir = List Folder Here
cmd-dired-removeSubdir = Remove Listed Folder
fm-not-a-folder = Not a folder
fm-no-subdir = Not in a folder listed here
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
cmd-dired-markChangedSince = Mark Changed Since
fm-bad-since = Not an age or a date: { $text } (try 2d, 3h, 1w, today, 2026-09-01)
cmd-dired-executeFlagged = Delete Flagged
cmd-dired-delete = Move to Trash
cmd-dired-deletePermanently = Delete for Good
cmd-dired-copy = Copy To
cmd-dired-move = Rename or Move To
cmd-dired-editNames = Edit Names
cmd-dired-commitNames = Apply Edited Names
cmd-dired-abortNames = Discard Edited Names
cmd-dired-findName = Find by Name
cmd-dired-searchFiles = Search in Files
cmd-dired-openExternal = Open with Application
cmd-dired-reveal = Show in System File Manager
cmd-dired-shellCommand = Shell Command on Files
cmd-dired-togglePreview = Preview Pane
cmd-dired-thumbnails = Picture Thumbnails
cmd-dired-undo = Undo File Operation
cmd-dired-mkdir = New Folder
cmd-dired-newFile = New File
cmd-dired-symlink = Symbolic Link
cmd-dired-chmod = Change Permissions
cmd-dired-touch = Touch
cmd-dired-markExecutables = Mark Executables
cmd-dired-upcase = Rename to Upper Case
cmd-dired-downcase = Rename to Lower Case
cmd-dired-renameRegexp = Rename by Regular Expression
cmd-dired-compress = Compress or Extract
fm-compressed = Made { $name }
fm-extracted = Extracted
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
fm-undone-moved = { $count ->
    [one] Moved 1 item back
   *[other] Moved { $count } items back
}
fm-undone-trashed = { $count ->
    [one] Brought 1 item back from the trash
   *[other] Brought { $count } items back from the trash
}
fm-wdired-help = Edit the names, then Ctrl+S to rename them all, Escape to discard
fm-wdired-lines = Only names can change: a line was added, removed or changed outside its name
fm-wdired-empty = The new name of { $name } is empty
fm-wdired-duplicate = Two files would be named { $name }
fm-wdired-no-folder = No folder for { $name }
fm-renamed = { $count ->
    [one] Renamed 1 item
   *[other] Renamed { $count } items
}
fm-found = { $count ->
    [one] 1 found by name { $pattern }
   *[other] { $count } found by name { $pattern }
}
msg-link-stored = Stored a link to { $names }
msg-link-nothing-to-store = Nothing here to link to: the document has no file
msg-no-stored-link = No link stored: use Store Link first
fm-confirm-shell = Run “{ $command }” on { $what }?
fm-running-shell = Running { $command }…
fm-shell-done = { $command }: done
fm-shell-failed = The command stopped with { $code }: { $error }
fm-preview-none = Nothing to preview
fm-preview-binary = Not text: no preview
fm-preview-graphical = The preview pane is in the graphical editor; the terminal opens the file with Enter
fm-nothing-to-undo = No file operation to undo
fm-undo-blocked = Cannot undo: { $name } is in the way or gone
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
cmd-export-gfm = Export as GitHub Markdown
cmd-export-latex = Export as LaTeX
cmd-export-latexSubtree = Export Subtree as LaTeX
cmd-export-pdf = Export as PDF (LaTeX)
cmd-export-pdfSubtree = Export Subtree as PDF (LaTeX)
cmd-export-docx = Export as Word (pandoc)
cmd-export-odt = Export as OpenDocument (pandoc)
cmd-export-epub = Export as EPUB (pandoc)
cmd-export-rtf = Export as RTF (pandoc)
cmd-file-import = Import as Org (pandoc)…
cmd-export-text = Export as Plain Text
cmd-export-dialog = Export…
cmd-export-toggleBodyOnly = Toggle Export of the Body Only
cmd-export-toggleOpenAfter = Toggle Opening Exported Files
cmd-export-toggleMath = Toggle Formulas as MathJax or SVG
export-setting = Setting
export-on = on
export-off = off
export-body-only = Body only
export-open-after = Open after exporting
export-math = Formulas
export-math-svg = SVG images
export-math-mathjax = MathJax
cmd-export-htmlSubtree = Export Subtree as HTML
cmd-export-markdownSubtree = Export Subtree as Markdown
msg-export-needs-file = Save the document to a file first; the export goes beside it
msg-no-other-document = No other document to go back to
status-files = files
kind-org = Org
kind-klm = Kalem
kind-dropped-spans = { $count ->
    [one] { $count } formatted span
   *[other] { $count } formatted spans
}
kind-dropped-paragraphs = { $count ->
    [one] { $count } paragraph attribute
   *[other] { $count } paragraph attributes
}
kind-dropped-document = { $count ->
    [one] { $count } document option line
   *[other] { $count } document option lines
}
kind-dropped-nothing = no Kalem formatting
kind-markup-in-org = Formatting an earlier Kalem wrote; Org ignores it, and Kalem shows it as Org does
msg-compiling-pdf = Compiling the PDF…
msg-no-latex = No LaTeX found: install TeX Live, MacTeX, MiKTeX or tectonic
msg-pdf-failed = The PDF could not be made: { $error }
msg-build-no-log = { $program } stopped before LaTeX wrote a log: run it in a terminal to see why
msg-build-no-pdf = LaTeX wrote no PDF; the log has no error (an empty document?)
msg-pdf-error = { $place }: { $error }{ $count ->
    [0] {""}
    [one] {" "}(and 1 more error)
   *[other] {" "}(and { $count } more errors)
}
msg-pdf-done = Exported { $path }{ $count ->
    [0] {""}
    [one] {" "}(1 LaTeX warning)
   *[other] {" "}({ $count } LaTeX warnings)
}
msg-job-failed = A background task stopped unexpectedly
msg-no-pandoc = pandoc not found: install it from https://pandoc.org to read and write Word, OpenDocument and EPUB
msg-converting = Converting with pandoc…
msg-pandoc-failed = pandoc could not convert it: { $error }
msg-imported = Imported as { $path }
msg-import-exists = { $path } exists already
cite-bibliography-unreadable = The bibliography { $file } cannot be read: { $error }
cite-entry-skipped = An entry of { $file } is left out: { $error }
cite-unknown-key = No bibliography has the key @{ $key }
cite-unused-entry = Nothing cites the bibliography entry @{ $key }
category-footnotes = Footnotes
menu-footnote = Footnote
footnote-preview = Footnote { $label }: { $text }
menu-schedule = Schedule…
menu-deadline = Deadline…
properties-change = Change
properties-new = New Property…
properties-add = Add
properties-delete = Delete { $key }
properties-remove = Remove
menu-properties = Properties…
menu-drawer = Drawer…
menu-refile = Refile…
menu-archive-sibling = Archive to Sibling
menu-archive-tag = Toggle Archive Tag
menu-caption = Caption…
menu-name = Name…
menu-reference = Cross Reference…
msg-not-csv = Not a CSV document
msg-not-bib = Not a BibTeX file
msg-bib-no-entry = The cursor is not in an entry
msg-bib-field-name = A field name is letters, digits, and - _ : .
msg-bib-key = A key cannot be empty or hold commas, braces or blanks
msg-bib-key-taken = The key { $key } is already used
msg-csv-no-row = No row to move past
msg-csv-no-column = No column to move past
msg-csv-save-first = Save the file first
msg-csv-converted = Written as { $path }
cmd-bib-sortView = Sort Entries by Column
cmd-bib-unsortView = File Order
cmd-bib-setField = Set Field
cmd-bib-newEntry = New Entry
cmd-csv-sortView = Sort View by Column
cmd-csv-unsortView = File Order
cmd-csv-setDelimiter = Set Delimiter
cmd-csv-setQuote = Set Quote Character
cmd-csv-toggleHeader = First Row Is a Header
cmd-csv-detectDialect = Detect Delimiter and Header Again
msg-csv-bad-delimiter = Not a single character: { $value }
cmd-csv-filter = Filter Rows
cmd-csv-clearFilter = Show All Rows
cmd-csv-nextField = Next Field
cmd-csv-previousField = Previous Field
cmd-csv-insertRow = Insert Row
cmd-csv-deleteRow = Delete Row
cmd-csv-moveRowUp = Move Row Up
cmd-csv-moveRowDown = Move Row Down
cmd-csv-insertColumn = Insert Column
cmd-csv-deleteColumn = Delete Column
cmd-csv-moveColumnLeft = Move Column Left
cmd-csv-moveColumnRight = Move Column Right
cmd-csv-sortFile = Sort File by Column
cmd-csv-copyAsTsv = Copy as Tab-Separated Values
cmd-csv-openAsText = Open as Plain Text
cmd-csv-convertToOrg = Convert to Org Table
cmd-markdown-toggleCheckbox = Toggle Checkbox
which-key-later = later
which-key-plugin = plugin
cmd-project-searchWord = Search Project for Word
cmd-bookmark-set = Set Bookmark
cmd-bookmark-jump = Jump to Bookmark
cmd-bookmark-delete = Delete Bookmark
cmd-bookmark-goto = Go to Bookmark
category-bookmarks = Bookmarks
msg-bookmark-set = Bookmark “{ $name }” set
msg-bookmark-deleted = Bookmark “{ $name }” deleted
msg-no-bookmarks = No bookmarks yet (Set Bookmark, SPC b m)
msg-no-word = No word at the cursor
cmd-markdown-openLink = Open Link
cmd-markdown-convertToOrg = Convert to Org
cmd-markdown-newline = New Item
cmd-markdown-table-nextField = Next Field
cmd-markdown-table-previousField = Previous Field
cmd-markdown-table-align = Align Table
category-markdown = Markdown
msg-csv-duplicates = { $count ->
    [0] No duplicate rows
    [one] One duplicate row removed
   *[other] { $count } duplicate rows removed
}
msg-csv-nothing-to-split = No value in this column holds “{ $separator }”
msg-csv-bad-columns = Not columns: { $value } (letters or numbers, a - before one to sort it descending: B, -A)
msg-csv-no-numbers = No numbers in this column
msg-csv-sum = Sum: { $sum } (copied)
msg-csv-cell = Cell { $cell } ({ $org })
msg-csv-cell-named = Cell { $cell } ({ $org }), column “{ $column }”
msg-csv-bad-cell = Not a cell: { $value } (B3, @3$2 or 3,2)
msg-csv-no-series = This value does not continue as a series
cmd-csv-fillDown = Fill Down
cmd-csv-fillSeries = Fill Series
cmd-csv-removeDuplicates = Remove Duplicate Rows
cmd-csv-transpose = Transpose
cmd-csv-splitColumn = Split Column
cmd-csv-joinColumns = Join with Next Column
cmd-csv-sortFileBy = Sort File by Columns
cmd-csv-sumColumn = Sum Column
cmd-csv-cellCoordinates = Cell Coordinates
cmd-csv-goToCell = Go to Cell
cmd-csv-toggleAlignment = Align Numbers Right
cmd-csv-toggleRainbow = Rainbow Columns
cmd-csv-toggleCoordinates = Coordinate Grid
cmd-file-print = Print
msg-printing = Printing { $path }
msg-print-viewer = Opened { $path }; print it from the viewer
msg-print-no-display = Written { $path }; with no display, print it with lp
msg-print-failed = Could not print: { $error }
latex-unknown-label = No label { $key }
latex-duplicate-label = The label { $key } is defined more than once
latex-label-clash = A second label on this line: amsmath stops with “Multiple \label's” ({ $key })
latex-label-unwritten = { $key } is on a line without a number and no numbered line follows: LaTeX never writes it, and \ref prints ??
latex-label-before-caption = { $key } comes before the caption: it refers to the section, or with the caption package to nothing (\ref prints ??); put it after \caption
latex-missing-file = No file { $file }
latex-missing-picture = No picture { $file }
latex-item-outside-list = \item outside a list: LaTeX stops with "Lonely \item"
latex-too-deep = Lists nested more than four deep: LaTeX stops with "Too deeply nested"
latex-deprecated-font = \{ $command } is deprecated in LaTeX 2ε; use the \text… command or the declaration (\bfseries, \itshape)
latex-double-dollar = $$…$$ is plain TeX; use \[…\]
latex-tie = Use ~ before a reference or a citation, so that no line breaks before it
latex-ellipsis = Use \ldots for an ellipsis
latex-quotes = Use `` and '' for quotation marks
latex-install-texlive = Install it with: tlmgr install { $package }
latex-install-miktex = Install it with the MiKTeX Console, or: mpm --install={ $package }
cmd-latex-build = Build PDF
category-latex = LaTeX
msg-latex-built = Built { $path }{ $count ->
    [0] {""}
    [one] {" "}(1 LaTeX warning)
   *[other] {" "}({ $count } LaTeX warnings)
}
msg-not-latex = Not a LaTeX document
msg-latex-not-here = Not here: put the cursor on a sectioning command or a list item
msg-no-fix = No fix here: put the cursor on a flagged construct with an obvious fix
msg-build-cancelling = Stopping the build…
msg-no-build = No build is running
msg-build-cancelled = Build cancelled
msg-no-problems = No problems in this document
latex-build-problem = Build: { $message }
latex-diagnostic-fixable = { $mark } { $message } (Quick Fix fixes it)
cmd-latex-enter = New Line or Item
cmd-latex-link-open = Open Link
cmd-latex-list-indent = Nest Item
cmd-latex-list-outdent = Unnest Item
cmd-latex-nextProblem = Next Problem
cmd-latex-previousProblem = Previous Problem
cmd-latex-cancelBuild = Cancel Build
cmd-latex-problems = Show Problems
cmd-latex-fix = Quick Fix
cmd-latex-format-bold = Bold
cmd-latex-format-italic = Emphasis
cmd-latex-format-code = Typewriter
cmd-latex-format-underline = Underline
cmd-latex-section-setLevel = Heading Level
cmd-latex-section-promote = Promote Section
cmd-latex-section-demote = Demote Section
cmd-latex-section-moveUp = Move Section Up
cmd-latex-section-moveDown = Move Section Down
cmd-latex-math-toggleDisplay = Display Math
cmd-latex-math-toggleNumbering = Number Equation
cmd-latex-insert-figure = Insert Figure
cmd-latex-insert-table = Insert Table
cmd-latex-insert-equation = Insert Equation
cmd-latex-insert-citation = Insert Citation
msg-latex-no-bibliography = The document names no bibliography file with entries
cmd-latex-export-html = Export as HTML (pandoc)
cmd-latex-export-markdown = Export as Markdown (pandoc)
cmd-latex-export-docx = Export as Word (pandoc)
cmd-latex-convertToOrg = Convert to Org (pandoc)
msg-latex-converted-one-way = Converted one way: the LaTeX file is unchanged, and edits to the Org file do not go back to it
cmd-file-newFromTemplate = New from Template…
msg-unknown-template = No template { $name }
csv-unterminated-quote = This quote is never closed: the rest of the file is one value
csv-text-after-quote = Text after the closing quote (kept as part of the value)
csv-bare-quote = A quote inside an unquoted value (the value should be quoted and the quote doubled)
cmd-dired-copyFiles = Copy Files
cmd-dired-cutFiles = Cut Files
cmd-dired-paste = Paste Files
cmd-dired-duplicate = Duplicate
cmd-dired-markAll = Select All
cmd-dired-copyRelativePath = Copy Relative Paths
cmd-dired-properties = Properties
fm-clipboard-empty = No files copied or cut yet
fm-clip-copied = { $count ->
    [one] 1 file copied; paste it in a folder with Ctrl+V
   *[other] { $count } files copied; paste them in a folder with Ctrl+V
}
fm-clip-cut = { $count ->
    [one] 1 file cut; paste it in a folder with Ctrl+V to move it
   *[other] { $count } files cut; paste them in a folder with Ctrl+V to move them
}
fm-copy-suffix = copy
fm-prop-path = Path
fm-prop-kind = Kind
fm-prop-link = Symbolic link
fm-prop-folder = Folder
fm-prop-file = File
fm-prop-size = Size (bytes)
fm-prop-modified = Modified
fm-prop-created = Created
fm-prop-mode = Permissions
fm-prop-target = Link target
fm-menu-open = Open
fm-menu-open-system = Open with System Application
fm-menu-cut = Cut
fm-menu-copy = Copy
fm-menu-paste = Paste
fm-menu-rename = Rename
fm-menu-move-to = Move to…
fm-menu-copy-to = Copy to…
fm-menu-delete = Move to Trash
fm-menu-delete-permanently = Delete Permanently…
fm-menu-new-file = New File…
fm-menu-new-folder = New Folder…
fm-menu-invert = Invert Selection
fm-menu-sort = Sort By…
cmd-dired-contextMenu = File Menu
fm-menu = File
msg-no-picker = No list to resume yet
msg-no-panel = No panel shown or hidden yet
cmd-picker-resume = Resume Last Picker
cmd-view-toggleLastPanel = Toggle Last Panel
msg-universal-argument = Count { $n }: the next command runs { $n } times
cmd-app-universalArgument = Universal Argument
cmd-session-save = Save Session
cmd-session-saveAs = Save Session As
cmd-session-restore = Restore Last Session
cmd-session-restoreNamed = Restore Session
cmd-session-saveAndQuit = Save Session and Quit
category-session = Session
msg-no-sessions = No sessions saved yet
msg-no-session = No session called { $name }
msg-session-saved = Session saved: { $path }
msg-session-restored = Session restored: { $count } documents
cmd-app-quitWithoutSaving = Quit Without Saving
cmd-window-close = Close Window
cmd-app-restart = Restart
cmd-app-restartAndRestore = Restart and Restore
prompt-quit-discard = Quit and lose the changes of { $count } documents? (y)es, (n)o{" "}
dialog-quit-discard = Quit and lose the unsaved changes of { $count } documents?
dialog-quit = Quit
msg-restart-unsaved = Save or close the changed documents before restarting
cmd-markdown-table-moveRowUp = Move Row Up
cmd-markdown-table-moveRowDown = Move Row Down
cmd-markdown-table-moveColumnLeft = Move Column Left
cmd-markdown-table-moveColumnRight = Move Column Right
cmd-markdown-table-insertRow = Insert Row
cmd-markdown-table-deleteRow = Delete Row
cmd-markdown-table-insertColumn = Insert Column
cmd-markdown-table-deleteColumn = Delete Column
cmd-markdown-table-sort = Sort Rows by Column
cmd-markdown-emphasis-bold = Bold
cmd-markdown-emphasis-italic = Italic
cmd-markdown-emphasis-code = Code
cmd-markdown-emphasis-strikeThrough = Strike-through
cmd-markdown-insert-link = Insert Link
cmd-insert-text = Insert Text
cmd-insert-unicode = Insert Unicode Character
cmd-insert-emoji = Insert Emoji
cmd-insert-fileName = Insert File Name
cmd-insert-filePath = Insert File Path
cmd-insert-fromHistory = Insert from Clipboard History
cmd-insert-fromRegister = Insert from Register
category-unicode = Unicode
category-emoji = Emoji
category-clipboard = Clipboard history
category-registers = Registers
msg-no-history = Nothing copied yet
msg-no-registers = The registers are empty
cmd-notes-search = Search Notes
msg-no-notes-folder = No notes folder at { $path } (the setting notes.directory)
cmd-pane-splitRight = Split Right
cmd-pane-splitBelow = Split Below
cmd-pane-focus = Focus Pane
cmd-pane-move = Move Pane
cmd-pane-close = Close Pane
cmd-pane-closeWithDocument = Close Pane and Document
cmd-pane-only = Only This Pane
cmd-pane-next = Next Pane
cmd-pane-previous = Previous Pane
cmd-pane-balance = Balance Panes
cmd-pane-resize = Resize Pane
cmd-pane-swap = Swap Panes
cmd-pane-rotate = Rotate Panes
cmd-pane-undo = Undo Layout
cmd-pane-redo = Redo Layout
cmd-pane-new = New Pane
msg-last-pane = This is the only pane
cmd-workspace-list = Switch Workspace
cmd-workspace-new = New Workspace
cmd-workspace-newNamed = New Named Workspace
cmd-workspace-delete = Delete Workspace
cmd-workspace-rename = Rename Workspace
cmd-workspace-next = Next Workspace
cmd-workspace-previous = Previous Workspace
cmd-workspace-switch = Go to Workspace
cmd-workspace-last = Last Workspace
cmd-workspace-save = Save Workspace
cmd-workspace-load = Load Workspace
cmd-workspace-deleteSaved = Delete Saved Workspace
category-workspaces = Workspaces
msg-workspace = Workspace { $name }
msg-last-workspace = This is the only workspace
msg-no-saved-workspaces = No workspaces saved yet
msg-workspace-deleted = Saved workspace { $name } deleted
cmd-markdown-table-recalculate = Recalculate Table
msg-no-formulas = The table has no formulas (a line <!-- TBLFM: … --> after it)
cmd-markdown-list-moveUp = Move Item Up
cmd-markdown-list-moveDown = Move Item Down
cmd-markdown-list-renumber = Renumber List
cmd-markdown-frontMatter-edit = Edit Properties
cmd-markdown-frontMatter-set = Set Property
cmd-markdown-frontMatter-delete = Delete Property
front-matter-add = Add a property…
front-matter-delete = Delete { $key }
cmd-csv-recordView = Record View
cmd-csv-editField = Edit Field
cmd-csv-setField = Set Field
cmd-csv-replaceInColumn = Replace in Column
cmd-csv-frequencies = Frequency Table
cmd-csv-killField = Kill Field
cmd-csv-yankField = Yank Field
category-record = Record { $row }
category-frequencies = Frequencies
csv-column = Column { $n }
msg-replaced-count = { $count } fields changed
cmd-markdown-insert-image = Insert Image
cmd-file-removeUnusedImages = Remove Unused Images
cmd-pane-closeOrQuit = Close Pane or Quit
cmd-project-addFolder = Add Project Folder…
cmd-code-documentation = Show Documentation
cmd-code-definition = Go to Definition
cmd-code-declaration = Go to Declaration
cmd-code-typeDefinition = Go to Type Definition
cmd-code-implementation = Go to Implementations
cmd-code-references = Find References
cmd-code-symbols = Go to Symbol in Document
cmd-code-problems = List Problems
cmd-code-allProblems = List Problems of Open Files
cmd-code-restartServer = Restart Language Server
cmd-code-serverStatus = Language Server Status
cmd-code-goto = Go to Place
cmd-plugin-browse = Browse Plugins
cmd-plugin-install = Install Plugin…
cmd-plugin-confirmInstall = Confirm Plugin Installation
cmd-plugin-cancelInstall = Cancel Plugin Installation
cmd-plugin-list = Installed Plugins
cmd-plugin-manage = Manage Plugin
cmd-plugin-remove = Remove Plugin
cmd-plugin-removeConfirmed = Remove Plugin Now
fm-menu-remove-project = Remove from Projects (the folder stays)

# The viewer of files that are not text (design §11.13).
category-viewer = Viewer
mode-viewer = Viewer
msg-viewer-unsaved = Save or undo the changes to this file first
msg-viewer-no-document = No text document to insert a link into
msg-viewer-copied = Copied the picture
msg-viewer-cannot-open = The file cannot be shown: { $error }
cmd-viewer-zoomIn = Zoom In
cmd-viewer-zoomOut = Zoom Out
cmd-viewer-fit = Fit to Window
cmd-viewer-fitWidth = Fit to Width
cmd-viewer-actualSize = Actual Size
cmd-viewer-panLeft = Pan Left
cmd-viewer-panRight = Pan Right
cmd-viewer-panUp = Pan Up
cmd-viewer-panDown = Pan Down
cmd-viewer-rotateRight = Rotate View Right
cmd-viewer-rotateLeft = Rotate View Left
cmd-viewer-next = Next
cmd-viewer-previous = Previous
cmd-viewer-nextFile = Next File in Folder
cmd-viewer-previousFile = Previous File in Folder
cmd-viewer-first = First
cmd-viewer-last = Last
cmd-viewer-togglePlay = Play or Pause
cmd-viewer-info = Show Information
cmd-viewer-copy = Copy Picture
cmd-viewer-insertLink = Insert Link at Point
cmd-viewer-edit = Edit the File
cmd-viewer-grid-up = Cell Up
cmd-viewer-grid-down = Cell Down
cmd-viewer-grid-left = Cell Left
cmd-viewer-grid-right = Cell Right
cmd-viewer-grid-pageDown = Page Down
cmd-viewer-grid-pageUp = Page Up
cmd-viewer-grid-rowStart = First Column
cmd-viewer-grid-start = First Cell
cmd-viewer-grid-end = Last Cell
cmd-viewer-grid-nextSheet = Next Sheet
cmd-viewer-grid-previousSheet = Previous Sheet
cmd-viewer-grid-edit = Edit Cell
cmd-viewer-grid-editFormula = Enter a Formula
cmd-viewer-grid-setCell = Set Cell
cmd-viewer-grid-clear = Clear Cell
cmd-viewer-grid-copy = Copy Cell
cmd-viewer-grid-insertRow = Insert Row Above
cmd-viewer-grid-deleteRow = Delete Row
cmd-viewer-grid-insertColumn = Insert Column Left
cmd-viewer-grid-deleteColumn = Delete Column
cmd-viewer-grid-runMacro = Run Macro
