//! The globs a step's `on` list is written in, and matching a staged path
//! against them.
//!
//! The syntax is the part of `.gitignore` everyone already knows, and nothing
//! else: `*` and `?` within one path segment, `**` as a whole segment for any
//! number of them, and a pattern with no `/` in it matching the file's name at
//! any depth. Every other character is literal. Pure: paths in, a yes or no
//! out.

use anyhow::{Result, bail};

/// One parsed glob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// Whether the pattern is matched against the whole path from the
    /// repository root, rather than against the file name alone.
    anchored: bool,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// `**`: zero or more whole path segments.
    AnyDepth,
    /// One path segment, in which `*` and `?` are wildcards.
    Glob(String),
}

impl Pattern {
    /// Parse one glob, refusing the shapes that would match nothing or would
    /// mean something different from what they look like.
    pub fn parse(text: &str) -> Result<Self> {
        if text.is_empty() {
            bail!("an empty pattern matches nothing");
        }
        let (rooted, body) = match text.strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, text),
        };
        if body.is_empty() {
            bail!("`{text}` alone matches nothing");
        }
        // `.gitignore` reads a trailing slash as "a directory", and a staged
        // path is never a directory, so the pattern would silently match
        // nothing at all.
        if body.ends_with('/') {
            bail!("`{text}` would match a directory, never a file; write `{text}**`");
        }
        if body.split('/').any(str::is_empty) {
            bail!("`{text}` has an empty path segment");
        }

        let segments = body
            .split('/')
            .map(|segment| match segment {
                "**" => Segment::AnyDepth,
                glob => Segment::Glob(glob.to_owned()),
            })
            .collect();
        Ok(Self { anchored: rooted || body.contains('/'), segments })
    }

    /// Whether `path`, relative to the repository root with `/` separators as
    /// git reports it, is matched by this pattern.
    pub fn matches(&self, path: &str) -> bool {
        if self.anchored {
            let parts: Vec<&str> = path.split('/').collect();
            match_segments(&self.segments, &parts)
        } else {
            let name = path.rsplit('/').next().unwrap_or(path);
            match_segments(&self.segments, &[name])
        }
    }
}

/// Match pattern segments against path segments, `**` taking as many as it
/// needs.
///
/// A trailing `**` takes at least one: `tests/**` is everything *inside*
/// `tests`, which is how `.gitignore` reads it, and a file called `tests` is
/// not inside itself.
fn match_segments(pattern: &[Segment], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((Segment::AnyDepth, rest)) => {
            let fewest = usize::from(rest.is_empty());
            (fewest..=path.len()).any(|taken| match_segments(rest, &path[taken..]))
        }
        Some((Segment::Glob(glob), rest)) => path
            .split_first()
            .is_some_and(|(first, tail)| glob_segment(glob, first) && match_segments(rest, tail)),
    }
}

/// `*` and `?` within one segment, which never contains a `/`.
///
/// The usual backtracking-to-the-last-star walk: linear in practice, and with
/// no recursion for a pattern to make deep.
fn glob_segment(glob: &str, name: &str) -> bool {
    let glob: Vec<char> = glob.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut g, mut n) = (0, 0);
    // Where the last `*` was, and where in the name it is currently assumed
    // to stop.
    let mut star: Option<(usize, usize)> = None;

    while n < name.len() {
        match glob.get(g) {
            Some('*') => {
                star = Some((g, n));
                g += 1;
            }
            Some(&ch) if ch == '?' || ch == name[n] => {
                g += 1;
                n += 1;
            }
            _ => match star {
                Some((star_g, star_n)) => {
                    g = star_g + 1;
                    n = star_n + 1;
                    star = Some((star_g, star_n + 1));
                }
                None => return false,
            },
        }
    }
    glob[g..].iter().all(|&ch| ch == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, path: &str) -> bool {
        Pattern::parse(pattern).unwrap().matches(path)
    }

    #[test]
    fn a_pattern_without_a_slash_matches_the_name_at_any_depth() {
        assert!(matches("*.py", "a.py"), "at the root");
        assert!(matches("*.py", "pkg/sub/a.py"), "and below it, as `*.py` did as a git pathspec");
        assert!(matches("uv.lock", "uv.lock"), "a literal name");
        assert!(matches("uv.lock", "services/api/uv.lock"), "is a name, not a root path");
        assert!(!matches("*.py", "a.pyc"), "the whole name has to match, not a prefix of it");
        assert!(!matches("*.py", "pkg.py/notes.txt"), "a directory's name is not the file's");
    }

    #[test]
    fn a_pattern_with_a_slash_is_anchored_at_the_root() {
        assert!(matches("tests/*.py", "tests/test_a.py"));
        assert!(!matches("tests/*.py", "src/tests/test_a.py"), "anchored, not floating");
        assert!(matches("/uv.lock", "uv.lock"), "a leading slash anchors a bare name");
        assert!(!matches("/uv.lock", "services/uv.lock"), "and keeps it at the root");
    }

    #[test]
    fn a_single_star_stays_inside_one_segment() {
        assert!(!matches("tests/*.py", "tests/unit/test_a.py"), "`*` does not cross a `/`");
        assert!(matches("tests/*/test_a.py", "tests/unit/test_a.py"));
        assert!(matches("test_?.py", "test_a.py"), "`?` is one character");
        assert!(!matches("test_?.py", "test_ab.py"), "and exactly one");
        assert!(!matches("a?b", "a/b"), "and never a `/`");
    }

    #[test]
    fn a_double_star_segment_spans_any_number_of_directories() {
        assert!(matches("tests/**", "tests/a.py"), "one level");
        assert!(matches("tests/**", "tests/unit/deep/a.json"), "many levels");
        assert!(!matches("tests/**", "tests"), "something has to be inside the directory");
        assert!(matches("**/fixtures/*.json", "fixtures/a.json"), "zero leading directories");
        assert!(matches("**/fixtures/*.json", "tests/unit/fixtures/a.json"), "or several");
        assert!(matches("src/**/*.sql", "src/q.sql"), "zero in the middle");
        assert!(matches("src/**/*.sql", "src/db/sql/q.sql"), "or several in the middle");
        assert!(!matches("src/**/*.sql", "lib/q.sql"), "still anchored at `src`");
    }

    #[test]
    fn a_double_star_inside_a_segment_is_just_a_star() {
        assert!(matches("a**b.py", "axyzb.py"), "as in .gitignore");
        assert!(!matches("tests/a**", "tests/a/b"), "and like `*` it stays in its segment");
    }

    #[test]
    fn everything_else_is_literal() {
        assert!(matches("[ab].py", "[ab].py"), "no character classes");
        assert!(!matches("[ab].py", "a.py"), "so a bracket only matches a bracket");
        assert!(matches("a+b.py", "a+b.py"));
    }

    #[test]
    fn a_pattern_that_cannot_mean_what_it_says_is_refused() {
        for (text, why) in [
            ("", "an empty pattern"),
            ("/", "a bare slash"),
            ("tests/", "a trailing slash, which .gitignore reads as `directory`"),
            ("a//b", "an empty segment"),
        ] {
            assert!(Pattern::parse(text).is_err(), "{why} should be refused: `{text}`");
        }
        let err = format!("{:#}", Pattern::parse("tests/").unwrap_err());
        assert!(err.contains("tests/**"), "the refusal says what to write instead, got: {err}");
    }
}
