//! Printable `report.html` (SPEC §11): the same facts as `report.txt`, with
//! the manual steps as a tick-box checklist. One self-contained file, no
//! scripts or external resources, so it opens from the stick on any PC.

use std::fmt::Write as _;

use super::failures::{self, FailLine};
use super::manifest::{FileStatus, Manifest, ManualStep};
use super::preflight::gib;
use super::report::{MAX_LISTED, per_folder};

const STYLE: &str = "body{font:15px/1.45 system-ui,Segoe UI,sans-serif;max-width:52rem;\
margin:2rem auto;padding:0 1rem;color:#1b1b1b}h1{font-size:1.5rem;margin:0 0 .2rem}\
h2{font-size:1.1rem;margin:1.6rem 0 .4rem;border-bottom:1px solid #ccc}\
table{border-collapse:collapse;width:100%}td,th{text-align:left;padding:.2rem .5rem;\
border-bottom:1px solid #eee;vertical-align:top}td.n{text-align:right;white-space:nowrap}\
.muted{color:#666}.bad{color:#a40000}ol.check{list-style:none;padding:0}\
ol.check li{margin:.5rem 0;padding-left:1.8rem;text-indent:-1.8rem}\
ol.check input{width:1.1rem;height:1.1rem;margin-right:.6rem;vertical-align:-.15rem}\
@media print{body{margin:0;max-width:none}input{-webkit-appearance:none;appearance:none;\
border:1.5px solid #000}}";

/// Escape text for HTML element content and attribute values.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// Manual steps as a tick-box list (also used for the restore checklist).
pub fn checklist(steps: &[ManualStep]) -> String {
    if steps.is_empty() {
        return "<p class=\"muted\">Nothing to do by hand.</p>\n".into();
    }
    let mut s = String::from("<ol class=\"check\">\n");
    for (i, st) in steps.iter().enumerate() {
        let _ = writeln!(
            s,
            "<li><label><input type=\"checkbox\"><b>{}. {}</b></label><br>\
             <span class=\"muted\">{}</span></li>",
            i + 1,
            esc(&st.title),
            esc(&st.detail)
        );
    }
    s.push_str("</ol>\n");
    s
}

pub fn html(m: &Manifest) -> String {
    let (ok, skipped, failed) = m.counts();
    let bytes: u64 = m
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Ok)
        .map(|f| f.size)
        .sum();
    let machine = m.source.machine.as_deref().unwrap_or("?");
    let mut s = String::new();
    let _ = write!(
        s,
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>Rebootly backup of {}</title><style>{STYLE}</style></head><body>\n",
        esc(machine)
    );
    let _ = writeln!(s, "<h1>Backup of {}</h1>", esc(machine));
    let _ = writeln!(
        s,
        "<p class=\"muted\">{} · job {} · started {} · {}</p>",
        esc(&m.source.os_name),
        esc(&m.job_id),
        esc(&m.started),
        match &m.finished {
            Some(f) => format!("finished {}", esc(f)),
            None => "<b class=\"bad\">not finished (resume it)</b>".into(),
        }
    );
    let _ = writeln!(
        s,
        "<p><b>{ok}</b> files copied ({}), {skipped} skipped, {}</p>",
        gib(bytes),
        if failed > 0 {
            format!("<b class=\"bad\">{failed} failed</b>")
        } else {
            "0 failed".into()
        }
    );

    let _ = writeln!(s, "<h2>Things to do by hand</h2>");
    s.push_str(&checklist(&m.manual_steps));

    let _ = writeln!(s, "<h2>What was copied</h2>\n<table>");
    let _ = writeln!(
        s,
        "<tr><th>Folder</th><th class=\"n\">Files</th><th class=\"n\">Size</th></tr>"
    );
    for (k, (n, b)) in per_folder(m) {
        let _ = writeln!(
            s,
            "<tr><td>{}</td><td class=\"n\">{n}</td><td class=\"n\">{}</td></tr>",
            esc(&k),
            gib(b)
        );
    }
    s.push_str("</table>\n");

    if failed > 0 {
        let lines = failures::lines(&m.files);
        let _ = writeln!(s, "<h2 class=\"bad\">Failed files ({failed})</h2>\n<table>");
        for l in lines.iter().take(MAX_LISTED) {
            let (what, why) = match l {
                FailLine::File { source, reason } => (esc(source), esc(reason)),
                FailLine::Group {
                    folder,
                    reason,
                    count,
                } => (
                    format!("{} <b>({count} files in this folder)</b>", esc(folder)),
                    esc(reason),
                ),
            };
            let _ = writeln!(s, "<tr><td>{what}</td><td class=\"muted\">{why}</td></tr>");
        }
        s.push_str("</table>\n");
        if lines.len() > MAX_LISTED {
            let _ = writeln!(
                s,
                "<p class=\"muted\">… and {} more line(s) (see manifest.json)</p>",
                lines.len() - MAX_LISTED
            );
        }
        for h in failures::hints(&m.files) {
            let _ = writeln!(s, "<p>{}</p>", esc(h));
        }
    }
    if let Some(sb) = &m.sensitive {
        let _ = writeln!(
            s,
            "<h2>Encrypted keys and credentials</h2><p>{} file(s) in <code>{}</code>. \
             The passphrase is not stored anywhere: keep it safe.</p>",
            sb.files.len(),
            esc(&sb.file)
        );
    }
    if !m.excluded.is_empty() {
        let _ = writeln!(s, "<h2>Left out on purpose</h2>\n<table>");
        for e in &m.excluded {
            let _ = writeln!(
                s,
                "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
                esc(e.description),
                e.count,
                gib(e.bytes)
            );
        }
        s.push_str("</table>\n");
    }
    if !m.warnings.is_empty() {
        let _ = writeln!(s, "<h2>Warnings</h2>\n<ul>");
        for w in &m.warnings {
            let _ = writeln!(s, "<li>{}</li>", esc(w));
        }
        s.push_str("</ul>\n");
    }
    s.push_str("</body></html>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_lists_steps() {
        let steps = vec![ManualStep {
            title: "Sign in to <Outlook>".into(),
            detail: "Use \"Add account\" & wait".into(),
        }];
        let c = checklist(&steps);
        assert!(c.contains("type=\"checkbox\""));
        assert!(c.contains("Sign in to &lt;Outlook&gt;"));
        assert!(c.contains("&quot;Add account&quot; &amp; wait"));
        assert!(checklist(&[]).contains("Nothing to do"));
    }
}
