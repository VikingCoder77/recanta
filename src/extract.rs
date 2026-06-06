//! Text extraction for binary document formats (PRD: general AIOS memory). PDF via
//! `pdf-extract`; Word `.docx` by reading `word/document.xml` from the zip and stripping
//! tags. Both are best-effort: scanned PDFs and exotic layouts may yield little text.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

/// Extract text from a PDF.
pub fn pdf(path: &Path) -> Result<String> {
    pdf_extract::extract_text(path)
        .with_context(|| format!("extracting text from PDF {}", path.display()))
}

/// Extract text from a `.docx` (Office Open XML): unzip `word/document.xml` and pull the
/// text out of `<w:t>` runs, inserting breaks at paragraph boundaries.
pub fn docx(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("reading {} as docx (zip)", path.display()))?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .context("docx missing word/document.xml")?
        .read_to_string(&mut xml)
        .context("reading word/document.xml")?;
    Ok(strip_docx_xml(&xml))
}

/// Pull readable text out of WordprocessingML: `<w:t>…</w:t>` are text runs, `</w:p>`
/// ends a paragraph, `<w:tab/>`/`<w:br/>` are whitespace.
fn strip_docx_xml(xml: &str) -> String {
    let mut out = String::new();
    let bytes = xml.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(close) = xml[i..].find('>') else { break };
            let tag = &xml[i + 1..i + close];
            if tag == "w:p" || tag.starts_with("w:p ") || tag == "/w:p" {
                out.push('\n');
            } else if tag == "w:tab" || tag == "w:tab/" || tag.starts_with("w:tab") {
                out.push('\t');
            } else if tag.starts_with("w:br") {
                out.push('\n');
            }
            i += close + 1;
        } else {
            // Text content between tags.
            let Some(next) = xml[i..].find('<') else {
                out.push_str(&xml[i..]);
                break;
            };
            out.push_str(unescape(&xml[i..i + next]).as_str());
            i += next;
        }
    }
    // Collapse the runs of blank lines docx tends to produce.
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_wordml_to_text() {
        let xml = "<w:document><w:body><w:p><w:r><w:t>Hello</w:t></w:r>\
                   <w:r><w:t> world</w:t></w:r></w:p><w:p><w:r><w:t>Line two &amp; more</w:t>\
                   </w:r></w:p></w:body></w:document>";
        let text = strip_docx_xml(xml);
        assert!(text.contains("Hello world"));
        assert!(text.contains("Line two & more"));
    }
}
