// dired-7qs
//! Argument-safe command construction. File operands are always passed as
//! discrete argv values; they are never interpolated into a shell string.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// One process to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// True when the command string is interpreted by a shell (opt-in).
    pub shell_mode: bool,
}

impl Invocation {
    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.shell_mode {
            s.push_str("[SHELL] ");
        }
        s.push_str(&self.program.to_string_lossy());
        for a in &self.args {
            s.push(' ');
            let a = a.to_string_lossy();
            match shlex::try_quote(&a) {
                Ok(q) => s.push_str(&q),
                Err(_) => s.push_str(&format!("{a:?}")),
            }
        }
        s
    }
}

/// Turn an operand path into an argv value relative to `cwd`, protected
/// against being parsed as an option.
pub fn operand_arg(path: &Path, cwd: &Path) -> OsString {
    let rel = path.strip_prefix(cwd).unwrap_or(path);
    let rel = if rel.as_os_str().is_empty() { Path::new(".") } else { rel };
    if rel.to_string_lossy().starts_with('-') {
        let mut s = OsString::from("./");
        s.push(rel.as_os_str());
        s
    } else {
        rel.as_os_str().to_os_string()
    }
}

/// Build invocations for a Dired shell command.
///
/// Argv mode: the template is split into words with POSIX-like quoting.
/// A standalone `*` word expands to all operands (one invocation); a
/// standalone `?` word runs the command once per operand; otherwise the
/// operands are appended.
///
/// Shell mode: the template is run by `shell -c`, and operands are passed
/// as positional parameters (`"$@"` / `"$1"`), never spliced into the string.
pub fn build_invocations(
    template: &str,
    operands: &[PathBuf],
    cwd: &Path,
    shell: Option<&str>,
) -> Result<Vec<Invocation>, String> {
    let args: Vec<OsString> = operands.iter().map(|p| operand_arg(p, cwd)).collect();
    if let Some(shell) = shell {
        let words: Vec<&str> = template.split_whitespace().collect();
        let per_file = words.contains(&"?");
        let script: String = if per_file {
            words
                .iter()
                .map(|w| if *w == "?" { "\"$1\"" } else { w })
                .collect::<Vec<_>>()
                .join(" ")
        } else if words.contains(&"*") {
            words
                .iter()
                .map(|w| if *w == "*" { "\"$@\"" } else { w })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            format!("{template} \"$@\"")
        };
        let mk = |files: Vec<OsString>| {
            let mut a = vec![OsString::from("-c"), OsString::from(&script), OsString::from("dired")];
            a.extend(files);
            Invocation {
                program: OsString::from(shell),
                args: a,
                cwd: cwd.to_path_buf(),
                shell_mode: true,
            }
        };
        return Ok(if per_file {
            args.into_iter().map(|a| mk(vec![a])).collect()
        } else {
            vec![mk(args)]
        });
    }
    let words = shlex::split(template).ok_or("unbalanced quotes in command")?;
    if words.is_empty() {
        return Err("empty command".into());
    }
    let per_file = words.iter().any(|w| w == "?");
    let star = words.iter().any(|w| w == "*");
    let expand = |files: &[OsString]| -> Vec<OsString> {
        let mut out = Vec::new();
        for w in &words {
            if (per_file && w == "?") || (!per_file && star && w == "*") {
                out.extend(files.iter().cloned());
            } else {
                out.push(OsString::from(w));
            }
        }
        if !per_file && !star {
            out.extend(files.iter().cloned());
        }
        out
    };
    let mk = |argv: Vec<OsString>| {
        let mut it = argv.into_iter();
        Invocation {
            program: it.next().unwrap_or_default(),
            args: it.collect(),
            cwd: cwd.to_path_buf(),
            shell_mode: false,
        }
    };
    Ok(if per_file {
        args.iter().map(|a| mk(expand(std::slice::from_ref(a)))).collect()
    } else {
        vec![mk(expand(&args))]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_by_default() {
        let inv = build_invocations(
            "wc -l",
            &[PathBuf::from("/d/a b"), PathBuf::from("/d/-x")],
            Path::new("/d"),
            None,
        )
        .unwrap();
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].program, "wc");
        assert_eq!(inv[0].args, vec!["-l", "a b", "./-x"]);
    }

    #[test]
    fn per_file_and_star() {
        let ops = [PathBuf::from("/d/a"), PathBuf::from("/d/b")];
        let inv = build_invocations("cp ? ?.bak", &ops, Path::new("/d"), None).unwrap();
        // `?.bak` is not a standalone `?` word, so it stays literal.
        assert_eq!(inv.len(), 2);
        assert_eq!(inv[1].args, vec!["b", "?.bak"]);
        let inv = build_invocations("tar cf x.tar *", &ops, Path::new("/d"), None).unwrap();
        assert_eq!(inv[0].args, vec!["cf", "x.tar", "a", "b"]);
    }

    #[test]
    fn shell_mode_uses_positional() {
        let ops = [PathBuf::from("/d/$(rm -rf ~)")];
        let inv = build_invocations("ls -l | head", &ops, Path::new("/d"), Some("/bin/sh")).unwrap();
        assert!(inv[0].shell_mode);
        assert_eq!(inv[0].args[1], "ls -l | head \"$@\"");
        assert_eq!(inv[0].args[3], "$(rm -rf ~)");
    }
}
