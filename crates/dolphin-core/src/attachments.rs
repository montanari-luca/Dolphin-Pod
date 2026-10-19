//! Attachments: files are copied into the app data folder; images go to vision models, text and PDFs
//! are inlined as text.

use std::path::Path;

use base64::Engine;

use crate::core::Core;
use crate::error::{CoreError, Result};
use crate::model::{Attachment, AttachmentKind};
use crate::providers::Part;
use crate::util::{new_id, truncate};

pub const MAX_BYTES: usize = 25 * 1024 * 1024;
const MAX_INLINE_CHARS: usize = 150_000;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

const TEXT_EXT: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "jsonl", "yaml", "yml", "toml", "xml", "html", "htm", "css", "scss",
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "py", "rb", "go", "rs", "java", "kt", "kts", "swift", "c", "h", "cpp", "hpp",
    "cc", "cs", "php", "sh", "bash", "zsh", "fish", "ps1", "sql", "graphql", "proto", "ini", "cfg", "conf", "env", "log",
    "tex", "r", "lua", "dart", "scala", "ex", "exs", "erl", "hs", "ml", "vue", "svelte", "astro", "gradle", "dockerfile",
    "makefile", "lock", "gitignore",
];

pub fn classify(name: &str, mime: &str) -> AttachmentKind {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or(name).to_lowercase();
    if mime.starts_with("image/") && mime != "image/svg+xml" {
        AttachmentKind::Image
    } else if mime == "application/pdf" || ext == "pdf" {
        AttachmentKind::Pdf
    } else if mime.starts_with("text/") || mime.contains("json") || mime.contains("xml") || TEXT_EXT.contains(&ext.as_str()) {
        AttachmentKind::Text
    } else {
        AttachmentKind::Other
    }
}

fn safe_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    let n = n.trim_matches('.').to_string();
    if n.is_empty() { "file".into() } else { truncate(&n, 80).trim_end_matches('…').to_string() }
}

impl Core {
    /// Store bytes (paste, drop, or a generated image) and register a staged attachment.
    pub fn stage_attachment(&self, name: &str, mime: &str, bytes: &[u8]) -> Result<Attachment> {
        if bytes.len() > MAX_BYTES {
            return Err(CoreError::invalid(format!("{name} is larger than 25 MB")));
        }
        let id = new_id();
        let mime = if mime.is_empty() { mime_guess::from_path(name).first_or_octet_stream().to_string() } else { mime.to_string() };
        let path = self.attachments_dir().join(format!("{id}-{}", safe_name(name)));
        std::fs::write(&path, bytes)?;
        let a = Attachment {
            id,
            name: name.to_string(),
            kind: classify(name, &mime),
            mime,
            size: bytes.len() as i64,
            path: path.to_string_lossy().into_owned(),
        };
        self.db.insert_attachment(&a, None)?;
        Ok(a)
    }

    pub fn stage_attachment_from_path(&self, path: &str) -> Result<Attachment> {
        let p = Path::new(path);
        let meta = std::fs::metadata(p)?;
        if !meta.is_file() {
            return Err(CoreError::invalid("only files can be attached"));
        }
        if meta.len() as usize > MAX_BYTES {
            return Err(CoreError::invalid("files larger than 25 MB cannot be attached"));
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        let mime = mime_guess::from_path(p).first_or_octet_stream().to_string();
        self.stage_attachment(&name, &mime, &std::fs::read(p)?)
    }
}

/// Turn an attachment into content parts for a model.
pub fn to_parts(a: &Attachment, vision: bool) -> Vec<Part> {
    let note = |s: String| vec![Part::Text(s)];
    match a.kind {
        AttachmentKind::Image if vision => match std::fs::read(&a.path) {
            Ok(b) if b.len() <= MAX_IMAGE_BYTES => vec![
                Part::Text(format!("[image attached: {}]", a.name)),
                Part::Image { mime: a.mime.clone(), data_b64: base64::engine::general_purpose::STANDARD.encode(b) },
            ],
            Ok(_) => note(format!("[image {} is too large to send]", a.name)),
            Err(_) => note(format!("[image {} is no longer available]", a.name)),
        },
        AttachmentKind::Image => note(format!("[image attached: {} — this model cannot see images]", a.name)),
        AttachmentKind::Text => match std::fs::read(&a.path) {
            Ok(b) => note(format!("<attachment name=\"{}\">\n{}\n</attachment>", a.name, truncate(&String::from_utf8_lossy(&b), MAX_INLINE_CHARS))),
            Err(_) => note(format!("[{} is no longer available]", a.name)),
        },
        AttachmentKind::Pdf => match std::fs::read(&a.path).ok().and_then(|b| pdf_extract::extract_text_from_mem(&b).ok()) {
            Some(text) if !text.trim().is_empty() => note(format!("<attachment name=\"{}\" type=\"pdf\">\n{}\n</attachment>", a.name, truncate(text.trim(), MAX_INLINE_CHARS))),
            _ => note(format!("[PDF attached: {} — no extractable text]", a.name)),
        },
        AttachmentKind::Other => note(format!("[file attached: {} ({}, {} bytes) — not readable as text]", a.name, a.mime, a.size)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_mime_and_extension() {
        assert_eq!(classify("a.png", "image/png"), AttachmentKind::Image);
        assert_eq!(classify("main.rs", "application/octet-stream"), AttachmentKind::Text);
        assert_eq!(classify("paste.txt", "text/plain"), AttachmentKind::Text);
        assert_eq!(classify("doc.pdf", ""), AttachmentKind::Pdf);
        assert_eq!(classify("x.zip", "application/zip"), AttachmentKind::Other);
        assert_eq!(safe_name("../../etc/passwd"), "_.._etc_passwd");
    }
}
