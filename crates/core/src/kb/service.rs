//! KB service — document ingestion orchestration (kb-technical-design §5).
//!
//! Pipeline: upload/online → storage → `KbDocumentCreated` event →
//! `KbProcessDocument` job → anydoc parse → chunk (§3) → embed →
//! vector upsert + KB tantivy index → `KbDocumentReady`. Re-parse is
//! idempotent: delete-then-insert by doc in all three stores (SQL chunks,
//! vector backend, tantivy).

use std::sync::Arc;

use crate::config::app::AppConfig;
use crate::db::DbDriver as _;
use crate::errors::app_error::{AppError, AppResult};
use crate::event::Event;
use crate::event::EventEmitter;
use crate::kb::chunker::{self, ChunkerConfig};
use crate::kb::kbsearch::{KbIndexUnit, KbSearchEngine};
use crate::kb::models::{chunk, document, faq, knowledge_base, wiki_page, wiki_source};
use crate::kb::vectors::{VectorIndex, VectorItem};
use crate::storage::Storage;
use crate::types::snowflake_id::SnowflakeId;

/// Embedding seam: production wraps the OpenAI-compatible provider
/// (`/embeddings`, §4.1); tests inject a deterministic stub.
#[async_trait::async_trait]
pub trait KbEmbedder: Send + Sync {
    async fn embed(&self, tenant: &str, texts: &[&str]) -> AppResult<Vec<Vec<f32>>>;

    /// Per-KB embedding: the KB row pins its model + dim at creation
    /// (immutable). Production uses the KB's model; tests fall back to
    /// [`KbEmbedder::embed`]. `tenant` scopes gateway routing (§10.2).
    async fn embed_for(
        &self,
        tenant: &str,
        _model: &str,
        dim: u32,
        texts: &[&str],
    ) -> AppResult<Vec<Vec<f32>>> {
        let out = self.embed(tenant, texts).await?;
        for v in &out {
            if v.len() as u32 != dim {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "embedding dim mismatch: kb pins {dim}, model returned {}",
                    v.len()
                )));
            }
        }
        Ok(out)
    }
}

/// Placeholder provider for tests where the chat path is never exercised
/// (required because `KbDeps.router` is mandatory, §10.2).
pub struct NoopProvider;

#[async_trait::async_trait]
impl raisfast_agent::ModelProvider for NoopProvider {
    fn name(&self) -> &str {
        "noop"
    }

    async fn chat(
        &self,
        _request: &raisfast_agent::provider::ChatRequest<'_>,
        _model: &str,
    ) -> Result<raisfast_agent::provider::ChatResponse, raisfast_agent::ProviderError> {
        Err(raisfast_agent::ProviderError::Config(
            "noop provider must not be called".to_owned(),
        ))
    }
}

/// Production embedder over the llm 底座 facade [§10.2] — the ONLY entry.
pub struct ProviderEmbedder {
    router: Arc<crate::llm::service::LlmRouter>,
    /// Texts per `/embeddings` request.
    batch_size: usize,
}

/// Retry shape [抄WK:keywords_vector_hybrid_indexer.go batchEmbedWithBackoff]:
/// 5 attempts, exponential backoff 200ms → 3.2s.
const EMBED_RETRY_ATTEMPTS: usize = 5;
const EMBED_RETRY_BASE_DELAY_MS: u64 = 200;

impl ProviderEmbedder {
    pub fn new(router: Arc<crate::llm::service::LlmRouter>, batch_size: usize) -> Self {
        Self { router, batch_size }
    }

    /// Slice texts into fixed-size batches and embed them sequentially,
    /// retrying each batch with exponential backoff on failure
    /// [抄WK:models/embedding/batch.go BatchEmbedWithPool 语义，串行替代协程池].
    async fn embed_batched(
        &self,
        tenant: &str,
        texts: &[&str],
        model: &str,
    ) -> AppResult<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(self.batch_size.max(1)) {
            let mut delay = EMBED_RETRY_BASE_DELAY_MS;
            let mut last_err: Option<String> = None;
            let mut vectors: Option<Vec<Vec<f32>>> = None;
            for attempt in 0..EMBED_RETRY_ATTEMPTS {
                match self.embed_once(tenant, batch, model).await {
                    Ok(v) if v.len() == batch.len() => {
                        vectors = Some(v);
                        break;
                    }
                    Ok(v) => {
                        last_err = Some(format!(
                            "embedding count mismatch: batch {} got {}",
                            batch.len(),
                            v.len()
                        ));
                    }
                    Err(e) => {
                        last_err = Some(e.to_string());
                    }
                }
                tracing::warn!(
                    "embedding batch ({}/{} texts) attempt {}/{} failed: {}",
                    batch.len(),
                    texts.len(),
                    attempt + 1,
                    EMBED_RETRY_ATTEMPTS,
                    last_err.as_deref().unwrap_or_default()
                );
                if attempt + 1 < EMBED_RETRY_ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    delay *= 2;
                }
            }
            match vectors {
                Some(v) => out.extend(v),
                None => {
                    return Err(AppError::ServiceUnavailable(format!(
                        "embedding: {}",
                        last_err.unwrap_or_else(|| "unknown error".into())
                    )));
                }
            }
        }
        Ok(out)
    }

    /// 单批嵌入：唯一入口 llm 底座（路由/failover/计费/日志，§10.2）。
    async fn embed_once(
        &self,
        tenant: &str,
        batch: &[&str],
        model: &str,
    ) -> Result<Vec<Vec<f32>>, String> {
        let effective = (!model.is_empty()).then_some(model);
        self.router
            .call(tenant, crate::llm::models::log::LogSource::Kb)
            .embed(effective, batch)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl KbEmbedder for ProviderEmbedder {
    async fn embed_for(
        &self,
        tenant: &str,
        model: &str,
        dim: u32,
        texts: &[&str],
    ) -> AppResult<Vec<Vec<f32>>> {
        let out = self.embed_batched(tenant, texts, model).await?;
        for v in &out {
            if v.len() as u32 != dim {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "embedding dim mismatch: kb pins {dim}, model '{model}' returned {}",
                    v.len()
                )));
            }
        }
        Ok(out)
    }

    async fn embed(&self, tenant: &str, texts: &[&str]) -> AppResult<Vec<Vec<f32>>> {
        // 空模型名 → facade 解析租户默认 embedding 模型（§10.2）。
        self.embed_batched(tenant, texts, "").await
    }
}

/// KB 单次 chat（S1 理解 / S9 生成 / 蒸馏）：唯一入口是 llm 底座
/// facade（模型解析/路由/failover/计费/日志全在内核，§10.2）。
pub async fn kb_chat(
    deps: &KbDeps,
    tenant: &str,
    request: &raisfast_agent::provider::ChatRequest<'_>,
) -> AppResult<String> {
    kb_chat_model(deps, tenant, None, request).await
}

/// Model-pinned variant: S1 understand routes to a dedicated fast model
/// when `RAISFAST_KB_UNDERSTAND_MODEL` is configured（改写+关键词抽取是小
/// 任务，推荐非推理小模型——S1 在每次 ask 的关键路径上）；`None` resolves
/// the tenant default chat model.
pub async fn kb_chat_model(
    deps: &KbDeps,
    tenant: &str,
    model: Option<&str>,
    request: &raisfast_agent::provider::ChatRequest<'_>,
) -> AppResult<String> {
    let resp = deps
        .router
        .call(tenant, crate::llm::models::log::LogSource::Kb)
        .chat(model, request)
        .await?;
    Ok(resp.text.unwrap_or_default())
}

/// Wired dependencies for service functions (built once at startup).
pub struct KbDeps {
    pub pool: crate::db::Pool,
    pub config: Arc<AppConfig>,
    pub storage: Arc<dyn Storage>,
    pub vector: Arc<dyn VectorIndex>,
    pub kbsearch: Arc<KbSearchEngine>,
    pub embedder: Arc<dyn KbEmbedder>,
    /// S5 reranker; `None` = rerank off (§6.1).
    pub reranker: Option<Arc<dyn crate::kb::rerank::KbReranker>>,
    /// Parser engine registry (builtin always; service engines as
    /// configured) — kb-parser-engines-design §2.
    pub parsers: Arc<crate::docparse::ParserRegistry>,
    /// LLM 底座（chat + embedding 唯一入口，§10.2）。
    pub router: Arc<crate::llm::service::LlmRouter>,
    pub emitter: EventEmitter,
}

/// MIME whitelist for KB uploads — extends the media allowlist with
/// document formats [抄RF:services/media.rs save_file 校验流程].
pub const KB_ALLOWED_MIME: &[&str] = &[
    "text/markdown",
    "text/plain",
    "text/html",
    "text/csv",
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/epub+zip",
    "application/json",
    "application/rtf",
    "application/vnd.oasis.opendocument.text",
    // Image-as-document (kb-parser-engines-design §0): builtin cannot
    // parse these — a configured service engine (docreader) takes them;
    // upload pre-check rejects with a clear error when none is available.
    "image/png",
    "image/jpeg",
    "image/webp",
];

const MAX_DOC_BYTES: usize = 50 * 1024 * 1024;

/// Uploaded-file entry point: validate → store → insert pending row → event.
pub async fn upload_document(
    deps: &KbDeps,
    kb_id: SnowflakeId,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    user_id: Option<SnowflakeId>,
    tenant_id: &str,
) -> AppResult<document::KbDocument> {
    ensure_kb(deps, kb_id, tenant_id).await?;
    let mime = normalize_mime(mime_type, filename);
    if !KB_ALLOWED_MIME.contains(&mime.as_str()) {
        return Err(AppError::BadRequest(format!(
            "unsupported kb document type: {mime}"
        )));
    }
    // Type the builtin cannot parse (image-as-document): reject at upload
    // with a clear error unless a service engine is routable — never
    // accept-and-fail-silently (kb-parser-engines-design §0).
    let builtin = crate::docparse::builtin::BuiltinEngine;
    if !crate::docparse::ParseEngine::supports(&builtin, &mime, filename) {
        let kb = knowledge_base::find_kb_by_id(&deps.pool, kb_id, tenant_id).await?;
        let routable = match kb {
            Some(kb) => deps
                .parsers
                .route(
                    deps.config.kb.parser_engine.as_deref(),
                    kb.parser_config.as_ref().and_then(|v| {
                        serde_json::from_value::<crate::docparse::ParserConfig>(v.clone()).ok()
                    }),
                    None,
                    &mime,
                    filename,
                )
                .await
                .map(|(e, _)| e.supports(&mime, filename))
                .unwrap_or(false),
            None => false,
        };
        if !routable {
            return Err(AppError::BadRequest(format!(
                "文件类型 {mime} 需要解析引擎（如 docreader）：请在服务端配置                  RAISFAST_KB_DOCREADER_URL 并将 KB 的 parser_config 规则指向它"
            )));
        }
    }
    if data.is_empty() {
        return Err(AppError::BadRequest("empty file".into()));
    }
    if data.len() > MAX_DOC_BYTES {
        return Err(AppError::PayloadTooLarge("document exceeds 50MB".into()));
    }
    let ext = filename.rsplit('.').next().unwrap_or_default();
    let key = crate::services::media::storage_key("kb", ext);
    deps.storage.put(&key, data, &mime).await?;

    let doc = document::create_document(
        &deps.pool,
        &document::CreateKbDocumentCmd {
            kb_id,
            title: trim_filename(filename),
            source: "upload".into(),
            storage_key: Some(key),
            mime_type: Some(mime),
            size: data.len() as i64,
            created_by: user_id,
        },
        tenant_id,
    )
    .await?;
    deps.emitter.emit(Event::KbDocumentCreated(doc.clone()));
    Ok(doc)
}

/// Online markdown entry point (D7): same pipeline, parse step skipped by
/// treating the stored bytes as markdown.
pub async fn create_online_document(
    deps: &KbDeps,
    kb_id: SnowflakeId,
    title: &str,
    markdown: &str,
    user_id: Option<SnowflakeId>,
    tenant_id: &str,
) -> AppResult<document::KbDocument> {
    ensure_kb(deps, kb_id, tenant_id).await?;
    if markdown.trim().is_empty() {
        return Err(AppError::BadRequest("empty markdown".into()));
    }
    let data = markdown.as_bytes();
    let key = crate::services::media::storage_key("kb", "md");
    deps.storage.put(&key, data, "text/markdown").await?;

    let doc = document::create_document(
        &deps.pool,
        &document::CreateKbDocumentCmd {
            kb_id,
            title: title.to_string(),
            source: "online".into(),
            storage_key: Some(key),
            mime_type: Some("text/markdown".into()),
            size: data.len() as i64,
            created_by: user_id,
        },
        tenant_id,
    )
    .await?;
    deps.emitter.emit(Event::KbDocumentCreated(doc.clone()));
    Ok(doc)
}

/// The `KbProcessDocument` job body — the whole ingest pipeline.
/// Idempotent: safe to re-run at any failure point (delete-then-insert).
pub async fn process_document(
    deps: &KbDeps,
    doc_id: SnowflakeId,
    tenant_id: &str,
) -> AppResult<()> {
    process_document_traced(
        deps,
        doc_id,
        tenant_id,
        &mut crate::kb::trace::RunRecorder::disabled(),
    )
    .await
}

/// [kb-observability-design T1] `process_document` with a run recorder —
/// the caller (job handler / reparse) owns the recorder so it can attach
/// exact attempt/job_id provenance (DR6); the pipeline body brackets each
/// stage (parse/chunk/embed/vector/bm25) into it (DR6 zero-invasion: only
/// the orchestration function is touched).
pub async fn process_document_traced(
    deps: &KbDeps,
    doc_id: SnowflakeId,
    tenant_id: &str,
    trace: &mut crate::kb::trace::RunRecorder,
) -> AppResult<()> {
    let result = process_document_inner(deps, doc_id, tenant_id, trace).await;
    match &result {
        Ok(()) => {
            trace
                .finish(&deps.pool, crate::kb::models::kb_run::STATUS_OK, None)
                .await;
        }
        Err(e) => {
            trace.fail_stage(&e.to_string()); // close any open stage
            trace
                .finish(
                    &deps.pool,
                    crate::kb::models::kb_run::STATUS_FAILED,
                    Some(&e.to_string()),
                )
                .await;
        }
    }
    result
}

async fn process_document_inner(
    deps: &KbDeps,
    doc_id: SnowflakeId,
    tenant_id: &str,
    trace: &mut crate::kb::trace::RunRecorder,
) -> AppResult<()> {
    let Some(doc) = document::find_document_by_id(&deps.pool, doc_id, tenant_id).await? else {
        return Ok(()); // deleted meanwhile — nothing to do
    };
    let Some(kb) = knowledge_base::find_kb_by_id(&deps.pool, doc.kb_id, tenant_id).await? else {
        return Ok(());
    };
    // Per-KB pinned model/dim (immutable since creation, D6-revised).
    let model = kb
        .embedding_model
        .clone()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| {
            AppError::BadRequest("该知识库未配置嵌入模型（创建时必填，之后不可修改）".into())
        })?;
    let dim = u32::try_from(kb.embedding_dim.unwrap_or(0)).unwrap_or(0);
    if dim == 0 {
        return Err(AppError::BadRequest(
            "该知识库未配置向量维度（创建时必填，之后不可修改）".into(),
        ));
    }
    // First persistence hop for the trace run row (DR2 long task).
    trace.set_kb(kb.id);
    trace.begin(&deps.pool, &deps.config).await;

    // ① parse → markdown via the engine registry (kb-parser-engines §2).
    trace.stage("parse");
    document::set_document_status(&deps.pool, doc_id, "parsing", None, None, tenant_id).await?;
    // Image recognition gate: resolved before parse so disabled KBs skip the
    // asset pass entirely (kb-image-recognition-design §3 D2).
    let image_cfg = crate::kb::images::resolve_image_config(deps, &kb);
    let mime = doc.mime_type.as_deref().unwrap_or_default();
    let filename = doc.storage_key.as_deref().unwrap_or_default();
    let (engine, route_warnings) = deps
        .parsers
        .route(
            deps.config.kb.parser_engine.as_deref(),
            kb.parser_config.as_ref().and_then(|v| {
                serde_json::from_value::<crate::docparse::ParserConfig>(v.clone()).ok()
            }),
            doc.parser_engine.as_deref(),
            mime,
            filename,
        )
        .await?;
    let (markdown, parsed) = match &doc.storage_key {
        Some(key) => {
            let bytes = deps.storage.get(key).await?;
            if bytes.is_empty() {
                return Err(AppError::BadRequest(
                    "document bytes missing in storage".into(),
                ));
            }
            let mut parsed = engine
                .parse(
                    &bytes,
                    mime,
                    filename,
                    &crate::docparse::ParseOpts {
                        extract_images: image_cfg.is_some(),
                    },
                )
                .await
                .map_err(|e| {
                    // Engine runtime error fails the document loudly — never
                    // silently degrade to builtin (§2 D2 failure semantics).
                    AppError::BadRequest(format!("parse engine '{}': {e}", engine.name()))
                })?;
            parsed.engine = engine.name().to_string();
            let markdown = parsed.markdown.clone();
            (markdown, parsed)
        }
        None => return Err(AppError::BadRequest("document has no storage key".into())),
    };
    // QA gate (kb-parser-engines-design §3.2): empty/corrupted content never
    // enters the index; partial degradation becomes a first-class marker.
    let gate_warnings = match crate::kb::parse_quality::gate(&parsed) {
        Ok(mut warnings) => {
            warnings.extend(route_warnings.iter().cloned());
            warnings
        }
        Err(e) => {
            document::set_document_status(
                &deps.pool,
                doc_id,
                "failed",
                Some(&e.to_string()),
                None,
                tenant_id,
            )
            .await?;
            trace.end_stage(
                crate::kb::trace::STAGE_FAILED,
                serde_json::json!({
                    "engine": parsed.engine,
                    "effective_chars": crate::kb::parse_quality::effective_chars(&markdown),
                    "gate": "rejected",
                }),
                Some(&e.to_string()),
            );
            return Err(e);
        }
    };
    let degraded = crate::kb::parse_quality::degraded_marker(&parsed);
    document::set_document_degraded(&deps.pool, doc_id, degraded.as_deref(), tenant_id).await?;
    document::set_document_pages(&deps.pool, doc_id, parsed.pages, tenant_id).await?;
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({
            "engine": parsed.engine,
            "bytes": doc.size,
            "mime": doc.mime_type,
            "chars": markdown.chars().count(),
            "effective_chars": crate::kb::parse_quality::effective_chars(&markdown),
            "pages": parsed.pages,
            "scanned_pages": parsed.scanned_pages.len(),
            "gate": if gate_warnings.is_empty() { "passed" } else { "degraded" },
            "warnings": gate_warnings,
        }),
        None,
    );

    // ② chunk (§3) + ③ persist chunks
    trace.stage("chunk");
    document::set_document_status(&deps.pool, doc_id, "chunking", None, None, tenant_id).await?;
    // idempotency: wipe this doc's chunks + vector units + fts units first.
    // Vector purge is doc-scoped (payload/filter delete): the old generation's
    // unit ids die with their rows, so an id-list delete here would leak the
    // previous parse's vectors into the index (orphan-ghost recall).
    chunk::delete_chunks_by_doc(&deps.pool, doc_id).await?;
    deps.vector
        .delete_document(i64::from(doc.kb_id), i64::from(doc_id))
        .await?;
    // fts cleanup is best-effort: the index is a rebuildable acceleration
    // layer, and a missing/broken index dir (e.g. wiped while the server
    // runs) must not fail the delete — the units are gone with the doc.
    if let Err(e) = deps.kbsearch.delete_document(i64::from(doc_id)).await {
        tracing::warn!(error = %e, doc = %doc_id, "fts cleanup failed — index is rebuildable, ignoring");
    }
    crate::kb::models::image::delete_images_by_doc(&deps.pool, doc_id).await?;

    let cfg = ChunkerConfig::default();
    // 页锚点行替换为等长空白（字节偏移不变 → 页映射/图片引用映射对齐），
    // 锚点不进入检索文本；纯锚点段产出空块 → 跳过不入库。
    // 页锚点偏移必须在空白化【之前】取（等长替换，偏移对齐原文）。
    let marks = crate::kb::parse_quality::page_marks(&markdown);
    let markdown = crate::kb::parse_quality::blank_page_marks(&markdown);
    // text-splitter chunking is CPU-bound; run it out-of-process when possible
    // (hard-killable) and otherwise on the blocking pool.
    let via_subprocess = if crate::compute::subprocess_supported() {
        let params = serde_json::to_value(cfg)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("chunk params: {e}")))?;
        match crate::compute::run::<Vec<chunker::Chunk>>(
            "chunk_markdown",
            markdown.as_bytes(),
            &params,
        )
        .await
        {
            Ok(chunks) => Some(chunks),
            Err(crate::compute::ComputeError::Cancelled) => {
                return Err(AppError::BadRequest("document chunking cancelled".into()));
            }
            Err(crate::compute::ComputeError::Rejected(msg)) => {
                return Err(AppError::BadRequest(format!(
                    "document chunking failed: {msg}"
                )));
            }
            Err(crate::compute::ComputeError::Unavailable(e)) => {
                tracing::warn!("chunk subprocess unavailable ({e}); falling back in-process");
                None
            }
        }
    } else {
        None
    };
    let raw_chunks = match via_subprocess {
        Some(chunks) => chunks,
        None => {
            let md = markdown.clone();
            tokio::task::spawn_blocking(move || chunker::chunk_markdown(&md, &cfg))
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!("chunk task join error: {e}")))?
        }
    };
    let now = crate::utils::tz::now_utc();
    let mut inserts = Vec::with_capacity(raw_chunks.len());
    let mut chunk_ids: Vec<Option<SnowflakeId>> = vec![None; raw_chunks.len()];
    for (i, c) in raw_chunks.iter().enumerate() {
        let content = crate::kb::parse_quality::strip_page_marks(&c.content);
        if content.trim().is_empty() {
            continue; // anchor-only artifact chunk — never enters the index
        }
        let id = crate::utils::id::new_snowflake_id();
        chunk_ids[i] = Some(id);
        inserts.push(chunk::KbChunkInsert {
            id,
            kb_id: doc.kb_id,
            doc_id: Some(doc_id),
            faq_id: None,
            wiki_page_id: None,
            kind: "document".into(),
            parent_id: None, // patched below for children
            seq: i as i64,
            content,
            breadcrumb: if c.breadcrumb.is_empty() {
                None
            } else {
                Some(c.breadcrumb.clone())
            },
            byte_start: c.byte_start as i64,
            byte_end: c.byte_end as i64,
            questions: None,
            embedding: None,
            embedding_model: None,
            image_info: None,
            page: crate::kb::parse_quality::page_of(&marks, c.byte_start).map(i64::from),
            created_at: now,
        });
    }
    // link children to parents (raw index → kept insert position)
    // 子块（chunk_ids[i].is_some() 且有 parent）的 parent_id 指向父块 id。
    for (i, c) in raw_chunks.iter().enumerate() {
        let Some(child_id) = chunk_ids[i] else {
            continue;
        };
        if let Some(p) = c.parent
            && let Some(pid) = chunk_ids[p]
            && let Some(ins) = inserts.iter_mut().find(|ins| ins.id == child_id)
        {
            ins.parent_id = Some(pid);
        }
    }
    for ins in &inserts {
        chunk::insert_chunk(&deps.pool, ins).await?;
    }
    // Image registration (recognition-enabled KBs only): embedded assets +
    // external markdown references → `kb_images` pending rows
    // (kb-image-recognition-design §3 D4/D5).
    let mut image_count = 0usize;
    if image_cfg.is_some() {
        let chunk_rows: Vec<(SnowflakeId, i64, i64)> = inserts
            .iter()
            .map(|c| (c.id, c.byte_start, c.byte_end))
            .collect();
        image_count = crate::kb::images::register_document_images(
            deps,
            &doc,
            &parsed.images,
            &markdown,
            &chunk_rows,
            tenant_id,
        )
        .await?;
    }
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({
            "total": inserts.len(),
            "parents": raw_chunks.iter().filter(|c| c.parent.is_none()).count(),
            "children": raw_chunks.iter().filter(|c| c.parent.is_some()).count(),
        }),
        None,
    );

    // Leaf retrieval units: child chunks when a parent has children,
    // otherwise the childless parent itself (a lone small chunk).
    let has_children: std::collections::HashSet<usize> =
        raw_chunks.iter().filter_map(|c| c.parent).collect();
    let mut leaf_units: Vec<(i64, usize)> = Vec::new(); // (chunk_id, raw index)
    for (i, c) in raw_chunks.iter().enumerate() {
        if c.parent.is_some() || !has_children.contains(&i) {
            leaf_units.push((i64::from(inserts[i].id), i));
        }
    }

    // ④ embed children (parents ride along via expansion in the M3 pipeline)
    trace.stage("embed");
    document::set_document_status(&deps.pool, doc_id, "embedding", None, None, tenant_id).await?;
    let embed_targets: Vec<(usize, String)> = leaf_units
        .iter()
        .map(|&(_, raw_i)| (raw_i, raw_chunks[raw_i].content.clone()))
        .collect();
    let texts: Vec<&str> = embed_targets.iter().map(|(_, t)| t.as_str()).collect();
    let vectors = deps
        .embedder
        .embed_for(tenant_id, &model, dim, &texts)
        .await?;

    let mut items = Vec::with_capacity(embed_targets.len());
    for (idx, (raw_i, _)) in embed_targets.iter().enumerate() {
        let (chunk_id, _) = leaf_units
            .iter()
            .find(|&&(_, ri)| ri == *raw_i)
            .map(|&(cid, ri)| (cid, ri))
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("chunk id lookup failed")))?;
        // persist embedding BLOB (SQL is the truth, D3)
        raisfast_derive::crud_update!(
            &deps.pool,
            "kb_chunks",
            bind: ["embedding" => chunk::pack_embedding(&vectors[idx]), "embedding_model" => kb.embedding_model.clone().unwrap_or_default()],
            where: ("id", chunk_id)
        )?;
        items.push(VectorItem {
            unit_id: chunk_id,
            kb_id: i64::from(doc.kb_id),
            doc_id: Some(i64::from(doc_id)),
            kind: "document".into(),
            embedding: vectors[idx].clone(),
        });
    }
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({
            "chunks": embed_targets.len(),
            "model": model,
            "dim": dim,
        }),
        None,
    );

    // ⑤ vector + BM25 index (delete-then-insert idempotency per doc)
    trace.stage("vector");
    document::set_document_status(&deps.pool, doc_id, "indexing", None, None, tenant_id).await?;
    deps.vector
        .delete(
            i64::from(doc.kb_id),
            &items.iter().map(|v| v.unit_id).collect::<Vec<_>>(),
        )
        .await?;
    deps.vector
        .upsert(i64::from(doc.kb_id), dim, &items)
        .await?;
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({
            "backend": deps.vector.backend_name(),
            "upserted": items.len(),
        }),
        None,
    );
    let fts_units: Vec<KbIndexUnit> = inserts
        .iter()
        .map(|c| KbIndexUnit {
            unit_id: i64::from(c.id),
            kb_id: i64::from(c.kb_id),
            doc_id: i64::from(doc_id),
            kind: c.kind.clone(),
            text: match &c.breadcrumb {
                Some(b) => format!("{b}\n{}", c.content),
                None => c.content.clone(),
            },
        })
        .collect();
    trace.stage("bm25");
    deps.kbsearch.reindex_document(&fts_units).await?;
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({ "units": fts_units.len() }),
        None,
    );

    // ⑥ ready
    let total = inserts.len() as i64;
    document::set_document_status(&deps.pool, doc_id, "ready", None, Some(total), tenant_id)
        .await?;
    // Incremental invalidation (§7): pages citing this doc go stale.
    let stale = crate::kb::models::wiki_page::mark_stale_by_doc(&deps.pool, doc_id).await?;
    if !stale.is_empty() {
        tracing::info!(
            "[kb] doc {doc_id} re-parsed: {} wiki page(s) marked stale",
            stale.len()
        );
    }
    deps.emitter.emit(Event::KbDocumentReady(
        document::find_document_by_id(&deps.pool, doc_id, tenant_id)
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("doc vanished")))?,
    ));
    // Post-ready image recognition job (async gain, never blocks readiness;
    // kb-image-recognition-design §3 D3).
    if image_count > 0 {
        use crate::worker::JobQueue as _;
        let queue = crate::worker::DefaultJobQueue::new(deps.pool.clone());
        let mut new_job = crate::worker::NewJob::from(crate::worker::Job::KbImageRecognize {
            doc_id,
            tenant_id: tenant_id.to_string(),
        });
        new_job.priority = -5; // bulk VLM work must not starve online jobs
        new_job.timeout_secs = Some(crate::worker::LONG_JOB_TIMEOUT_SECS);
        queue.enqueue(new_job).await?;
        tracing::info!("[kb] doc {doc_id}: queued recognition for {image_count} image(s)");
    }
    Ok(())
}

/// Delete a document everywhere (SQL + vector + FTS), then emit.
pub async fn delete_document_everywhere(
    deps: &KbDeps,
    doc_id: SnowflakeId,
    tenant_id: &str,
) -> AppResult<()> {
    let Some(doc) = document::find_document_by_id(&deps.pool, doc_id, tenant_id).await? else {
        return Ok(());
    };
    // Doc-scoped index purges (self-describing deletes — no SQL pre-read).
    deps.vector
        .delete_document(i64::from(doc.kb_id), i64::from(doc_id))
        .await?;
    // Best-effort, same rationale as the re-parse cleanup above.
    if let Err(e) = deps.kbsearch.delete_document(i64::from(doc_id)).await {
        tracing::warn!(error = %e, doc = %doc_id, "fts cleanup failed — index is rebuildable, ignoring");
    }
    chunk::delete_chunks_by_doc(&deps.pool, doc_id).await?;
    crate::kb::models::wiki_source::delete_sources_by_doc(&deps.pool, doc_id).await?;
    document::delete_document(&deps.pool, doc_id, tenant_id).await?;
    // Original bytes on storage: warn-only cleanup (row is already gone;
    // a leaked file is preferable to a failed delete) [抄RF:services/media.rs].
    if let Some(key) = &doc.storage_key
        && let Err(e) = deps.storage.delete(key).await
    {
        tracing::warn!(key = %key, error = %e, "failed to delete kb document file from storage");
    }
    let stale = crate::kb::models::wiki_page::mark_stale_by_doc(&deps.pool, doc_id).await?;
    if !stale.is_empty() {
        tracing::info!(
            "[kb] doc {doc_id} deleted: {} wiki page(s) marked stale",
            stale.len()
        );
    }
    deps.emitter.emit(Event::KbDocumentDeleted(doc));
    Ok(())
}

/// Delete a KB and everything in it (documents, chunks, FAQs, wiki pages,
/// provenance links, vector + FTS indexes, original files), mirroring
/// `delete_document_everywhere` at KB scope.
pub async fn delete_kb_everywhere(
    deps: &KbDeps,
    kb_id: SnowflakeId,
    tenant_id: &str,
) -> AppResult<()> {
    let Some(kb) = knowledge_base::find_kb_by_id(&deps.pool, kb_id, tenant_id).await? else {
        return Ok(());
    };
    let docs = document::find_documents_by_kb(&deps.pool, kb_id, tenant_id).await?;
    // Vector + FTS: whole-KB wipe (both are keyed by kb_id; fts best-effort).
    deps.vector.delete_all(i64::from(kb_id)).await?;
    if let Err(e) = deps.kbsearch.delete_kb(i64::from(kb_id)).await {
        tracing::warn!(error = %e, kb = %kb_id, "fts wipe failed — index is rebuildable, ignoring");
    }
    // SQL rows, children first (no FK cascade: order matters).
    chunk::delete_chunks_by_kb(&deps.pool, kb_id).await?;
    wiki_source::delete_sources_by_kb(&deps.pool, kb_id).await?;
    wiki_page::delete_pages_by_kb(&deps.pool, kb_id, tenant_id).await?;
    faq::delete_faqs_by_kb(&deps.pool, kb_id, tenant_id).await?;
    document::delete_documents_by_kb(&deps.pool, kb_id, tenant_id).await?;
    knowledge_base::delete_kb(&deps.pool, kb_id, tenant_id).await?;
    // Original bytes on storage: warn-only cleanup (rows are already gone;
    // a leaked file is preferable to a failed delete) [抄RF:services/media.rs].
    for doc in &docs {
        if let Some(key) = &doc.storage_key
            && let Err(e) = deps.storage.delete(key).await
        {
            tracing::warn!(key = %key, error = %e, "failed to delete kb document file from storage");
        }
    }
    tracing::info!(
        "[kb] kb {} ({}) deleted: {} document(s) removed with indexes",
        i64::from(kb_id),
        kb.slug,
        docs.len()
    );
    Ok(())
}

async fn ensure_kb(
    deps: &KbDeps,
    kb_id: SnowflakeId,
    tenant_id: &str,
) -> AppResult<knowledge_base::KbKnowledgeBase> {
    knowledge_base::find_kb_by_id(&deps.pool, kb_id, tenant_id)
        .await?
        .ok_or_else(|| AppError::NotFound("kb_knowledge_base".into()))
}

fn normalize_mime(mime: &str, filename: &str) -> String {
    let m = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if m.is_empty() || m == "application/octet-stream" {
        mime_from_ext(filename).unwrap_or(m)
    } else {
        m
    }
}

fn mime_from_ext(filename: &str) -> Option<String> {
    let ext = filename.rsplit('.').next()?.to_lowercase();
    Some(
        match ext.as_str() {
            "md" | "markdown" => "text/markdown",
            "txt" => "text/plain",
            "html" | "htm" => "text/html",
            "csv" => "text/csv",
            "pdf" => "application/pdf",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "ppt" => "application/vnd.ms-powerpoint",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "epub" => "application/epub+zip",
            "json" => "application/json",
            "rtf" => "application/rtf",
            "odt" => "application/vnd.oasis.opendocument.text",
            _ => return None,
        }
        .to_string(),
    )
}

fn trim_filename(filename: &str) -> String {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "untitled".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kb::vectors::BruteForceIndex;

    /// Deterministic embedder for tests: dim from the hash of first chars.
    struct MockEmbedder {
        dim: usize,
    }

    #[async_trait::async_trait]
    impl KbEmbedder for MockEmbedder {
        async fn embed(&self, _tenant: &str, texts: &[&str]) -> AppResult<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0_f32; self.dim];
                    let seed = t.bytes().map(|b| b as usize).sum::<usize>();
                    v[seed % self.dim] = 1.0;
                    v
                })
                .collect())
        }
    }

    async fn test_deps() -> KbDeps {
        let pool = crate::test_pool!();
        let config = Arc::new(crate::config::app::AppConfig::test_defaults());
        let bus = crate::eventbus::EventBus::new(16);
        let router = crate::llm::service::LlmRouter::with_provider_for_test(
            Some(pool.clone()),
            Arc::new(NoopProvider),
            &["kb-test-model"],
            Some("kb-test-model"),
        )
        .await;
        KbDeps {
            pool,
            config,
            storage: Arc::new(
                crate::storage::local::LocalStorage::new("/tmp/kb-test-uploads", "/uploads")
                    .unwrap(),
            ),
            vector: Arc::new(BruteForceIndex::new()),
            kbsearch: Arc::new(KbSearchEngine::open_in_memory().unwrap()),
            embedder: Arc::new(MockEmbedder { dim: 4 }),
            reranker: None,
            parsers: std::sync::Arc::new(crate::docparse::ParserRegistry::new(Vec::new())),
            router,
            emitter: EventEmitter::eventbus_only(bus),
        }
    }

    async fn seed_kb(deps: &KbDeps) -> SnowflakeId {
        knowledge_base::create_kb(
            &deps.pool,
            &knowledge_base::CreateKbCmd {
                name: "产品文档".into(),
                description: None,
                slug: "docs".into(),
                kind: "document".into(),
                indexing_strategy: None,
                embedding_model: Some("test-model".into()),
                embedding_dim: Some(4),
                rerank_model: None,
                rerank_window: None,
                rerank_threshold: None,
                chat_model: None,
                distill_model: None,
                image_config: None,
                parser_config: None,
            },
            "default",
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn online_doc_full_pipeline() {
        let deps = test_deps().await;
        let kb_id = seed_kb(&deps).await;

        let mut markdown =
            "# 安装指南\n\n通过 cargo 安装 raisfast 服务端。\n\n## 配置数据库\n\n".to_string();
        markdown.push_str(&"支持 SQLite、PostgreSQL 与 MySQL 三种后端。".repeat(80));
        let doc = create_online_document(&deps, kb_id, "安装指南", &markdown, None, "default")
            .await
            .unwrap();
        assert_eq!(doc.status, "pending");

        process_document(&deps, doc.id, "default").await.unwrap();

        let doc = document::find_document_by_id(&deps.pool, doc.id, "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(doc.status, "ready", "error: {:?}", doc.error);
        assert!(doc.chunk_count > 0);

        let chunks = chunk::find_chunks_by_doc(&deps.pool, doc.id).await.unwrap();
        assert_eq!(chunks.len() as i64, doc.chunk_count);
        let children = chunks.iter().filter(|c| c.parent_id.is_some()).count();
        let embedded = chunks.iter().filter(|c| c.embedding.is_some()).count();
        eprintln!(
            "[DBG] chunks={} children={} embedded={}",
            chunks.len(),
            children,
            embedded
        );

        assert!(children > 0, "parent-child must produce children");
        assert_eq!(
            embedded, children,
            "exactly the child chunks carry embeddings"
        );
        // embedding BLOB roundtrip
        let with_vec = chunks.iter().find(|c| c.embedding.is_some()).unwrap();
        let v = chunk::unpack_embedding(with_vec.embedding.as_deref().unwrap());
        assert_eq!(v.len(), 4);
        // breadcrumbs populated for heading sections
        assert!(chunks.iter().any(|c| c.breadcrumb.is_some()));

        // BM25 side: search finds the doc's units in the KB scope
        let hits = deps
            .kbsearch
            .search(i64::from(kb_id), "数据库", 10)
            .await
            .unwrap();
        assert!(!hits.is_empty(), "fts must find the seeded content");

        // vector side: brute force hits a unit of this doc
        let probe = {
            let mut v = vec![0.0_f32; 4];
            v[3] = 1.0;
            v
        };
        let vhits = deps
            .vector
            .search(i64::from(kb_id), &probe, 10, None)
            .await
            .unwrap();
        assert!(!vhits.is_empty());

        // idempotency: reprocess keeps counts stable
        process_document(&deps, doc.id, "default").await.unwrap();
        let chunks2 = chunk::find_chunks_by_doc(&deps.pool, doc.id).await.unwrap();
        assert_eq!(
            chunks2.len(),
            chunks.len(),
            "reprocess must not duplicate chunks"
        );

        // delete cleans everywhere
        delete_document_everywhere(&deps, doc.id, "default")
            .await
            .unwrap();
        assert!(
            chunk::find_chunks_by_doc(&deps.pool, doc.id)
                .await
                .unwrap()
                .is_empty()
        );
        let hits = deps
            .kbsearch
            .search(i64::from(kb_id), "数据库", 10)
            .await
            .unwrap();
        assert!(hits.is_empty(), "fts must be cleared after delete");
    }

    #[tokio::test]
    async fn upload_rejects_unknown_mime() {
        let deps = test_deps().await;
        let kb_id = seed_kb(&deps).await;
        let err = upload_document(
            &deps,
            kb_id,
            "evil.exe",
            "application/x-executable",
            b"MZ..",
            None,
            "default",
        )
        .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn markdown_upload_passes_through_parser() {
        let deps = test_deps().await;
        let kb_id = seed_kb(&deps).await;
        let doc = upload_document(
            &deps,
            kb_id,
            "notes.md",
            "text/markdown",
            b"# T\n\nhello kb knowledge base markdown upload pipeline",
            None,
            "default",
        )
        .await
        .unwrap();
        process_document(&deps, doc.id, "default").await.unwrap();
        let doc = document::find_document_by_id(&deps.pool, doc.id, "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(doc.status, "ready");
    }

    /// E0 QA gate (kb-parser-engines-design §3.2): near-empty content never
    /// enters the index — the doc fails loudly.
    #[tokio::test]
    async fn empty_content_rejected_by_gate() {
        let deps = test_deps().await;
        let kb_id = seed_kb(&deps).await;
        let doc = upload_document(
            &deps,
            kb_id,
            "empty.md",
            "text/markdown",
            b"# E\n\n   \n\n  ",
            None,
            "default",
        )
        .await
        .unwrap();
        let err = process_document(&deps, doc.id, "default")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("解析产物为空"));
        let doc = document::find_document_by_id(&deps.pool, doc.id, "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(doc.status, "failed", "gate rejection must fail the doc");
        assert!(
            doc.error
                .as_deref()
                .is_some_and(|e| e.contains("解析产物为空"))
        );
    }

    /// E0 degraded marker (§3.3): set on partial parse, cleared on a clean
    /// re-parse; status stays `ready` either way.
    #[tokio::test]
    async fn degraded_marker_set_and_cleared() {
        let deps = test_deps().await;
        let kb_id = seed_kb(&deps).await;
        let doc = upload_document(
            &deps,
            kb_id,
            "d.md",
            "text/markdown",
            b"# D\n\ncontent long enough to pass the parse gate comfortably",
            None,
            "default",
        )
        .await
        .unwrap();
        process_document(&deps, doc.id, "default").await.unwrap();
        document::set_document_degraded(
            &deps.pool,
            doc.id,
            Some(r#"{"engine":"builtin","scanned_pages":[2,3]}"#),
            "default",
        )
        .await
        .unwrap();
        let doc = document::find_document_by_id(&deps.pool, doc.id, "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(doc.status, "ready", "degraded keeps ready status");
        assert!(
            doc.parse_degraded
                .as_deref()
                .is_some_and(|v| v.contains("scanned_pages"))
        );
        // Clean re-parse wipes the marker.
        process_document(&deps, doc.id, "default").await.unwrap();
        let doc = document::find_document_by_id(&deps.pool, doc.id, "default")
            .await
            .unwrap()
            .unwrap();
        assert!(
            doc.parse_degraded.is_none(),
            "clean reparse must clear the marker"
        );
    }
}

/// Traced chunk edit [kb-observability-design §4.2 chunk_edit]: snapshot
/// revision → content update → re-embed → vector + FTS reindex. The
/// five-step sequence is one autocommit chain — a mid-way failure leaves
/// the doc `ready` with a stale vector, which is exactly why the run row
/// (kind=`chunk_edit`) is the only durable record of the failure.
pub async fn edit_chunk_traced(
    deps: &KbDeps,
    chunk: &crate::kb::models::chunk::KbChunk,
    user_id: Option<SnowflakeId>,
    content: &str,
) -> AppResult<()> {
    let prior = crate::kb::models::kb_run::count_runs(
        &deps.pool,
        crate::kb::models::kb_run::KIND_CHUNK_EDIT,
        chunk.doc_id,
        None,
    )
    .await
    .unwrap_or(0);
    let mut trace = crate::kb::trace::RunRecorder::create(
        crate::kb::trace::TraceMode::parse(&deps.config.kb.trace_mode),
        crate::kb::trace::RunSpec {
            kind: crate::kb::models::kb_run::KIND_CHUNK_EDIT,
            trigger_src: "admin",
            tenant_id: "default".to_string(), // resolved from the KB row on the ingest path; chunk rows carry no tenant
            kb_id: Some(chunk.kb_id),
            doc_id: chunk.doc_id,
            agent_id: None,
            session_id: None,
            job_id: None,
            attempt: prior + 1,
            long_running: false,
        },
    );
    trace.begin(&deps.pool, &deps.config).await;
    trace.stage("persist");
    let result = edit_chunk_inner(deps, chunk, user_id, content, &mut trace).await;
    match &result {
        Ok(()) => {
            trace
                .finish(&deps.pool, crate::kb::models::kb_run::STATUS_OK, None)
                .await;
        }
        Err(e) => {
            trace.fail_stage(&e.to_string());
            trace
                .finish(
                    &deps.pool,
                    crate::kb::models::kb_run::STATUS_FAILED,
                    Some(&e.to_string()),
                )
                .await;
        }
    }
    result
}

async fn edit_chunk_inner(
    deps: &KbDeps,
    chunk: &crate::kb::models::chunk::KbChunk,
    user_id: Option<SnowflakeId>,
    content: &str,
    trace: &mut crate::kb::trace::RunRecorder,
) -> AppResult<()> {
    // Snapshot the pre-edit content.
    let snapshot = serde_json::json!({
        "content": chunk.content,
        "kind": chunk.kind,
        "breadcrumb": chunk.breadcrumb,
    });
    raisfast_derive::crud_insert!(
        &deps.pool,
        "content_revisions",
        [
            "content_type" => "kb_chunk",
            "record_id" => chunk.id,
            "revision_number" => 1_i64,
            "snapshot" => snapshot,
            "created_by" => user_id
        ]
    )?;
    crate::kb::models::chunk::update_content(&deps.pool, chunk.id, content).await?;
    trace.end_stage(crate::kb::trace::STAGE_OK, serde_json::json!({}), None);
    trace.stage("embed");
    // Update content + re-embed + re-index both paths.
    let vectors = deps.embedder.embed("default", &[content]).await?;
    let vector = vectors.first().cloned().unwrap_or_default();
    crate::kb::models::chunk::update_embedding(&deps.pool, chunk.id, &vector).await?;
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({ "dim": vector.len() }),
        None,
    );
    trace.stage("index");
    deps.vector
        .upsert(
            i64::from(chunk.kb_id),
            vector.len() as u32,
            &[VectorItem {
                unit_id: i64::from(chunk.id),
                kb_id: i64::from(chunk.kb_id),
                doc_id: chunk.doc_id.map(i64::from),
                kind: chunk.kind.clone(),
                embedding: vector,
            }],
        )
        .await?;
    let doc_key = chunk
        .doc_id
        .map(i64::from)
        .unwrap_or_else(|| i64::from(chunk.id));
    deps.kbsearch
        .reindex_document(&[KbIndexUnit {
            unit_id: i64::from(chunk.id),
            kb_id: i64::from(chunk.kb_id),
            doc_id: doc_key,
            kind: chunk.kind.clone(),
            text: content.to_string(),
        }])
        .await?;
    trace.end_stage(
        crate::kb::trace::STAGE_OK,
        serde_json::json!({ "backend": deps.vector.backend_name() }),
        None,
    );
    Ok(())
}

/// Full vector-index rebuild for one KB from the SQL truth (D3) — shared
/// by the admin endpoint, the `KbRebuildVectorIndex` job and the
/// bruteforce lazy warm-up (DR10). `tenant_hint` is only used for logging;
/// the KB row is resolved by id (infrastructure operation, cross-tenant
/// by design). Returns the number of re-indexed units.
pub async fn rebuild_vector_index_from_sql(
    pool: &crate::db::Pool,
    vector: &std::sync::Arc<dyn crate::kb::vectors::VectorIndex>,
    kb_id: SnowflakeId,
) -> AppResult<usize> {
    let sql = format!(
        "SELECT embedding_dim FROM kb_knowledge_bases WHERE id = {}",
        crate::db::Driver::ph(1)
    );
    let dim: Option<i64> = sqlx::query_scalar(crate::db::safe_sql(&sql))
        .bind(i64::from(kb_id))
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e.to_string())))?
        .flatten();
    let dim = u32::try_from(dim.unwrap_or(0)).unwrap_or(0);
    if dim == 0 {
        return Err(AppError::BadRequest(
            "kb has no embedding_dim configured".into(),
        ));
    }
    let chunks = raisfast_derive::crud_find_all!(
        pool,
        "kb_chunks",
        crate::kb::models::chunk::KbChunk,
        where: ("kb_id", kb_id)
    )?;
    let mut items = Vec::new();
    for c in chunks.iter().filter(|c| c.embedding.is_some()) {
        items.push(crate::kb::vectors::VectorItem {
            unit_id: i64::from(c.id),
            kb_id: i64::from(c.kb_id),
            doc_id: c.doc_id.map(i64::from),
            kind: c.kind.clone(),
            embedding: crate::kb::models::chunk::unpack_embedding(
                c.embedding.as_deref().unwrap_or_default(),
            ),
        });
    }
    let n = items.len();
    vector.rebuild(i64::from(kb_id), dim, &items).await?;
    tracing::info!("[kb] vector index rebuilt for kb {kb_id}: {n} units");
    Ok(n)
}

// ── FAQ indexing (M5) ──────────────────────────────────────────────

/// (Re)index a FAQ: embed every question variant as a `kind='faq'` unit
/// into vector + BM25. Idempotent per FAQ (delete-then-insert).
pub async fn index_faq(deps: &KbDeps, faq: &crate::kb::models::faq::KbFaq) -> AppResult<()> {
    let old = crate::kb::models::chunk::find_chunks_by_faq(&deps.pool, faq.id).await?;
    let old_ids: Vec<i64> = old.iter().map(|c| i64::from(c.id)).collect();
    if !old_ids.is_empty() {
        deps.vector.delete(i64::from(faq.kb_id), &old_ids).await?;
        deps.kbsearch.delete_document(i64::from(faq.id)).await?;
    }
    crate::kb::models::chunk::delete_chunks_by_faq(&deps.pool, faq.id).await?;

    let variants = crate::kb::models::faq::question_variants(faq);
    let texts: Vec<&str> = variants.iter().map(String::as_str).collect();
    let kb_row = crate::kb::models::knowledge_base::find_kb_by_id(&deps.pool, faq.kb_id, "default")
        .await?
        .ok_or_else(|| AppError::NotFound("kb_knowledge_base".into()))?;
    let model = kb_row.embedding_model.clone().unwrap_or_default();
    let dim = u32::try_from(kb_row.embedding_dim.unwrap_or(0)).unwrap_or(0);
    let vectors = deps
        .embedder
        .embed_for("default", &model, dim, &texts)
        .await?;
    let now = crate::utils::tz::now_utc();
    let mut items = Vec::with_capacity(variants.len());
    let mut fts = Vec::with_capacity(variants.len());
    for (idx, variant) in variants.iter().enumerate() {
        let chunk_id = crate::utils::id::new_snowflake_id();
        crate::kb::models::chunk::insert_chunk(
            &deps.pool,
            &crate::kb::models::chunk::KbChunkInsert {
                id: chunk_id,
                kb_id: faq.kb_id,
                doc_id: None,
                faq_id: Some(faq.id),
                wiki_page_id: None,
                kind: "faq".into(),
                parent_id: None,
                seq: idx as i64,
                content: variant.clone(),
                breadcrumb: None,
                byte_start: 0,
                byte_end: 0,
                questions: None,
                embedding: Some(crate::kb::models::chunk::pack_embedding(&vectors[idx])),
                embedding_model: None,
                image_info: None,
                page: None,
                created_at: now,
            },
        )
        .await?;
        items.push(crate::kb::vectors::VectorItem {
            unit_id: i64::from(chunk_id),
            kb_id: i64::from(faq.kb_id),
            doc_id: None,
            kind: "faq".into(),
            embedding: vectors[idx].clone(),
        });
        fts.push(crate::kb::kbsearch::KbIndexUnit {
            unit_id: i64::from(chunk_id),
            kb_id: i64::from(faq.kb_id),
            doc_id: i64::from(faq.id),
            kind: "faq".into(),
            text: variant.clone(),
        });
    }
    let dim = vectors.first().map(|v| v.len() as u32).unwrap_or(0);
    deps.vector
        .upsert(i64::from(faq.kb_id), dim, &items)
        .await?;
    deps.kbsearch.reindex_document(&fts).await?;
    Ok(())
}

/// Remove a FAQ everywhere.
pub async fn deindex_faq(deps: &KbDeps, faq_id: SnowflakeId, kb_id: SnowflakeId) -> AppResult<()> {
    let old = crate::kb::models::chunk::find_chunks_by_faq(&deps.pool, faq_id).await?;
    let ids: Vec<i64> = old.iter().map(|c| i64::from(c.id)).collect();
    if !ids.is_empty() {
        deps.vector.delete(i64::from(kb_id), &ids).await?;
    }
    deps.kbsearch.delete_document(i64::from(faq_id)).await?;
    crate::kb::models::chunk::delete_chunks_by_faq(&deps.pool, faq_id).await?;
    Ok(())
}

#[cfg(test)]
mod faq_tests {
    use super::*;
    use crate::kb::models::faq::{self, CreateFaqCmd};
    use crate::kb::pipeline::{self, AskRequest};
    use crate::kb::vectors::BruteForceIndex;
    use raisfast_agent::{ChatRequest, ChatResponse, ModelProvider, ProviderError};

    struct OneHotEmbedder;
    #[async_trait::async_trait]
    impl KbEmbedder for OneHotEmbedder {
        async fn embed(&self, _tenant: &str, texts: &[&str]) -> AppResult<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0_f32; 4];
                    v[t.bytes().map(|b| b as usize).sum::<usize>() % 4] = 1.0;
                    v
                })
                .collect())
        }
    }

    struct EchoProvider;
    #[async_trait::async_trait]
    impl ModelProvider for EchoProvider {
        fn name(&self) -> &str {
            "echo"
        }
        async fn chat(
            &self,
            _r: &ChatRequest<'_>,
            _m: &str,
        ) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse::text_only("[1] FAQ 命中回答。"))
        }
    }

    async fn deps() -> KbDeps {
        let pool = crate::test_pool!();
        let mut config = crate::config::app::AppConfig::test_defaults();
        config.kb.enabled = true;
        config.kb.fallback_threshold = 0.05;
        let router = crate::llm::service::LlmRouter::with_provider_for_test(
            Some(pool.clone()),
            Arc::new(EchoProvider),
            &["kb-test-model"],
            Some("kb-test-model"),
        )
        .await;
        KbDeps {
            pool,
            config: Arc::new(config),
            storage: Arc::new(
                crate::storage::local::LocalStorage::new("/tmp/kb-faq-test", "/uploads").unwrap(),
            ),
            vector: Arc::new(BruteForceIndex::new()),
            kbsearch: Arc::new(crate::kb::kbsearch::KbSearchEngine::open_in_memory().unwrap()),
            embedder: Arc::new(OneHotEmbedder),
            reranker: None,
            parsers: std::sync::Arc::new(crate::docparse::ParserRegistry::new(Vec::new())),
            router,
            emitter: crate::event::EventEmitter::eventbus_only(crate::eventbus::EventBus::new(16)),
        }
    }

    async fn seed_kb(deps: &KbDeps) -> SnowflakeId {
        crate::kb::models::knowledge_base::create_kb(
            &deps.pool,
            &crate::kb::models::knowledge_base::CreateKbCmd {
                name: "faq-kb".into(),
                description: None,
                slug: "faq-kb".into(),
                kind: "document".into(),
                indexing_strategy: None,
                embedding_model: Some("m".into()),
                embedding_dim: Some(4),
                rerank_model: None,
                rerank_window: None,
                rerank_threshold: None,
                chat_model: None,
                distill_model: None,
                image_config: None,
                parser_config: None,
            },
            "default",
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn faq_index_withdraw_and_pipeline_injection() {
        let deps = deps().await;
        let kb_id = seed_kb(&deps).await;
        let faq = faq::create_faq(
            &deps.pool,
            &CreateFaqCmd {
                kb_id,
                standard_question: "如何重置密码".into(),
                similar_questions: vec!["忘记密码怎么办".into()],
                answers: vec!["在登录页点击「忘记密码」".into()],
                enabled: true,
                created_by: None,
            },
            "default",
        )
        .await
        .unwrap();
        index_faq(&deps, &faq).await.unwrap();

        // variants indexed as faq units in both stores
        let units = crate::kb::models::chunk::find_chunks_by_faq(&deps.pool, faq.id)
            .await
            .unwrap();
        assert_eq!(units.len(), 2);
        assert!(
            units
                .iter()
                .all(|u| u.kind == "faq" && u.embedding.is_some())
        );
        let hits = deps
            .kbsearch
            .search(i64::from(kb_id), "重置密码", 10)
            .await
            .unwrap();
        assert!(!hits.is_empty());

        // pipeline: ask the FAQ → answered, FAQ unit pinned first
        let ask = AskRequest {
            tenant_id: "default".into(),
            kb_ids: vec![i64::from(kb_id)],
            doc_ids: Vec::new(),
            question: "如何重置密码".into(),
        };
        let mut outcome = pipeline::prepare_answer(&deps, &ask).await.unwrap();
        pipeline::finish_answer(&deps, &mut outcome).await.unwrap();
        assert_eq!(outcome.status, "answered");
        assert!(
            outcome.context_units.first().is_some_and(|u| u.is_faq),
            "FAQ must lead context"
        );

        // withdraw (disable) removes units from retrieval
        deindex_faq(&deps, faq.id, kb_id).await.unwrap();
        let units = crate::kb::models::chunk::find_chunks_by_faq(&deps.pool, faq.id)
            .await
            .unwrap();
        assert!(units.is_empty());
    }

    #[tokio::test]
    async fn gaps_list_uncovered_queries() {
        let deps = deps().await;
        let kb_id = seed_kb(&deps).await;
        crate::kb::models::query_log::insert_log(
            &deps.pool,
            Some(kb_id),
            "什么是量子纠缠",
            Some("知识库未覆盖该问题。"),
            None,
            "uncovered",
            Some(0.0),
            None,
        )
        .await
        .unwrap();
        crate::kb::models::query_log::insert_log(
            &deps.pool,
            Some(kb_id),
            "什么是量子纠缠",
            Some("知识库未覆盖该问题。"),
            None,
            "uncovered",
            Some(0.0),
            None,
        )
        .await
        .unwrap();
        let gaps = crate::kb::models::query_log::list_gaps(&deps.pool, 10)
            .await
            .unwrap();
        let entry = gaps.iter().find(|(q, _, _)| q == "什么是量子纠缠");
        assert!(
            entry.is_some_and(|(_, c, _)| *c >= 2),
            "gaps must aggregate counts: {gaps:?}"
        );
    }

    /// Fixture re-export for `trace_tests` (same shape, explicit seam).
    pub(super) async fn deps_for_trace() -> KbDeps {
        deps().await
    }

    pub(super) async fn seed_kb_for_trace(deps: &KbDeps) -> SnowflakeId {
        seed_kb(deps).await
    }
}

#[cfg(test)]
mod trace_tests {
    //! [kb-observability-design T1] ingest 链 run 记录断言：成功/失败落库、
    //! stages 完整性、errors 模式不落成功 run。
    use super::*;
    use crate::kb::models::kb_run::{self, RunFilter};
    use crate::kb::trace::{RunRecorder, RunSpec, TraceMode};

    async fn deps() -> KbDeps {
        // Reuse the FAQ test fixture shape (one-hot embedder + bruteforce).
        super::faq_tests::deps_for_trace().await
    }

    fn recorder(mode: TraceMode, doc_id: SnowflakeId, attempt: i64) -> RunRecorder {
        RunRecorder::create(
            mode,
            RunSpec {
                kind: kb_run::KIND_INGEST_DOC,
                trigger_src: "admin",
                tenant_id: "default".into(),
                kb_id: None,
                doc_id: Some(doc_id),
                agent_id: None,
                session_id: None,
                job_id: None,
                attempt,
                long_running: true,
            },
        )
    }

    #[tokio::test]
    async fn ingest_success_records_ok_run_with_stages() {
        let deps = deps().await;
        let kb_id = super::faq_tests::seed_kb_for_trace(&deps).await;
        let mut markdown = "# 部署\n\n".to_string();
        markdown.push_str(&"raisfast 单二进制部署，支持 docker 与裸机。".repeat(40));
        let doc = create_online_document(&deps, kb_id, "部署", &markdown, None, "default")
            .await
            .unwrap();
        let mut trace = recorder(TraceMode::All, doc.id, 1);
        process_document_traced(&deps, doc.id, "default", &mut trace)
            .await
            .unwrap();

        let (runs, total) = kb_run::list_runs(
            &deps.pool,
            "default",
            &RunFilter {
                kind: Some("ingest_doc".into()),
                doc_id: Some(i64::from(doc.id)),
                ..RunFilter::default()
            },
            1,
            10,
        )
        .await
        .unwrap();
        assert_eq!(total, 1, "exactly one run row for this doc");
        let run = runs.first().expect("run row");
        assert_eq!(run.status, "ok");
        assert_eq!(run.attempt, 1);
        assert_eq!(run.kb_id, Some(kb_id));
        assert!(run.latency_ms.is_some());
        let stages = run
            .stages
            .as_ref()
            .and_then(|s| s.as_array())
            .expect("stages");
        let names: Vec<&str> = stages
            .iter()
            .filter_map(|s| s.get("stage").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(names, vec!["parse", "chunk", "embed", "vector", "bm25"]);
        assert!(
            stages
                .iter()
                .all(|s| s.get("status").and_then(|v| v.as_str()) == Some("ok"))
        );
        // detail lookup roundtrip
        let detail = kb_run::find_run_by_id(&deps.pool, run.id, "default")
            .await
            .unwrap()
            .expect("detail");
        assert_eq!(detail.id, run.id);
    }

    #[tokio::test]
    async fn ingest_failure_records_failed_run_and_error() {
        let deps = deps().await;
        let kb_id = super::faq_tests::seed_kb_for_trace(&deps).await;
        // Doc whose storage key does not exist → parse stage fails.
        let doc = document::create_document(
            &deps.pool,
            &document::CreateKbDocumentCmd {
                kb_id,
                title: "ghost".into(),
                source: "upload".into(),
                storage_key: Some("kb/missing-file.bin".into()),
                mime_type: Some("text/plain".into()),
                size: 10,
                created_by: None,
            },
            "default",
        )
        .await
        .unwrap();
        let mut trace = recorder(TraceMode::All, doc.id, 2);
        let result = process_document_traced(&deps, doc.id, "default", &mut trace).await;
        assert!(result.is_err(), "missing storage bytes must fail ingest");

        let (runs, _) = kb_run::list_runs(
            &deps.pool,
            "default",
            &RunFilter {
                kind: Some("ingest_doc".into()),
                doc_id: Some(i64::from(doc.id)),
                ..RunFilter::default()
            },
            1,
            10,
        )
        .await
        .unwrap();
        let run = runs.first().expect("run row");
        assert_eq!(run.status, "failed");
        assert_eq!(run.attempt, 2);
        assert!(run.error.as_deref().is_some_and(|e| !e.is_empty()));
        let stages = run
            .stages
            .as_ref()
            .and_then(|s| s.as_array())
            .expect("stages");
        let failed = stages
            .iter()
            .find(|s| s.get("status").and_then(|v| v.as_str()) == Some("failed"))
            .expect("a failed stage must be recorded");
        assert_eq!(failed.get("stage").and_then(|v| v.as_str()), Some("parse"));
    }

    #[tokio::test]
    async fn errors_mode_skips_successful_run() {
        let deps = deps().await;
        let kb_id = super::faq_tests::seed_kb_for_trace(&deps).await;
        let mut markdown = "# 备份\n\n".to_string();
        markdown.push_str(&"使用 just db-backup 进行数据库备份。".repeat(40));
        let doc = create_online_document(&deps, kb_id, "备份", &markdown, None, "default")
            .await
            .unwrap();
        let mut trace = recorder(TraceMode::Errors, doc.id, 1);
        process_document_traced(&deps, doc.id, "default", &mut trace)
            .await
            .unwrap();

        let (_, total) = kb_run::list_runs(
            &deps.pool,
            "default",
            &RunFilter {
                kind: Some("ingest_doc".into()),
                doc_id: Some(i64::from(doc.id)),
                ..RunFilter::default()
            },
            1,
            10,
        )
        .await
        .unwrap();
        assert_eq!(
            total, 0,
            "DR5: successful runs are not persisted in errors mode"
        );
    }
}
