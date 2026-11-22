//! Builds the CLI report (JSON value, printed as JSON or as plain text).

use super::Options;
use crate::fmt::{count, date, size};
use crate::model::Tree;
use crate::rules::{ao_sessions::AoSessions, candidates};
use crate::scan::{extensions, Progress};
use crate::views::queries::{files_where, folders_to_depth, folders_where, Hit, TimeField};
use crate::views::snapshot;
use serde_json::{json, Value};

pub fn run(o: &Options) -> i32 {
    if let Some(p) = o.paths.iter().find(|p| !p.exists()) {
        eprintln!("error: {} does not exist", p.display());
        return 2;
    }
    let tree = match crate::scan::scan_many(&o.paths, &Progress::default(), o.method) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return 3;
        }
    };
    if crate::scan::profile::on() {
        // Time what the window does after a scan (snapshot written to a temp file, not the baseline).
        let tmp = std::env::temp_dir().join(format!("diskscope-profile-{}.snap", std::process::id()));
        let _ = crate::scan::profile::time("derive: snapshot save", || snapshot::save(&tree, &tmp));
        std::fs::remove_file(&tmp).ok();
        drop(crate::gui::loaded::Derived::compute(&tree, &AoSessions::default()));
        eprintln!("[profile] peak private memory            {:>9} MB", crate::sys::memory().peak >> 20);
    }
    let report = build(&tree, o);
    if o.save_snapshot {
        if let Err(e) = snapshot::save(&tree, &snapshot::snapshot_file(&tree.root_path)) {
            eprintln!("warning: snapshot not saved: {e}");
        }
    }
    if o.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        print_text(&report);
    }
    0
}

fn hits(tree: &Tree, h: &[Hit], time: Option<TimeField>) -> Value {
    Value::Array(
        h.iter()
            .map(|h| {
                let mut v = json!({ "path": tree.path(h.node).display().to_string(), "bytes": h.bytes, "size": size(h.bytes) });
                if let Some(t) = time {
                    v["time"] = json!(date(t.of(tree.node(h.node))));
                }
                v
            })
            .collect(),
    )
}

fn build(tree: &Tree, o: &Options) -> Value {
    let root = o.under.as_ref().and_then(|u| tree.find(u)).unwrap_or(0);
    let r = tree.node(0);
    let now = crate::fmt::now();
    let mut v = json!({
        "root": tree.root_display(),
        "under": tree.path(root).display().to_string(),
        "scanned_at": date(tree.scanned_at),
        "elapsed_ms": tree.elapsed_ms,
        "bytes": r.size, "size": size(r.size), "allocated": size(r.alloc),
        "files": r.files, "items": r.items,
        "denied_folders": tree.denied, "reparse_points_skipped": tree.reparse_skipped,
        "memory_mb": crate::sys::memory().process / (1 << 20),
        "memory_peak_mb": crate::sys::memory().peak / (1 << 20),
        "method": tree.method, "method_note": tree.method_note,
        "volume_used": tree.volume_used.map(size), "volume_used_bytes": tree.volume_used,
        "note": "read-only report; diskscope's CLI never deletes anything",
        "top_files": hits(tree, &files_where(tree, root, o.top, |n| !n.is_group()), Some(TimeField::Modified)),
        "top_folders": hits(tree, &folders_where(tree, root, o.top, 1 << 20, |_| true), None),
    });
    let stats = extensions::ext_stats(tree, root);
    v["extensions"] = stats
        .iter()
        .take(15)
        .map(|s| json!({ "ext": tree.exts[s.ext as usize], "type": extensions::type_name(&tree.exts[s.ext as usize]), "bytes": s.size, "size": size(s.size), "files": s.files }))
        .collect();
    if let Some(age) = o.recent {
        let since = now.saturating_sub(age);
        let pred = |n: &crate::model::Node| TimeField::Changed.of(n) >= since;
        v["recent"] = json!({ "since": date(since), "files": hits(tree, &files_where(tree, root, o.top, pred), Some(TimeField::Changed)),
            "folders": hits(tree, &folders_where(tree, root, o.top, 1 << 20, pred), None) });
    }
    if let Some(age) = o.least_used {
        let field = if crate::sys::last_access_enabled() == Some(true) { TimeField::Accessed } else { TimeField::Modified };
        let before = now.saturating_sub(age);
        let pred = |n: &crate::model::Node| field.of(n) < before;
        v["least_used"] = json!({ "by": format!("{field:?}").to_lowercase(), "older_than": date(before),
            "files": hits(tree, &files_where(tree, root, o.top, pred), Some(field)),
            "folders": hits(tree, &folders_where(tree, root, o.top, 1 << 20, pred), None) });
    }
    if let Some(depth) = o.depth {
        v["folders"] = folders_to_depth(tree, root, depth, 5000)
            .iter()
            .map(|h| {
                let n = tree.node(h.node);
                json!({ "path": tree.path(h.node).display().to_string(), "bytes": n.size, "size": size(n.size), "allocated": size(n.alloc),
                    "files": n.files, "modified": date(n.modified), "created": date(n.created) })
            })
            .collect();
    }
    if o.candidates {
        let list = candidates::detect(tree, &AoSessions::load(), now);
        v["candidates_total"] = json!(size(candidates::total_without_overlap(tree, &list)));
        v["candidates"] = list
            .iter()
            .map(|c| json!({ "path": tree.path(c.node).display().to_string(), "bytes": c.size, "size": size(c.size), "kind": c.kind, "reason": c.reason }))
            .collect();
    }
    if o.grew {
        v["grew"] = match snapshot::load(&snapshot::snapshot_file(&tree.root_path)) {
            None => json!({ "error": "no previous snapshot for this path (run once with --save-snapshot or scan it in the window)" }),
            Some(prev) => json!({ "since": date(prev.taken_at), "folders": snapshot::grew(tree, &prev, 50 << 20, o.top).iter().map(|g| json!({
                "path": tree.path(g.node).display().to_string(), "grew": size(g.new - g.old), "old": size(g.old), "new": size(g.new) })).collect::<Vec<_>>() }),
        };
    }
    v
}

fn print_text(v: &Value) {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    println!("{}  ({} on disk)  {} files, {} items  scanned in {:.1}s  [{} MB RAM]",
        s("size"), s("allocated"), count(v["files"].as_u64().unwrap_or(0)), count(v["items"].as_u64().unwrap_or(0)),
        v["elapsed_ms"].as_u64().unwrap_or(0) as f64 / 1000.0, v["memory_mb"]);
    println!("root {}   denied folders {}   reparse points not followed {}   method {} {}", s("root"), v["denied_folders"], v["reparse_points_skipped"], s("method"), s("method_note"));
    let list = |title: &str, arr: &Value| {
        let Some(a) = arr.as_array() else { return };
        println!("\n== {title} ({})", a.len());
        for x in a {
            let extra = [x["time"].as_str(), x["kind"].as_str(), x["reason"].as_str(), x["grew"].as_str().map(|_| "")]
                .into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ");
            let amount = x["size"].as_str().or(x["grew"].as_str()).unwrap_or("");
            println!("{amount:>10}  {}  {extra}", x["path"].as_str().unwrap_or(""));
        }
    };
    list("biggest folders (where the bytes are)", &v["top_folders"]);
    list("biggest files", &v["top_files"]);
    list("folders (by depth)", &v["folders"]);
    if let Some(a) = v["extensions"].as_array() {
        println!("\n== extensions");
        for x in a {
            println!("{:>10}  {:<12} {:>9} files  {}", x["size"].as_str().unwrap_or(""), x["ext"].as_str().unwrap_or(""), count(x["files"].as_u64().unwrap_or(0)), x["type"].as_str().unwrap_or(""));
        }
    }
    for (k, t) in [("recent", "recently created/modified"), ("least_used", "least used")] {
        if v[k].is_object() {
            list(&format!("{t} files"), &v[k]["files"]);
            list(&format!("{t} folders"), &v[k]["folders"]);
        }
    }
    list("cleanup candidates (suggestions only; nothing was deleted)", &v["candidates"]);
    if v["grew"].is_object() {
        if let Some(e) = v["grew"]["error"].as_str() { println!("\n== grew: {e}"); } else { list("grew since last snapshot", &v["grew"]["folders"]); }
    }
}
