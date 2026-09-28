//! User settings that change the model. Each corresponds to an Emacs
//! variable; the defaults are those of `emacs -Q` with Org 9.7.

/// Which tags or properties are inherited: `org-use-tag-inheritance` and
/// `org-use-property-inheritance` accept `t`, `nil`, a regular expression
/// or a list of names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inheritance {
    /// Everything is inherited (`t`).
    All,
    /// Nothing is inherited (`nil`).
    None,
    /// Names matching a regular expression, in Rust `regex` syntax
    /// (translated from the Emacs expression by the caller).
    Regex(String),
    /// These names only.
    List(Vec<String>),
}

/// Settings of the model.
#[derive(Debug, Clone)]
pub struct Settings {
    /// `org-use-tag-inheritance` (default `t`).
    pub tag_inheritance: Inheritance,
    /// `org-tags-exclude-from-inheritance` (default empty).
    pub tags_exclude_from_inheritance: Vec<String>,
    /// `org-use-property-inheritance` (default `nil`).
    pub property_inheritance: Inheritance,
    /// `org-global-properties` (default empty).
    pub global_properties: Vec<(String, String)>,
    /// `org-global-properties-fixed`.
    pub global_properties_fixed: Vec<(String, String)>,
    /// `org-priority-highest`, `org-priority-lowest` and
    /// `org-priority-default`, as character codes (default A, C, B).
    pub priorities: (u32, u32, u32),
    /// `org-enforce-todo-dependencies` (default `nil`).
    pub enforce_todo_dependencies: bool,
    /// `org-category` (default `nil`).
    pub category: Option<String>,
    /// `org-hierarchical-todo-statistics` (default `t`).
    pub hierarchical_todo_statistics: bool,
    /// `org-checkbox-hierarchical-statistics` (default `t`).
    pub checkbox_hierarchical_statistics: bool,
    /// `org-group-tags` (default `t`): match strings expand group tags.
    pub group_tags: bool,
    /// `org-tag-alist` in `#+TAGS` syntax, used when the document has no
    /// `#+TAGS` (default empty).
    pub tag_alist: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            tag_inheritance: Inheritance::All,
            tags_exclude_from_inheritance: Vec::new(),
            property_inheritance: Inheritance::None,
            global_properties: Vec::new(),
            global_properties_fixed: vec![
                (
                    "VISIBILITY_ALL".into(),
                    "folded children content all".into(),
                ),
                (
                    "CLOCK_MODELINE_TOTAL_ALL".into(),
                    "current today repeat all auto".into(),
                ),
            ],
            priorities: ('A' as u32, 'C' as u32, 'B' as u32),
            enforce_todo_dependencies: false,
            category: None,
            hierarchical_todo_statistics: true,
            checkbox_hierarchical_statistics: true,
            group_tags: true,
            tag_alist: String::new(),
        }
    }
}
