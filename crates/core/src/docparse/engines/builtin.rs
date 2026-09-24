//! Builtin engine — the anydoc pipeline as one engine (E1): markdown
//! passthrough / anydoc conversion / scanned-PDF placeholder degradation,
//! plus embedded-asset extraction for non-PDF document formats. Behavior
//! is identical to the pre-registry pipeline (zero-regression E1
//! acceptance).

use std::sync::OnceLock;

use super::{ParseOpts, ParseOutcome, ParsedImage};
use crate::errors::app_error::{AppError, AppResult};

/// Default upper bound on concurrently running builtin parses. The parsers are
/// CPU- and memory-heavy (a single scanned PDF can OOM a container —
/// `dev-docs/document/service-design.md` §), so bounding matters more than
/// throughput; raise only alongside the machine's memory budget.
const DEFAULT_PARSE_CONCURRENCY: usize = 2;

/// Configured parse-concurrency override, set once at startup from
/// `RAISFAST_KB_PARSER_CONCURRENCY` (see [`init_parse_concurrency`]).
static PARSE_LIMIT: OnceLock<usize> = OnceLock::new();

/// Overrides the builtin parse concurrency. Call once at startup, before
/// workers start; later calls are ignored (first wins). The semaphore is
/// created lazily on first use, so the value is picked up as long as this runs
/// during startup.
pub fn init_parse_concurrency(limit: usize) {
    let _ = PARSE_LIMIT.set(limit.max(1));
}

/// Process-wide parse gate: bounds concurrent blocking parses across every
/// caller (KB ingest, doc conversion, flow nodes).
fn parse_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEM.get_or_init(|| {
        let limit = PARSE_LIMIT
            .get()
            .copied()
            .unwrap_or(DEFAULT_PARSE_CONCURRENCY);
        tokio::sync::Semaphore::new(limit)
    })
}

pub struct BuiltinEngine;

#[async_trait::async_trait]
impl super::ParseEngine for BuiltinEngine {
    fn name(&self) -> &'static str {
        "builtin"
    }

    fn supports(&self, mime: &str, filename: &str) -> bool {
        // The builtin takes everything except pure image files (no
        // rasterization ability — image-as-document needs a service engine;
        // kb-parser-engines-design §0).
        !(mime.starts_with("image/") || looks_like_image_ext(filename))
    }

    async fn probe(&self) -> bool {
        true // in-process, always available
    }

    async fn parse(
        &self,
        bytes: &[u8],
        mime: &str,
        filename: &str,
        opts: &ParseOpts,
    ) -> AppResult<ParseOutcome> {
        // Bound concurrent parses (each is CPU- and memory-heavy).
        let _permit = parse_semaphore()
            .acquire()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("parse gate closed: {e}")))?;

        // Preferred: parse in a child process so a hung/cancelled parse can be
        // hard-killed (`kill_on_drop` + the job cancel token). Tests and
        // unusually-named binaries fall back to the in-process path.
        if crate::compute::subprocess_supported() {
            let params = serde_json::json!({
                "mime": mime,
                "filename": filename,
                "extract_images": opts.extract_images,
            });
            match crate::compute::run::<ParseOutcome>("parse_builtin", bytes, &params).await {
                Ok(outcome) => return Ok(outcome),
                Err(crate::compute::ComputeError::Cancelled) => {
                    return Err(AppError::BadRequest("document parse cancelled".into()));
                }
                Err(crate::compute::ComputeError::Rejected(msg)) => {
                    // A real parse failure — do not retry in-process.
                    return Err(AppError::BadRequest(format!(
                        "document parse failed: {msg}"
                    )));
                }
                Err(crate::compute::ComputeError::Unavailable(e)) => {
                    tracing::warn!("parse subprocess unavailable ({e}); falling back in-process");
                }
            }
        }

        // Fallback: synchronous parse on the blocking pool (won't stall the
        // async runtime, but cannot be hard-killed).
        let bytes = bytes.to_vec();
        let mime = mime.to_string();
        let filename = filename.to_string();
        let opts = *opts;
        tokio::task::spawn_blocking(move || parse_builtin(&bytes, &mime, &filename, &opts))
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("parse task join error: {e}")))?
    }
}

fn looks_like_image_ext(filename: &str) -> bool {
    filename.rsplit('.').next().is_some_and(|ext| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif"
        )
    })
}

/// The pre-registry parse pipeline, verbatim (service.rs `parse_document` +
/// the embedded-asset pass previously done at registration time). Asset
/// images get synthetic ref names — the anydoc String API renders them as
/// alt text only, so they stay doc-level (no chunk association); engine
/// paths keep real refs [抄WK:collectAssets 命名形态, 位置保留=等 anydoc
/// 上游开放项].
pub fn parse_builtin(
    bytes: &[u8],
    mime: &str,
    filename: &str,
    opts: &ParseOpts,
) -> AppResult<ParseOutcome> {
    let is_markdown = mime == "text/markdown" || mime == "text/plain" || filename.ends_with(".md");
    if is_markdown {
        let md = String::from_utf8(bytes.to_vec())
            .map_err(|e| AppError::BadRequest(format!("invalid utf-8 markdown: {e}")))?;
        return Ok(ParseOutcome {
            markdown: md,
            engine: "builtin".into(),
            ..Default::default()
        });
    }
    match anydoc::to_markdown_bytes(bytes, None) {
        Ok(md) => Ok(ParseOutcome {
            markdown: md,
            images: embedded_assets(bytes, opts.extract_images),
            engine: "builtin".into(),
            ..Default::default()
        }),
        Err(anydoc::ConvertError::NeedsOcr { pages, page_count }) => {
            let markdown = extract_pdf_skip_ocr(bytes, &pages, page_count)?;
            Ok(ParseOutcome {
                markdown,
                engine: "builtin".into(),
                pages: Some(page_count),
                scanned_pages: pages,
                ..Default::default()
            })
        }
        Err(e) => Err(AppError::BadRequest(format!("document parse failed: {e}"))),
    }
}

/// Embedded image assets of non-PDF document formats via anydoc's
/// Document API (PDF has no document model — declared limitation).
fn embedded_assets(bytes: &[u8], extract: bool) -> Vec<ParsedImage> {
    if !extract {
        return Vec::new();
    }
    match anydoc::to_document(bytes, None) {
        Ok(doc) => doc
            .assets
            .into_iter()
            .enumerate()
            .filter(|(_, a)| a.media_type.starts_with("image/") && !a.bytes.is_empty())
            .map(|(i, a)| {
                let ext = a.media_type.rsplit('/').next().unwrap_or("bin");
                ParsedImage {
                    ref_name: format!("images/image-{}.{ext}", i + 1),
                    mime_type: a.media_type,
                    bytes: a.bytes,
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Degraded PDF parse: keep text pages, insert placeholders for scanned
/// pages. Fails only when nothing extractable remains（自 kb/service.rs 迁入
/// ——通用解析能力归 docparse 底座）.
pub fn extract_pdf_skip_ocr(bytes: &[u8], ocr_pages: &[u32], page_count: u32) -> AppResult<String> {
    let extracted = pdf_inspector::extract_pages_markdown_mem(bytes, None)
        .map_err(|e| AppError::BadRequest(format!("document parse failed: {e}")))?;
    let mut md = String::new();
    for page in &extracted.pages {
        if page.needs_ocr {
            let reason = page.ocr_reason.as_deref().unwrap_or("scanned page");
            md.push_str(&format!(
                "\n\n> [第 {} 页为扫描件（{}），需要 OCR，已跳过]\n",
                page.page + 1,
                reason
            ));
        } else {
            md.push_str(&page.markdown);
        }
    }
    if md.trim().is_empty() {
        return Err(AppError::BadRequest(format!(
            "document parse failed: pages {ocr_pages:?} of {page_count} need OCR and no extractable text remains"
        )));
    }
    tracing::warn!(
        "[docparse] pdf degraded parse: {} of {} pages need OCR (skipped: {ocr_pages:?}), placeholders inserted",
        ocr_pages.len(),
        page_count
    );
    Ok(md)
}
