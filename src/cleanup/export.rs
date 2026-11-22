//! "Export to CSV" for candidates and the review list: writes a report file, deletes nothing.

use crate::fmt::csv_field;
use std::io::Write;
use std::path::Path;

pub struct ExportRow {
    pub path: String,
    pub size: u64,
    pub kind: String,
    pub reason: String,
}

pub fn write_csv(file: &Path, rows: &[ExportRow]) -> std::io::Result<()> {
    let mut w = std::io::BufWriter::new(std::fs::File::create(file)?);
    writeln!(w, "path,size_bytes,size,kind,reason")?;
    for r in rows {
        writeln!(
            w,
            "{},{},{},{},{}",
            csv_field(&r.path),
            r.size,
            csv_field(&crate::fmt::size(r.size)),
            csv_field(&r.kind),
            csv_field(&r.reason)
        )?;
    }
    w.flush()
}

#[test]
fn writes_quoted_rows() {
    let f = std::env::temp_dir().join(format!("diskscope-export-{}.csv", std::process::id()));
    let rows = [ExportRow { path: "C:\\a, b".into(), size: 2048, kind: "node_modules".into(), reason: "say \"hi\"".into() }];
    write_csv(&f, &rows).unwrap();
    let s = std::fs::read_to_string(&f).unwrap();
    std::fs::remove_file(&f).ok();
    assert!(s.contains("\"C:\\a, b\",2048,2.0 KB,node_modules,\"say \"\"hi\"\"\""));
}
