// dired-7qs
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;

use dired_domain::plan::{build_rename_plan, RenameRequest};
use dired_domain::{display_name, shell::build_invocations};
use proptest::prelude::*;

proptest! {
    /// Any permutation of names within a directory is scheduled without
    /// overwriting and ends in the requested state.
    #[test]
    fn rename_permutations_are_safe(perm in Just((0..6usize).collect::<Vec<_>>()).prop_shuffle()) {
        prop_assume!(perm.iter().enumerate().any(|(i, p)| i != *p));
        let names: Vec<String> = (0..6).map(|i| format!("f{i}")).collect();
        let dir = PathBuf::from("/d");
        let mut existing = HashMap::new();
        existing.insert(dir.clone(), names.iter().map(OsString::from).collect::<HashSet<_>>());
        let reqs: Vec<RenameRequest> = names.iter().enumerate().map(|(i, n)| RenameRequest {
            src: dir.join(n), key: None, new_name: names[perm[i]].clone(), is_dir: false,
        }).collect();
        let plan = build_rename_plan(1, reqs, &existing);
        prop_assert!(plan.errors.is_empty());
        let mut fs: HashMap<PathBuf, usize> = names.iter().enumerate().map(|(i, n)| (dir.join(n), i)).collect();
        for s in &plan.steps {
            prop_assert!(!fs.contains_key(&s.to));
            let v = fs.remove(&s.from).unwrap();
            fs.insert(s.to.clone(), v);
        }
        for (i, n) in names.iter().enumerate() {
            prop_assert_eq!(fs[&dir.join(&names[perm[i]])], i, "{}", n);
        }
    }

    /// Display names never contain control characters.
    #[test]
    fn display_names_are_single_line(s in "\\PC*|[\\x00-\\x1f]{0,5}") {
        let d = display_name(std::ffi::OsStr::new(&s));
        prop_assert!(!d.chars().any(|c| c.is_control()));
    }

    /// Operands are passed verbatim as argv values.
    #[test]
    fn argv_operands_verbatim(name in "[a-zA-Z0-9 '\"$;&|<>*?()\\\\]{1,20}") {
        let cwd = PathBuf::from("/d");
        let inv = build_invocations("echo", &[cwd.join(&name)], &cwd, None).unwrap();
        let arg = inv[0].args.last().unwrap().to_string_lossy().into_owned();
        let expect = if name.starts_with('-') { format!("./{name}") } else { name.clone() };
        prop_assert_eq!(arg, expect);
    }
}
