// dired-p2d
//! Per-style rendering tests (ratatui TestBackend).

mod common;

use std::sync::Arc;

use common::*;
use dired_testkit::MockFs;

fn char_index(row: &str, needle: &str) -> usize {
    let byte = row.find(needle).unwrap_or_else(|| panic!("`{needle}` not in `{row}`"));
    row[..byte].chars().count()
}

#[tokio::test]
async fn size_column_is_colored_by_magnitude_unless_disabled() {
    for (extra, colored) in [("", true), ("[display.ui]\ncolumn_colors = false\n", false)] {
        let fs = Arc::new(MockFs::new());
        fs.write("/w/big.bin", &"x".repeat(5000)); // 4.9K: the "normal" tier
        fs.write("/w/z.txt", "");
        let c = cfg(&format!("version = 1\n[theme]\nname = \"nord\"\n{extra}"));
        let theme = theme_of(&c);
        let mut ui = Ui::open(fs, c, 80, 10, "/w").await;
        ui.app.press("M->"); // point on z.txt, so big.bin's row is not highlighted
        let rows = ui.draw();
        let (y, row) = rows.iter().enumerate().find(|(_, r)| r.contains("big.bin")).expect("row");
        let x = char_index(row, &dired_domain::format_size(5000)) as u16;
        let want = if colored { theme.normal.fg } else { theme.dim.fg };
        assert_eq!(ui.cell_fg(x, y as u16), want, "colored={colored}\n{}", rows.join("\n"));
    }
}
