//! Projects on temporary trees: ignore rules, binary files, nested
//! projects, the watcher, search, and a large tree.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use kalem_project::{FileIndex, Projects, Query, Search};

fn tree(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kalem-project-t-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    kalem_project::list::normal(&d)
}

fn names(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn ignore_rules() {
    let root = tree(
        "ignore",
        &[
            ("a.org", b"* A\n"),
            ("notes/b.org", b"* B\n"),
            (".gitignore", b"build/\n*.tmp\n"),
            ("build/out.org", b"x"),
            ("c.tmp", b"x"),
            (".ignore", b"secret.org\n"),
            ("secret.org", b"x"),
            (".git/config", b"x"),
            ("image.png", b"\x89PNG"),
            ("blob", b"a\0b"),
            (".hidden.org", b"* H\n"),
            ("log/x.log", b"x"),
        ],
    );
    let idx = FileIndex::new(&root, &["*.log".into()]);
    let files = idx.wait();
    assert_eq!(
        names(&files),
        [
            ".gitignore",
            ".hidden.org",
            ".ignore",
            "a.org",
            "notes/b.org"
        ]
    );
    assert_eq!(idx.root(), root.as_path());
}

#[test]
fn watcher_and_refresh() {
    let root = tree("watch", &[("a.org", b"x")]);
    let idx = FileIndex::new(&root, &[]);
    assert_eq!(names(&idx.wait()), ["a.org"]);
    std::fs::write(root.join("b.org"), "y").unwrap();
    // The watcher sees it; without events (some file systems), a refresh.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let files = idx.wait();
        if files.len() == 2 {
            break;
        }
        if std::time::Instant::now() > deadline {
            idx.refresh();
            assert_eq!(names(&idx.wait()), ["a.org", "b.org"]);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn nested_projects() {
    let root = tree(
        "nested",
        &[
            ("a.org", b"x"),
            ("sub/b.org", b"y"),
            ("sub/deeper/c.org", b"z"),
        ],
    );
    let mut p = Projects::default();
    p.add(&root).unwrap();
    p.add(&root.join("sub")).unwrap();
    assert_eq!(p.containing(&root.join("a.org")).unwrap().root, root);
    assert_eq!(
        p.containing(&root.join("sub/deeper/c.org")).unwrap().root,
        root.join("sub")
    );
    // A missing folder stays listed, shown as missing.
    let gone = tree("gone", &[]);
    p.add(&gone).unwrap();
    std::fs::remove_dir_all(&gone).unwrap();
    assert!(!p.get(&gone).unwrap().exists());
}

#[test]
fn searching() {
    let root = tree(
        "search",
        &[
            ("a.org", b"* Apple pie\nno match\napple\n"),
            ("b/c.txt", b"pineapple and APPLE\n"),
            (".gitignore", b"skip.org\n"),
            ("skip.org", b"apple\n"),
            ("bin.dat", b"apple\0"),
        ],
    );
    let run = |q: Query| {
        let mut hits = Vec::new();
        kalem_project::search::search(&root, &[], &[], &q, &AtomicBool::new(false), |h| {
            hits.push(h);
            true
        })
        .unwrap();
        hits.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        hits.iter()
            .map(|h| {
                let rel = h
                    .path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                format!(
                    "{rel}:{}:{} {}",
                    h.line,
                    h.column,
                    &h.text[h.column..h.column + h.len]
                )
            })
            .collect::<Vec<_>>()
    };
    let q = |text: &str| Query {
        text: text.into(),
        ..Query::default()
    };
    assert_eq!(
        run(q("apple")),
        ["a.org:1:2 Apple", "a.org:3:0 apple", "b/c.txt:1:4 apple"]
    );
    assert_eq!(
        run(Query {
            case_sensitive: true,
            ..q("APPLE")
        }),
        ["b/c.txt:1:14 APPLE"]
    );
    assert_eq!(
        run(Query {
            whole_word: true,
            ..q("apple")
        }),
        ["a.org:1:2 Apple", "a.org:3:0 apple", "b/c.txt:1:14 APPLE"]
    );
    assert_eq!(
        run(Query {
            regex: true,
            ..q("^no \\w+")
        }),
        ["a.org:2:0 no match"]
    );
    // Not a regular expression unless asked.
    assert!(run(q("a.p")).is_empty());
    assert!(
        kalem_project::search::search(
            &root,
            &[],
            &[],
            &Query {
                regex: true,
                ..q("(")
            },
            &AtomicBool::new(false),
            |_| true
        )
        .is_err()
    );
    // In the background.
    let s = Search::start(&root, &[], &[], q("apple"));
    let (hits, error) = s.wait();
    assert_eq!((hits.len(), error), (3, None));
    let s = Search::start(
        &root,
        &[],
        &[],
        Query {
            regex: true,
            ..q("[")
        },
    );
    assert!(s.wait().1.is_some());
}

#[test]
fn the_open_file_is_searched_though_gitignore_leaves_it_out() {
    let root = tree(
        "search-also",
        &[
            ("a.org", b"apple\n"),
            (".gitignore", b"TODO.md\n"),
            ("TODO.md", b"* Todo\nan apple a day\n"),
            ("logseq/config.edn", b"apple\n"),
        ],
    );
    let outside = tree("search-also-outside", &[("b.org", b"apple\n")]);
    let run = |ignore: &[String], also: &[std::path::PathBuf]| {
        let mut hits = Vec::new();
        kalem_project::search::search(
            &root,
            ignore,
            also,
            &Query {
                text: "apple".into(),
                ..Query::default()
            },
            &AtomicBool::new(false),
            |h| {
                hits.push(h);
                true
            },
        )
        .unwrap();
        let mut hits: Vec<String> = hits
            .iter()
            .map(|h| {
                let rel = h.path.strip_prefix(&root).unwrap_or(&h.path);
                format!("{}:{}", rel.to_string_lossy().replace('\\', "/"), h.line)
            })
            .collect();
        hits.sort();
        hits
    };
    assert_eq!(run(&[], &[]), ["a.org:1", "logseq/config.edn:1"]);
    // The open TODO.md, though .gitignore names it.
    let todo = root.join("TODO.md");
    assert_eq!(
        run(&[], std::slice::from_ref(&todo)),
        ["TODO.md:2", "a.org:1", "logseq/config.edn:1"]
    );
    // A file the walk reaches anyway is searched once; one outside the
    // project, or gone, not at all.
    assert_eq!(
        run(
            &[],
            &[
                root.join("a.org"),
                outside.join("b.org"),
                root.join("gone.md")
            ]
        ),
        ["a.org:1", "logseq/config.edn:1"]
    );
    // The project's own patterns still leave a file out, or its folder.
    let edn = root.join("logseq/config.edn");
    assert_eq!(
        run(&["TODO.md".into(), "/logseq/".into()], &[todo, edn]),
        ["a.org:1"]
    );
}

#[test]
fn large_tree() {
    let root = tree("large", &[]);
    for d in 0..50 {
        let dir = root.join(format!("d{d}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..100 {
            std::fs::write(dir.join(format!("f{f}.org")), format!("* file {d} {f}\n")).unwrap();
        }
    }
    let t = std::time::Instant::now();
    let idx = FileIndex::new(&root, &[]);
    let files = idx.wait();
    assert_eq!(files.len(), 5000);
    assert!(t.elapsed().as_secs() < 20);
    let s = Search::start(
        &root,
        &[],
        &[],
        Query {
            text: "file 7 42".into(),
            ..Query::default()
        },
    );
    let (hits, _) = s.wait();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, Path::new(&root).join("d7/f42.org"));
}
