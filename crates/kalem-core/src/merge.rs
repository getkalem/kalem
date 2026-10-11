//! Three texts merged line by line: the file as the editor last read it,
//! the document as the user changed it since, and the file as another
//! program (Logseq, Obsidian, a sync) wrote it meanwhile. Where the two
//! changed different lines both changes are kept; where they changed the
//! same lines differently there is no merge, and the user decides.

/// The longest common subsequence of `a` and `b`, as the pairs of their
/// lines that match, in order (Myers' algorithm, in time and space of
/// their length times the lines that differ); none past `max` differing
/// lines.
fn matches(a: &[&str], b: &[&str], max: usize) -> Option<Vec<(usize, usize)>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let off = (n + m) as usize + 1;
    let mut v = vec![0isize; 2 * off + 1];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=(n + m).min(max as isize) {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let i = (k + off as isize) as usize;
            let mut x = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
                v[i + 1]
            } else {
                v[i - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    let d_end = found?;
    // Back from the end through the steps taken.
    let mut pairs = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..=d_end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let i = |k: isize| (k + off as isize) as usize;
        let prev_k = if k == -d || (k != d && v[i(k - 1)] < v[i(k + 1)]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[i(prev_k)];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            pairs.push((x as usize, y as usize));
        }
        x = prev_x;
        y = prev_y;
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        pairs.push((x as usize, y as usize));
    }
    pairs.reverse();
    Some(pairs)
}

/// `ours` and `theirs`, both changed from `base`, merged: each line
/// either kept from `base` or changed on one side only, or changed the
/// same way on both. None when both changed the same lines differently,
/// or when too much differs to compare.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Option<String> {
    const MAX: usize = 5000;
    fn lines(s: &str) -> Vec<&str> {
        s.split_inclusive('\n').collect()
    }
    let (b, o, t) = (lines(base), lines(ours), lines(theirs));
    let mut o_of = vec![None; b.len()];
    for (i, j) in matches(&b, &o, MAX)? {
        o_of[i] = Some(j);
    }
    let mut t_of = vec![None; b.len()];
    for (i, j) in matches(&b, &t, MAX)? {
        t_of[i] = Some(j);
    }
    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let (mut ib, mut io, mut it) = (0, 0, 0);
    loop {
        // The next line of `base` both kept, after what is merged.
        let stable = (ib..b.len()).find_map(|j| match (o_of[j], t_of[j]) {
            (Some(x), Some(y)) if x >= io && y >= it => Some((j, x, y)),
            _ => None,
        });
        let (jb, jo, jt) = stable.unwrap_or((b.len(), o.len(), t.len()));
        let (cb, co, ct) = (&b[ib..jb], &o[io..jo], &t[it..jt]);
        let chunk = if co == cb {
            ct
        } else if ct == cb || co == ct {
            co
        } else {
            return None;
        };
        for l in chunk {
            out.push_str(l);
        }
        let Some(_) = stable else { break };
        out.push_str(b[jb]);
        (ib, io, it) = (jb + 1, jo + 1, jt + 1);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_to_different_lines_are_both_kept() {
        let base = "a\nb\nc\nd\ne\n";
        let ours = "a\nB, typed here\nc\nd\ne\n";
        let theirs = "a\nb\nc\nD, written elsewhere\ne\nf\n";
        assert_eq!(
            merge3(base, ours, theirs).as_deref(),
            Some("a\nB, typed here\nc\nD, written elsewhere\ne\nf\n")
        );
        // Lines added and removed on either side.
        assert_eq!(
            merge3("a\nb\nc\n", "a\nb\nc\nours\n", "theirs\na\nc\n").as_deref(),
            Some("theirs\na\nc\nours\n")
        );
        // The same change on both sides: once.
        assert_eq!(
            merge3("a\nb\n", "a\nB\n", "a\nB\n").as_deref(),
            Some("a\nB\n")
        );
        // No final line feed on one side (Logseq writes none).
        assert_eq!(
            merge3("a\nb\nc\n", "A\nb\nc\n", "a\nb\nc").as_deref(),
            Some("A\nb\nc")
        );
    }

    #[test]
    fn the_same_lines_changed_differently_are_no_merge() {
        assert_eq!(merge3("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n"), None);
        // Next to each other, the change of one line touching the other's.
        assert_eq!(merge3("a\nb\n", "a\nb\nours\n", "a\nb\ntheirs\n"), None);
    }

    use proptest::prelude::*;

    /// The lines of `region` changed by `ops`, one for each line: kept,
    /// removed, replaced, or a new line put before it; the new lines
    /// named by `tag`, so that no line is in two places.
    fn edited(region: &[String], ops: &[u8], tag: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (i, l) in region.iter().enumerate() {
            match ops.get(i).copied().unwrap_or(0) % 4 {
                0 => out.push(l.clone()),
                1 => {}
                2 => out.push(format!("{tag}{i}\n")),
                _ => {
                    out.push(format!("{tag}+{i}\n"));
                    out.push(l.clone());
                }
            }
        }
        out
    }

    proptest! {
        /// Changes on either side of a line neither touched: both kept.
        #[test]
        fn changes_apart_are_both_kept(
            n in 3usize..40,
            at in 0usize..1000,
            ours in prop::collection::vec(any::<u8>(), 0..40),
            theirs in prop::collection::vec(any::<u8>(), 0..40),
        ) {
            let base: Vec<String> = (0..n).map(|i| format!("l{i}\n")).collect();
            let s = 1 + at % (n - 2);
            let o = edited(&base[..s], &ours, "o");
            let t = edited(&base[s + 1..], &theirs, "t");
            let sep = std::slice::from_ref(&base[s]);
            let ours_text = [&o[..], sep, &base[s + 1..]].concat().concat();
            let theirs_text = [&base[..s], sep, &t[..]].concat().concat();
            let want = [&o[..], sep, &t[..]].concat().concat();
            prop_assert_eq!(merge3(&base.concat(), &ours_text, &theirs_text), Some(want));
        }

        /// Any three texts: one side unchanged, or both changed alike,
        /// gives the other; and the merge is the same whichever side is
        /// whose.
        #[test]
        fn any_three_texts(b in "[ab\n]{0,40}", o in "[ab\n]{0,40}", t in "[ab\n]{0,40}") {
            prop_assert_eq!(merge3(&b, &o, &b), Some(o.clone()));
            prop_assert_eq!(merge3(&b, &b, &t), Some(t.clone()));
            prop_assert_eq!(merge3(&b, &o, &o), Some(o.clone()));
            prop_assert_eq!(merge3(&b, &o, &t), merge3(&b, &t, &o));
        }
    }

    #[test]
    fn the_longest_common_lines() {
        let a = ["x\n", "a\n", "b\n", "c\n"];
        let b = ["a\n", "c\n", "y\n"];
        assert_eq!(matches(&a, &b, 100), Some(vec![(1, 0), (3, 1)]));
        assert_eq!(matches(&a, &b, 1), None);
        assert_eq!(matches(&[], &[], 0), Some(vec![]));
    }
}
