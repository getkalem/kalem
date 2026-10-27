# kalem-vim

The Vim keys of the [Kalem](https://github.com/getkalem/kalem) editor:
the modal input layer of its Vim keymap profile, shared by the terminal
and the graphical editor. It works on `kalem_core::DocumentState` and
leaves insert mode to the frontend: `Vim::key` takes a key in the other
modes, with the `Host` the frontend gives it (its clipboard, its view),
and says whether the key was handled.

Covered: normal, insert, visual and replace modes, counts, motions,
operators, text objects, registers, macros, marks, undo, search, `.`,
`gq`, CTRL-W's window keys, the command line (`:s`, `:g`, `:normal`,
`:sort`, `:set` and the file and window commands), and the keys of the
plugins Doom Emacs has on (vim-surround, vim-commentary, vim-unimpaired).
`tests/vim/cases.json` at the repository's root holds key sequences whose
outcome is checked against Vim itself (`tools/vim-expected.py`).

Split from `kalem-core` along roadmap R3.7: the core never used the
layer, only the frontends did, so it depends on the core and nothing in
the core on it.

Licensed under MIT or Apache-2.0, at your option.
