// dired-7qs
use dired_config::{parse_seq, Config};
use proptest::prelude::*;

proptest! {
    /// Config parsing never panics and always yields a usable config.
    #[test]
    fn config_parse_total(s in "\\PC{0,200}") {
        let (c, _) = Config::parse(&s, "fuzz");
        prop_assert!(c.version > 0);
    }

    #[test]
    fn config_parse_tomlish(s in "(version = [0-9]\n)?(\\[(display|theme|safety|keymap)\\]\n)?([a-z_]{1,8} = (\"[a-z ]{0,5}\"|[0-9]{1,3}|true)\n){0,6}") {
        let _ = Config::parse(&s, "fuzz");
    }

    /// Key sequences round-trip through their display form.
    #[test]
    fn keys_roundtrip(s in "((C-)?(M-)?[a-z%$!&^+]|RET|TAB|SPC|<up>)( ((C-)?(M-)?[a-z%$]|RET)){0,3}") {
        let keys = parse_seq(&s).unwrap();
        prop_assert_eq!(parse_seq(&dired_config::format_seq(&keys)).unwrap(), keys);
    }
}
