// dired-tlu
//! Conditions for marking files: `size>10M age>30d type=file ext=log name~re`.
//! Pure parsing and matching; the caller supplies "now".

use std::time::{Duration, SystemTime};

use regex::Regex;

use crate::entry::{Entry, FileKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
}

impl Op {
    fn holds(self, a: u64, b: u64) -> bool {
        match self {
            Op::Gt => a > b,
            Op::Lt => a < b,
            Op::Ge => a >= b,
            Op::Le => a <= b,
            Op::Eq => a == b,
            Op::Ne => a != b,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeKind {
    File,
    Dir,
    Link,
    Exec,
}

#[derive(Debug, Clone)]
enum Term {
    Size(Op, u64),
    /// Seconds since modification.
    Age(Op, u64),
    /// `negate`, kind.
    Type(bool, TypeKind),
    /// `negate`, lowercase extensions.
    Ext(bool, Vec<String>),
    Name(Regex),
}

/// A parsed condition: every term must match.
#[derive(Debug, Clone)]
pub struct Predicate {
    terms: Vec<Term>,
}

const FIELDS: &str = "size, age, type, ext, name";

/// Split `12.5K` into the number and its unit (`K`).
fn number_and_unit<'a>(field: &str, v: &'a str) -> Result<(f64, &'a str), String> {
    let split = v.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f64 = num.parse().map_err(|_| format!("{field} needs a number, e.g. {field}>10"))?;
    Ok((n, unit))
}

fn parse_size(v: &str) -> Result<u64, String> {
    let (n, unit) = number_and_unit("size", v)?;
    let mult: f64 = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" => 1024.0,
        "m" => 1024.0 * 1024.0,
        "g" => 1024.0 * 1024.0 * 1024.0,
        "t" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return Err(format!("unknown size unit '{unit}' (B K M G T)")),
    };
    Ok((n * mult).round() as u64)
}

fn parse_age(v: &str) -> Result<u64, String> {
    let (n, unit) = number_and_unit("age", v)?;
    if unit.is_empty() {
        return Err("age needs a unit (s m h d w y), e.g. age>30d".into());
    }
    let mult: f64 = match unit.to_ascii_lowercase().as_str() {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3_600.0,
        "d" => 86_400.0,
        "w" => 604_800.0,
        "y" => 31_536_000.0,
        _ => return Err(format!("unknown age unit '{unit}' (s m h d w y)")),
    };
    Ok((n * mult).round() as u64)
}

fn parse_term(t: &str) -> Result<Term, String> {
    let Some(i) = t.find(|c: char| "<>=!~".contains(c)) else {
        return Err("expected FIELD OPERATOR VALUE, e.g. size>10M".into());
    };
    let (field, rest) = t.split_at(i);
    let (op, value) = ["!=", ">=", "<=", ">", "<", "=", "~"]
        .iter()
        .find_map(|o| rest.strip_prefix(o).map(|v| (*o, v)))
        .ok_or_else(|| format!("unknown operator in '{rest}'"))?;
    let cmp = match op {
        ">" => Some(Op::Gt),
        "<" => Some(Op::Lt),
        ">=" => Some(Op::Ge),
        "<=" => Some(Op::Le),
        "=" => Some(Op::Eq),
        "!=" => Some(Op::Ne),
        _ => None, // "~"
    };
    match field {
        "size" | "age" => {
            let Some(cmp) = cmp else { return Err(format!("{field} does not take '~'")) };
            if field == "size" {
                Ok(Term::Size(cmp, parse_size(value)?))
            } else {
                Ok(Term::Age(cmp, parse_age(value)?))
            }
        }
        "type" | "ext" => {
            let negate = match cmp {
                Some(Op::Eq) => false,
                Some(Op::Ne) => true,
                _ => return Err(format!("{field} takes only = or !=")),
            };
            if field == "type" {
                let k = match value {
                    "file" => TypeKind::File,
                    "dir" => TypeKind::Dir,
                    "link" => TypeKind::Link,
                    "exec" => TypeKind::Exec,
                    _ => return Err(format!("unknown type '{value}' (file, dir, link, exec)")),
                };
                Ok(Term::Type(negate, k))
            } else {
                let exts: Vec<String> =
                    value.split(',').filter(|s| !s.is_empty()).map(|s| s.to_lowercase()).collect();
                if exts.is_empty() {
                    return Err("ext needs at least one extension, e.g. ext=log,tmp".into());
                }
                Ok(Term::Ext(negate, exts))
            }
        }
        "name" => {
            if op != "~" {
                return Err("name needs '~' and a regexp, e.g. name~\\.bak$".into());
            }
            Regex::new(value).map(Term::Name).map_err(|e| format!("invalid regexp: {e}"))
        }
        other => Err(format!("unknown field '{other}' ({FIELDS})")),
    }
}

impl Term {
    /// `None` when the answer needs metadata that has not loaded yet.
    fn eval(&self, e: &Entry, now: SystemTime) -> Option<bool> {
        match self {
            Term::Size(op, n) => {
                if e.kind == FileKind::Dir {
                    return Some(false); // a directory's own size means nothing here
                }
                e.meta.as_ref().map(|m| op.holds(m.size, *n))
            }
            Term::Age(op, secs) => {
                let m = e.meta.as_ref()?;
                let Some(t) = m.modified else { return Some(false) };
                let age = now.duration_since(t).unwrap_or(Duration::ZERO).as_secs();
                Some(op.holds(age, *secs))
            }
            Term::Type(negate, k) => {
                let hit = match k {
                    TypeKind::File => e.kind == FileKind::File,
                    TypeKind::Dir => e.kind == FileKind::Dir,
                    TypeKind::Link => e.kind == FileKind::Symlink,
                    TypeKind::Exec => {
                        let m = e.meta.as_ref()?;
                        e.kind == FileKind::File && m.executable()
                    }
                };
                Some(hit != *negate)
            }
            Term::Ext(negate, list) => {
                let ext = e.extension().to_lowercase();
                Some(list.contains(&ext) != *negate)
            }
            Term::Name(re) => Some(re.is_match(&e.name)),
        }
    }
}

impl Predicate {
    pub fn parse(s: &str) -> Result<Predicate, String> {
        let mut terms = Vec::new();
        for t in s.split_whitespace() {
            terms.push(parse_term(t).map_err(|m| format!("bad term '{t}': {m}"))?);
        }
        if terms.is_empty() {
            return Err("empty condition: try size>10M, age>30d, type=file, ext=log or name~regexp".into());
        }
        Ok(Predicate { terms })
    }

    /// Whether `e` satisfies every term. `None` means a term needs metadata
    /// that has not loaded yet (and no other term already ruled the entry out).
    pub fn matches(&self, e: &Entry, now: SystemTime) -> Option<bool> {
        if e.special {
            return Some(false);
        }
        let mut unknown = false;
        for t in &self.terms {
            match t.eval(e, now) {
                Some(false) => return Some(false),
                Some(true) => {}
                None => unknown = true,
            }
        }
        if unknown {
            None
        } else {
            Some(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Entry, EntryMetadata, FileKind};
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::Duration;

    const NOW: u64 = 1_750_000_000;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(NOW)
    }

    fn entry(name: &str, kind: FileKind, size: u64, age_secs: Option<u64>, mode: u32) -> Entry {
        let mut e = Entry::new(OsString::from(name), Path::new(""), kind);
        let mut m = EntryMetadata::simple(kind, size);
        m.modified = age_secs.map(|a| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW - a));
        m.mode = Some(mode);
        e.meta = Some(m);
        e
    }

    fn file(name: &str, size: u64, age_secs: u64) -> Entry {
        entry(name, FileKind::File, size, Some(age_secs), 0o644)
    }

    fn check(expr: &str, e: &Entry) -> Option<bool> {
        Predicate::parse(expr).unwrap_or_else(|err| panic!("{expr}: {err}")).matches(e, now())
    }

    const DAY: u64 = 86_400;

    #[test]
    fn size_comparisons_and_units() {
        let f = file("a", 10 * 1024 * 1024, 0);
        assert_eq!(check("size>10M", &f), Some(false));
        assert_eq!(check("size>=10M", &f), Some(true));
        assert_eq!(check("size=10M", &f), Some(true));
        assert_eq!(check("size!=10M", &f), Some(false));
        assert_eq!(check("size<11M", &f), Some(true));
        assert_eq!(check("size>10240K", &f), Some(false));
        assert_eq!(check("size>=10240k", &f), Some(true), "units are case-insensitive");
        assert_eq!(check("size>1.5K", &file("b", 1600, 0)), Some(true));
        assert_eq!(check("size=0", &file("c", 0, 0)), Some(true));
        assert_eq!(check("size<1G", &f), Some(true));
        assert_eq!(check("size<1T", &f), Some(true));
        assert_eq!(check("size=5B", &file("d", 5, 0)), Some(true));
    }

    #[test]
    fn directories_never_match_size_terms() {
        let d = entry("sub", FileKind::Dir, 4096, Some(0), 0o755);
        assert_eq!(check("size>1K", &d), Some(false));
        assert_eq!(check("size<1G", &d), Some(false));
    }

    #[test]
    fn age_is_time_since_modification() {
        let f = file("a", 1, 3 * DAY);
        assert_eq!(check("age>2d", &f), Some(true));
        assert_eq!(check("age>3d", &f), Some(false));
        assert_eq!(check("age>=3d", &f), Some(true));
        assert_eq!(check("age<1w", &f), Some(true));
        assert_eq!(check("age<72h", &f), Some(false));
        assert_eq!(check("age>=4320m", &f), Some(true));
        assert_eq!(check("age<1y", &f), Some(true));
        assert_eq!(check("age>259199s", &f), Some(true));
        // Modified in the future counts as just now.
        let future = Entry { meta: Some({
            let mut m = EntryMetadata::simple(FileKind::File, 1);
            m.modified = Some(now() + Duration::from_secs(500));
            m
        }), ..file("f", 1, 0) };
        assert_eq!(check("age<1s", &future), Some(true));
        // A known entry without a modification time cannot be old.
        let none = entry("n", FileKind::File, 1, None, 0o644);
        assert_eq!(check("age>1s", &none), Some(false));
    }

    #[test]
    fn type_ext_and_name() {
        let exe = entry("run", FileKind::File, 1, Some(0), 0o755);
        let link = entry("ln", FileKind::Symlink, 1, Some(0), 0o777);
        let dir = entry("sub", FileKind::Dir, 1, Some(0), 0o755);
        let log = file("app.LOG", 1, 0);
        assert_eq!(check("type=exec", &exe), Some(true));
        assert_eq!(check("type=exec", &log), Some(false));
        assert_eq!(check("type=file", &exe), Some(true));
        assert_eq!(check("type=link", &link), Some(true));
        assert_eq!(check("type=dir", &dir), Some(true));
        assert_eq!(check("type!=dir", &dir), Some(false));
        assert_eq!(check("type!=dir", &log), Some(true));
        assert_eq!(check("ext=log", &log), Some(true), "case-insensitive");
        assert_eq!(check("ext=tmp,log", &log), Some(true));
        assert_eq!(check("ext!=log", &log), Some(false));
        assert_eq!(check("ext=rs", &log), Some(false));
        assert_eq!(check(r"name~\.log$", &log), Some(false), "name is matched case-sensitively");
        assert_eq!(check(r"name~(?i)\.log$", &log), Some(true));
        assert_eq!(check("name~^run", &exe), Some(true));
    }

    #[test]
    fn all_terms_must_match() {
        let old_log = file("a.log", 5000, 40 * DAY);
        let new_log = file("b.log", 5000, DAY);
        let old_txt = file("c.txt", 5000, 40 * DAY);
        let expr = "type=file ext=log age>30d size>1K";
        assert_eq!(check(expr, &old_log), Some(true));
        assert_eq!(check(expr, &new_log), Some(false));
        assert_eq!(check(expr, &old_txt), Some(false));
        assert_eq!(check("  ext=log   age>30d  ", &old_log), Some(true), "extra spaces are fine");
    }

    #[test]
    fn unloaded_metadata_is_unknown_unless_another_term_already_failed() {
        let mut e = Entry::new(OsString::from("a.log"), Path::new(""), FileKind::File);
        e.meta = None;
        assert_eq!(check("size>1K", &e), None);
        assert_eq!(check("age>1d", &e), None);
        assert_eq!(check("type=exec", &e), None);
        assert_eq!(check("ext=log", &e), Some(true), "name-only terms need no metadata");
        assert_eq!(check("ext=rs size>1K", &e), Some(false), "a definite miss wins over unknown");
        assert_eq!(check("ext=log size>1K", &e), None);
        assert_eq!(check("type=file", &e), Some(true), "kind is known from the listing");
    }

    #[test]
    fn the_parent_row_never_matches() {
        let p = Entry::parent_link();
        assert_eq!(check("name~.", &p), Some(false));
        assert_eq!(check("type=dir", &p), Some(false));
    }

    #[test]
    fn errors_name_the_bad_term() {
        for (expr, want) in [
            ("", "empty"),
            ("   ", "empty"),
            ("size>10Q", "unknown size unit 'Q'"),
            ("size>", "needs a number"),
            ("size>abc", "needs a number"),
            ("age>3", "needs a unit"),
            ("age>3x", "unknown age unit 'x'"),
            ("foo>3", "unknown field 'foo'"),
            ("size", "expected"),
            ("size~3", "does not take '~'"),
            ("name=foo", "name needs '~'"),
            ("type>dir", "type takes only = or !="),
            ("type=pipe", "unknown type 'pipe'"),
            ("ext>log", "ext takes only = or !="),
            ("ext=", "needs at least one extension"),
            (r"name~(", "invalid regexp"),
            ("size>1K junk", "bad term 'junk'"),
        ] {
            let err = Predicate::parse(expr).expect_err(expr);
            assert!(err.contains(want), "{expr:?}: {err:?} should contain {want:?}");
        }
        let err = Predicate::parse("age>1d size>10Q").unwrap_err();
        assert!(err.starts_with("bad term 'size>10Q'"), "{err}");
    }
}
