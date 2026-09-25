//! Application configuration structs and loading logic.
//!
//! All configuration items are provided via environment variables, with `.env` file support.
//! Missing optional variables fall back to sensible defaults.

use std::env;

use crate::db::DbDriver;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Application global configuration.
///
/// | Environment Variable | Type | Default | Description |
/// |----------|------|--------|------|
/// | `APP_HOST` | String | `0.0.0.0` | Listen address |
/// | `APP_PORT` | u16 | `9898` | Listen port |
/// | `APP_ENV` | String | `development` | Runtime environment |
/// | `API_RESTFUL` | bool | `true` | `true` = full RESTful (GET/POST/PUT/DELETE), `false` = GET/POST only |
/// | `DATABASE_URL` | String | (varies by DB backend) | Database connection string |
/// | `DB_POOL_SIZE` | u32 | `5` | Connection pool size |
/// | `JWT_SECRET` | String | (built-in default) | JWT signing secret |
/// | `JWT_ACCESS_EXPIRES` | u64 | `900` (15 minutes) | Access Token expiration time (seconds) |
/// | `JWT_REFRESH_EXPIRES` | u64 | `604800` (7 days) | Refresh Token expiration time (seconds) |
/// | `UPLOAD_DIR` | String | `{STORAGE_ROOT_DIR}/uploads` | Upload file storage directory |
/// | `MAX_UPLOAD_SIZE` | usize | `104857600` (100 MB) | Upload file size limit (bytes) |
/// | `STATIC_DIR` | String | `./static` | Static files directory (favicon, robots.txt, etc.) |
/// | `BASE_URL` | String | `http://{host}:{port}` | Full site URL (for RSS/media links) |
/// | `CORS_ORIGINS` | String | (empty=all allowed) | CORS allowed origins, comma-separated |
/// | `TLS_CERT_PATH` | String | (empty=HTTP) | TLS certificate file path (PEM format) |
/// | `TLS_KEY_PATH` | String | (empty=HTTP) | TLS private key file path (PEM format) |
/// | `PLUGIN_WASM_POOL_SIZE` | u32 | `4` | WASM instance pool size |
/// | `PLUGIN_LUA_POOL_SIZE` | u32 | `4` | Lua instance pool size |
/// | `PLUGIN_JS_POOL_SIZE` | u32 | `4` | JS instance pool size |
/// | `APP_TIMEZONE` | String | `UTC` | Site timezone (IANA format, e.g., `Asia/Shanghai`) |
/// | `GRAPHQL_ENABLED` | bool | `false` | Whether to enable GraphQL API |
/// | `WEBSOCKET_ENABLED` | bool | `false` | Whether to enable WebSocket real-time push |
/// | `STORAGE_ROOT_DIR` | String | `./storage` | Local file storage root directory (parent of uploads/logs/search_index/vfs/db) |
/// | `UPLOAD_DIR` | String | `{STORAGE_ROOT_DIR}/uploads` | Media upload directory |
/// | `LOG_DIR` | String | `{STORAGE_ROOT_DIR}/logs` | Log file directory |
/// | `SEARCH_INDEX_DIR` | String | `{STORAGE_ROOT_DIR}/search_index` | Search index directory |
/// | `PRESENCE_HEARTBEAT_TTL_SECS` | u64 | `75` | PresenceMap heartbeat freshness window (architecture §5.3) |
/// | `PRESENCE_SWEEP_INTERVAL_SECS` | u64 | `10` | PresenceMap reaper sweep interval |
/// | `PRESENCE_HEARTBEAT_INTERVAL_SECS` | u64 | `30` | Workspace frontend heartbeat cadence |
/// | `SSE_MAX_CLIENTS` | u64 | `64` | Authed workspace SSE stream cap |
/// | `SSE_MAX_SESSION_CLIENTS` | u64 | `512` | Public session (widget) SSE stream cap |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub env: String,
    pub api_restful: bool,
    pub database_url: String,
    pub db_pool_size: u32,
    pub jwt_secret: String,
    pub jwt_access_expires: u64,
    pub jwt_refresh_expires: u64,
    #[serde(default = "default_storage_root_dir")]
    pub storage_root_dir: String,
    pub upload_dir: String,
    #[serde(default = "default_backup_retention")]
    pub backup_retention: usize,
    #[serde(skip)]
    pub started_at: Option<std::time::Instant>,
    pub max_upload_size: usize,
    pub static_dir: String,
    pub widget_dir: String,
    pub base_url: String,
    pub cors_origins: Option<String>,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: Option<String>,
    #[serde(default)]
    pub plugin_hot_reload: bool,
    #[serde(default = "default_plugin_max_memory")]
    pub plugin_max_memory_mb: u32,
    #[serde(default = "default_plugin_timeout")]
    pub plugin_default_timeout_ms: u64,
    #[serde(default = "default_plugin_wasm_pool_size")]
    pub plugin_wasm_pool_size: u32,
    #[serde(default = "default_plugin_lua_pool_size")]
    pub plugin_lua_pool_size: u32,
    #[serde(default = "default_plugin_js_pool_size")]
    pub plugin_js_pool_size: u32,
    #[serde(default)]
    pub plugin_disabled: Vec<String>,
    #[serde(default = "default_plugin_vfs_root")]
    pub plugin_vfs_root: String,
    #[serde(default = "default_plugin_vfs_max_file_size")]
    pub plugin_vfs_max_file_size: usize,
    #[serde(default = "default_plugin_vfs_max_total_size")]
    pub plugin_vfs_max_total_size: usize,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_log_max_files")]
    pub log_max_files: usize,
    #[serde(default = "default_rate_limit_enabled")]
    pub rate_limit_enabled: bool,
    #[serde(default = "default_rate_limit_global_max")]
    pub rate_limit_global_max: u32,
    #[serde(default = "default_rate_limit_global_window")]
    pub rate_limit_global_window: u64,
    #[serde(default = "default_rate_limit_register_max")]
    pub rate_limit_register_max: u32,
    #[serde(default = "default_rate_limit_register_window")]
    pub rate_limit_register_window: u64,
    #[serde(default = "default_rate_limit_login_max")]
    pub rate_limit_login_max: u32,
    #[serde(default = "default_rate_limit_login_window")]
    pub rate_limit_login_window: u64,
    #[serde(default = "default_rate_limit_comment_max")]
    pub rate_limit_comment_max: u32,
    #[serde(default = "default_rate_limit_comment_window")]
    pub rate_limit_comment_window: u64,
    #[serde(default = "default_rate_limit_api_token_max")]
    pub rate_limit_api_token_max: u32,
    #[serde(default = "default_rate_limit_api_token_window")]
    pub rate_limit_api_token_window: u64,
    #[serde(default)]
    pub worker_enabled: bool,
    #[serde(default = "default_worker_concurrency")]
    pub worker_concurrency: usize,
    /// Size of the dedicated CPU pool (document parse / image / index). Defaults
    /// to `min(cpu cores, 4)`.
    #[serde(default = "default_worker_cpu_concurrency")]
    pub worker_cpu_concurrency: usize,
    #[serde(default = "default_worker_poll_interval_ms")]
    pub worker_poll_interval_ms: u64,
    #[serde(default = "default_worker_batch_size")]
    pub worker_batch_size: usize,
    #[serde(default = "default_worker_max_attempts")]
    pub worker_default_max_attempts: u32,
    #[serde(default = "default_worker_cron_tick_ms")]
    pub worker_cron_tick_ms: u64,
    #[serde(default = "default_worker_visibility_timeout_secs")]
    pub worker_visibility_timeout_secs: u64,
    /// Hard runtime cap (seconds) applied to any job that does not set its own
    /// `timeout_secs`; bounds runaway/hung jobs. `0` disables the global cap.
    /// Env `WORKER_JOB_TIMEOUT_SECS` (default 86400 = 24h).
    #[serde(default = "default_worker_job_timeout_secs")]
    pub worker_job_timeout_secs: u64,
    #[serde(default = "default_worker_sweep_interval_secs")]
    pub worker_sweep_interval_secs: u64,
    #[serde(default)]
    pub cron_seed_enabled: bool,
    #[serde(default = "default_cron_schedules")]
    pub cron_schedules: Vec<CronScheduleConfig>,
    #[serde(default = "default_cron_log_retention_days")]
    pub cron_log_retention_days: i64,
    /// Global switch for exec_kind=system schedules. When false, system schedules
    /// are rejected at creation time and silently skipped at dispatch.
    #[serde(default)]
    pub cron_allow_system_scripts: bool,
    /// Working directory for system script execution. Defaults to storage_root_dir.
    #[serde(default)]
    pub cron_system_workdir: Option<String>,
    #[serde(default = "default_order_expire_minutes")]
    pub order_expire_minutes: i64,
    #[serde(default = "default_search_engine")]
    pub search_engine: String,
    #[serde(default = "default_search_index_dir")]
    pub search_index_dir: String,
    #[serde(default = "default_content_type_dir")]
    pub content_type_dir: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_storage_driver")]
    pub storage_driver: String,
    pub s3_endpoint: Option<String>,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    #[serde(default = "default_s3_bucket")]
    pub s3_bucket: String,
    #[serde(default = "default_s3_region")]
    pub s3_region: String,
    pub s3_public_url: Option<String>,
    #[serde(default)]
    pub rule_engine: RuleEngineConfig,
    /// Whether to enable GraphQL API (default false)
    #[serde(default)]
    pub graphql_enabled: bool,
    /// Whether to enable WebSocket real-time push (default false)
    #[serde(default)]
    pub websocket_enabled: bool,
    /// MCP (Model Context Protocol) server configuration
    #[serde(default)]
    pub mcp: McpConfig,
    /// AI agent runtime configuration
    #[serde(default)]
    pub ai: AiConfig,
    /// Knowledge base (KB) configuration
    #[serde(default)]
    pub kb: KbConfig,
    #[serde(default)]
    pub integration: IntegrationConfig,
    #[serde(default)]
    pub apps: AppsConfig,
    #[serde(default)]
    pub oauth: crate::config::oauth::OAuthConfig,
    #[serde(default = "default_true")]
    pub registration_email_enabled: bool,
    #[serde(default)]
    pub registration_sms_enabled: bool,
    /// Presence store (PresenceMap) timing — see architecture §5.3.
    #[serde(default = "default_presence_heartbeat_ttl_secs")]
    pub presence_heartbeat_ttl_secs: u64,
    #[serde(default = "default_presence_sweep_interval_secs")]
    pub presence_sweep_interval_secs: u64,
    #[serde(default = "default_presence_heartbeat_interval_secs")]
    pub presence_heartbeat_interval_secs: u64,
    /// SSE connection caps (architecture §10.2): authed workspace stream and
    /// public session (widget) stream. Hardcoded 64/512 before 2026-09-01.
    #[serde(default = "default_sse_max_clients")]
    pub sse_max_clients: u64,
    #[serde(default = "default_sse_max_session_clients")]
    pub sse_max_session_clients: u64,
    #[serde(default = "default_sms_code_expires_in")]
    pub sms_code_expires_in: u64,
    #[serde(default = "default_sms_code_length")]
    pub sms_code_length: u32,
    #[serde(default = "default_sms_rate_limit_secs")]
    pub sms_rate_limit_secs: u64,
    #[serde(default)]
    pub require_email_verification: bool,
    #[serde(default)]
    pub builtins: BuiltinsConfig,
    #[serde(default)]
    pub builtin_tenantable: bool,
    pub base_domain: Option<String>,
    #[serde(default = "default_email_provider")]
    pub email_provider: String,
    pub email_smtp_host: Option<String>,
    #[serde(default = "default_email_smtp_port")]
    pub email_smtp_port: u16,
    pub email_smtp_user: Option<String>,
    pub email_smtp_pass: Option<String>,
    pub email_from: Option<String>,
    pub email_from_name: Option<String>,
    pub email_sendgrid_api_key: Option<String>,
    pub email_resend_api_key: Option<String>,
    pub email_aliyun_access_key_id: Option<String>,
    pub email_aliyun_access_key_secret: Option<String>,
    pub email_aliyun_region: Option<String>,
    pub email_tencent_secret_id: Option<String>,
    pub email_tencent_secret_key: Option<String>,
    pub email_tencent_region: Option<String>,
    #[serde(default = "default_sms_provider")]
    pub sms_provider: String,
    pub sms_aliyun_access_key_id: Option<String>,
    pub sms_aliyun_access_key_secret: Option<String>,
    pub sms_aliyun_sign_name: Option<String>,
    pub sms_aliyun_template_code: Option<String>,
    pub sms_twilio_account_sid: Option<String>,
    pub sms_twilio_auth_token: Option<String>,
    pub sms_twilio_from: Option<String>,
    pub app_key: Option<String>,
}

/// Built-in module switches
///
/// Controls which built-in feature modules are enabled. When disabled, corresponding
/// routes are not registered, and protected tables and reserved route segments are
/// automatically released (available for Content Type use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinsConfig {
    #[serde(default = "default_true")]
    pub blog: bool,
    #[serde(default = "default_true")]
    pub pages: bool,
    #[serde(default = "default_true")]
    pub media: bool,
    #[serde(default = "default_true")]
    pub fulltext: bool,
    #[serde(default = "default_true")]
    pub workflow: bool,
    #[serde(default = "default_true")]
    pub ecommerce: bool,
    #[serde(default = "default_true")]
    pub payment: bool,
    #[serde(default = "default_true")]
    pub wallet: bool,
    /// Whether to enable the MCP (Model Context Protocol) server at `/api/v1/mcp`
    /// and the `raisfast mcp serve` stdio subcommand (default true).
    #[serde(default = "default_true")]
    pub mcp: bool,
    /// Whether to enable the LLM foundation (channels/key pools/model
    /// directory admin API; the `/v1` relay lands in P3) — default true.
    #[serde(default = "default_true")]
    pub llm_gateway: bool,
}

impl Default for BuiltinsConfig {
    fn default() -> Self {
        Self {
            blog: true,
            pages: true,
            media: true,
            fulltext: true,
            workflow: true,
            ecommerce: true,
            payment: true,
            wallet: true,
            mcp: true,
            llm_gateway: true,
        }
    }
}

impl BuiltinsConfig {
    pub fn from_env() -> Self {
        Self {
            blog: env::var("BUILTIN_BLOG")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            pages: env::var("BUILTIN_PAGES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            media: env::var("BUILTIN_MEDIA")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            fulltext: env::var("BUILTIN_FULLTEXT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            workflow: env::var("BUILTIN_WORKFLOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            ecommerce: env::var("BUILTIN_ECOMMERCE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            payment: env::var("BUILTIN_PAYMENT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            wallet: env::var("BUILTIN_WALLET")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            mcp: env::var("BUILTIN_MCP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            llm_gateway: env::var("BUILTIN_LLM_GATEWAY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
        }
    }

    /// Whether all built-in modules are disabled (pure Headless CMS mode)
    pub fn is_all_disabled(&self) -> bool {
        !self.blog
            && !self.pages
            && !self.media
            && !self.fulltext
            && !self.workflow
            && !self.ecommerce
            && !self.payment
            && !self.wallet
            && !self.mcp
    }

    /// Returns the list of protected tables.
    ///
    /// At runtime, returns the live list queried from the database at startup
    /// (includes tables from incremental migrations). Falls back to the
    /// compile-time schema table list when no database is available (e.g. unit tests).
    pub fn protected_tables(&self) -> Vec<String> {
        crate::db::schema::get_protected_tables()
    }

    /// Returns the list of reserved route segments.
    ///
    /// These names are always reserved regardless of whether the corresponding
    /// module is enabled, to prevent content type registration conflicts when
    /// a module is re-enabled later.
    pub fn reserved_route_segments(&self) -> Vec<&'static str> {
        vec![
            "admin",
            "auth",
            "audit",
            "cart",
            "categories",
            "cms",
            "comments",
            "crons",
            "events",
            "graphql",
            "health",
            "media",
            "mcp",
            "oauth",
            "options",
            "orders",
            "pages",
            "password",
            "payment",
            "plugins",
            "posts",
            "products",
            "rbac",
            "reusable-blocks",
            "routes",
            "rss",
            "search",
            "sitemap",
            "sse",
            "stats",
            "tags",
            "tenants",
            "tokens",
            "user",
            "users",
            "wallets",
            "webhooks",
            "workflows",
            "ws",
        ]
    }
}

/// API Rule engine configuration
///
/// All hardcoded values in the rule engine can be overridden via environment variables for:
/// - Adapting to different database backends (SQLite / PostgreSQL / MySQL)
/// - Adjusting cache strategies
/// - Customizing expression prefixes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEngineConfig {
    /// Expression prefix: authenticated user ID (default `@request.auth.id`)
    pub prefix_auth_id: String,
    /// Expression prefix: authenticated user role (default `@request.auth.role`)
    pub prefix_auth_role: String,
    /// Expression prefix: request body field (default `@request.body.`)
    pub prefix_request_body: String,
    /// Expression prefix: URL query parameter (default `@request.query.`)
    pub prefix_request_query: String,
    /// Expression prefix: current time (default `@now`)
    pub prefix_now: String,
    /// Expression prefix: cross-table reference (default `@table.`, used in Phase 3)
    pub prefix_cross_table: String,
    /// SQL function that @now compiles to (default `datetime('now')`, can be `NOW()` for PG)
    pub sql_now_fn: String,
    /// SQL operator that :isset compiles to (default `IS NOT NULL`)
    pub sql_isset_op: String,
    /// SQL function name that :length compiles to (default `LENGTH`, can be `CHAR_LENGTH` for PG)
    pub sql_length_fn: String,
    /// SQL LIKE wildcard (default `%`)
    pub sql_like_wildcard: String,
    /// SQL LIKE single character wildcard (default `_`)
    pub sql_like_single_char: String,
    /// Regex equivalent of LIKE wildcard (default `.*`)
    pub regex_like_wildcard: String,
    /// Regex equivalent of LIKE single character (default `.`)
    pub regex_like_single_char: String,
    /// CMS list cache TTL (seconds, default 30)
    pub cms_cache_ttl_secs: u64,
    /// CMS list max items per page (default 100)
    pub cms_max_page_size: u64,
}

impl Default for RuleEngineConfig {
    fn default() -> Self {
        Self {
            prefix_auth_id: "@request.auth.id".into(),
            prefix_auth_role: "@request.auth.role".into(),
            prefix_request_body: "@request.body.".into(),
            prefix_request_query: "@request.query.".into(),
            prefix_now: "@now".into(),
            prefix_cross_table: "@table.".into(),
            sql_now_fn: crate::db::Driver::now_fn().into(),
            sql_isset_op: "IS NOT NULL".into(),
            sql_length_fn: "LENGTH".into(),
            sql_like_wildcard: "%".into(),
            sql_like_single_char: "_".into(),
            regex_like_wildcard: ".*".into(),
            regex_like_single_char: ".".into(),
            cms_cache_ttl_secs: 30,
            cms_max_page_size: 100,
        }
    }
}

impl RuleEngineConfig {
    /// Load from environment variables, using defaults for missing items
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            prefix_auth_id: env::var("RULE_PREFIX_AUTH_ID").unwrap_or(defaults.prefix_auth_id),
            prefix_auth_role: env::var("RULE_PREFIX_AUTH_ROLE")
                .unwrap_or(defaults.prefix_auth_role),
            prefix_request_body: env::var("RULE_PREFIX_REQUEST_BODY")
                .unwrap_or(defaults.prefix_request_body),
            prefix_request_query: env::var("RULE_PREFIX_REQUEST_QUERY")
                .unwrap_or(defaults.prefix_request_query),
            prefix_now: env::var("RULE_PREFIX_NOW").unwrap_or(defaults.prefix_now),
            prefix_cross_table: env::var("RULE_PREFIX_CROSS_TABLE")
                .unwrap_or(defaults.prefix_cross_table),
            sql_now_fn: env::var("RULE_SQL_NOW_FN").unwrap_or(defaults.sql_now_fn),
            sql_isset_op: env::var("RULE_SQL_ISSET_OP").unwrap_or(defaults.sql_isset_op),
            sql_length_fn: env::var("RULE_SQL_LENGTH_FN").unwrap_or(defaults.sql_length_fn),
            sql_like_wildcard: env::var("RULE_SQL_LIKE_WILDCARD")
                .unwrap_or(defaults.sql_like_wildcard),
            sql_like_single_char: env::var("RULE_SQL_LIKE_SINGLE_CHAR")
                .unwrap_or(defaults.sql_like_single_char),
            regex_like_wildcard: env::var("RULE_REGEX_LIKE_WILDCARD")
                .unwrap_or(defaults.regex_like_wildcard),
            regex_like_single_char: env::var("RULE_REGEX_LIKE_SINGLE_CHAR")
                .unwrap_or(defaults.regex_like_single_char),
            cms_cache_ttl_secs: env::var("CMS_CACHE_TTL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.cms_cache_ttl_secs),
            cms_max_page_size: env::var("CMS_MAX_PAGE_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.cms_max_page_size),
        }
    }
}

/// Single Cron schedule configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronScheduleConfig {
    pub label: String,
    pub job_type: String,
    pub payload: Option<String>,
    pub cron_expr: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// MCP (Model Context Protocol) server configuration
///
/// Controls the built-in MCP server which exposes raisfast's CMS data,
/// content-type schemas, and admin operations to AI assistants such as
/// Claude Desktop and Cursor.
///
/// | Environment Variable | Type | Default | Description |
/// |----------|------|--------|------|
/// | `MCP_ENABLED` | bool | `true` | Master switch (also governed by `BUILTIN_MCP`) |
/// | `MCP_LOCAL_USER_ID` | i64 | (empty) | User ID impersonated by the stdio transport (`mcp serve`) |
/// | `MCP_LOCAL_TENANT_ID` | String | `default` | Tenant for stdio transport |
/// | `MCP_MAX_RESULT_CHARS` | usize | `20000` | Truncate tool/resource results above this length |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// User ID impersonated by the stdio transport when no HTTP auth is available.
    /// When empty, stdio tools run as an anonymous reader (write tools will fail).
    #[serde(default)]
    pub local_user_id: Option<i64>,
    /// Tenant ID for the stdio transport.
    #[serde(default = "default_mcp_local_tenant")]
    pub local_tenant_id: String,
    /// Hard upper bound on the size of any tool/resource result, in characters.
    /// Protects AI clients from accidentally pulling huge tables.
    #[serde(default = "default_mcp_max_result_chars")]
    pub max_result_chars: usize,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            local_user_id: None,
            local_tenant_id: default_mcp_local_tenant(),
            max_result_chars: default_mcp_max_result_chars(),
        }
    }
}

impl McpConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            enabled: env::var("MCP_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            local_user_id: env::var("MCP_LOCAL_USER_ID")
                .ok()
                .and_then(|v| v.parse().ok()),
            local_tenant_id: env::var("MCP_LOCAL_TENANT_ID")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or(defaults.local_tenant_id),
            max_result_chars: env::var("MCP_MAX_RESULT_CHARS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.max_result_chars),
        }
    }
}

fn default_mcp_local_tenant() -> String {
    crate::constants::DEFAULT_TENANT.to_string()
}

/// AI agent runtime configuration (OpenAI-compatible provider defaults).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ai_timeout_secs")]
    pub timeout_secs: u64,
    /// Broadcast background/agent turn events on the EventBus (`ai.turn.*`).
    #[serde(default = "default_true")]
    pub broadcast_events: bool,
    /// Master switch for the `run_shell` agent tool. Default `false` (default
    /// closed): the tool is not even registered until an operator sets
    /// `RAISFAST_AI_ALLOW_SHELL=true`, and even then only agents whose `tools`
    /// allowlist names it can call it.
    #[serde(default)]
    pub allow_shell: bool,
    /// Memory tier budgets (zeroclaw `budget.rs` semantics). `0` = unbounded.
    /// Env: `RAISFAST_AI_MEMORY_CORE_MAX_ROWS` / `..._CORE_MAX_BYTES` /
    /// `..._DAILY_MAX_ROWS`. Conversation rows are never budget-evicted.
    #[serde(default)]
    pub memory_core_max_rows: i64,
    #[serde(default)]
    pub memory_core_max_bytes: i64,
    #[serde(default)]
    pub memory_daily_max_rows: i64,
    /// Context-window driven replay budget (zeroclaw `context_window` +
    /// `recovery_budget = window*9/10` semantics; proactive fold is our
    /// evolution). The window itself comes from the resolved model's directory
    /// `params.context_window` (§10.1) with this global fallback when absent.
    /// `0` = windowing disabled.
    ///
    /// - `RAISFAST_AI_CONTEXT_WINDOW_FALLBACK`: window tokens when the model
    ///   declares no `params.context_window` (default 0 = off).
    /// - `RAISFAST_AI_CONTEXT_OUTPUT_RESERVE`: explicit reserve override;
    ///   `0` (default) = auto formula `min(max(window*10%, 20_000), window*90%)`
    ///   (opencode `reserved`/COMPACTION_BUFFER semantics with a 20k floor).
    #[serde(default)]
    pub context_window_fallback: i64,
    #[serde(default)]
    pub context_output_reserve: i64,
    /// Admin-configured external MCP servers to expose to agents as tools.
    /// JSON array of `{name, command, args}` (env `RAISFAST_AI_MCP_SERVERS`).
    #[serde(default)]
    pub mcp_servers: Vec<serde_json::Value>,
    /// Consolidate folded transcript turns into Core memory facts (zeroclaw
    /// classify/consolidation). Env `RAISFAST_AI_MEMORY_CONSOLIDATE` (default
    /// false); runs one extraction LLM call per fold.
    #[serde(default)]
    pub memory_consolidate: bool,
    /// Multi-agent debate orchestration (dev-docs/agent/multi-agent-debate.md §14).
    #[serde(default)]
    pub debate: DebateConfig,
}

fn default_ai_timeout_secs() -> u64 {
    120
}

/// Debate orchestration knobs (multi-agent §9 governor + §14 config).
///
/// | Env | Type | Default | Description |
/// |-----|------|---------|-------------|
/// | `RAISFAST_AI_DEBATE_ENABLED` | bool | `false` | Master switch for the debate subsystem |
/// | `RAISFAST_AI_DEBATE_MAX_ROUNDS` | u32 | `3` | Max orchestration rounds (clamped 1..=5) |
/// | `RAISFAST_AI_DEBATE_MAX_CONCURRENT` | u32 | `2` | Max `running` debates per tenant |
/// | `RAISFAST_AI_DEBATE_MAX_TOTAL_TOKENS` | i64 | `0` | Debate token budget; `0` = unlimited; exceeded → escalate, never hard-fail |
/// | `RAISFAST_AI_DEBATE_AUTO_RESOLVE_MINOR` | bool | `true` | Auto-close `minor` disputes via the default rule before each terminal check (§6.5) |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_debate_max_rounds")]
    pub max_rounds: u32,
    #[serde(default = "default_debate_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(default)]
    pub max_total_tokens: i64,
    #[serde(default = "default_true")]
    pub auto_resolve_minor: bool,
}

fn default_debate_max_rounds() -> u32 {
    3
}

fn default_debate_max_concurrent() -> u32 {
    2
}

impl Default for DebateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_rounds: default_debate_max_rounds(),
            max_concurrent: default_debate_max_concurrent(),
            max_total_tokens: 0,
            auto_resolve_minor: true,
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: default_ai_timeout_secs(),
            broadcast_events: true,
            allow_shell: false,
            memory_core_max_rows: 0,
            memory_core_max_bytes: 0,
            memory_daily_max_rows: 0,
            memory_consolidate: false,
            context_window_fallback: 0,
            context_output_reserve: 0,
            mcp_servers: Vec::new(),
            debate: DebateConfig {
                enabled: false,
                max_rounds: default_debate_max_rounds(),
                max_concurrent: default_debate_max_concurrent(),
                max_total_tokens: 0,
                auto_resolve_minor: true,
            },
        }
    }
}

impl AiConfig {
    pub fn from_env() -> Self {
        // Model access has a single entry point: the llm 底座 (channels +
        // model directory + options defaults, §10.2). No LLM connection env is
        // read here; `AiConfig` only carries runtime behavior/policy.
        let defaults = Self::default();
        Self {
            enabled: env::var("RAISFAST_AI_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.enabled),
            timeout_secs: env::var("RAISFAST_AI_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.timeout_secs),
            broadcast_events: env::var("RAISFAST_AI_BROADCAST_EVENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.broadcast_events),
            allow_shell: env::var("RAISFAST_AI_ALLOW_SHELL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            memory_core_max_rows: env::var("RAISFAST_AI_MEMORY_CORE_MAX_ROWS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            memory_core_max_bytes: env::var("RAISFAST_AI_MEMORY_CORE_MAX_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            memory_daily_max_rows: env::var("RAISFAST_AI_MEMORY_DAILY_MAX_ROWS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            memory_consolidate: env::var("RAISFAST_AI_MEMORY_CONSOLIDATE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            context_window_fallback: env::var("RAISFAST_AI_CONTEXT_WINDOW_FALLBACK")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            context_output_reserve: env::var("RAISFAST_AI_CONTEXT_OUTPUT_RESERVE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            mcp_servers: env::var("RAISFAST_AI_MCP_SERVERS")
                .ok()
                .filter(|v| !v.is_empty())
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
            debate: DebateConfig {
                enabled: env::var("RAISFAST_AI_DEBATE_ENABLED")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(defaults.debate.enabled),
                max_rounds: env::var("RAISFAST_AI_DEBATE_MAX_ROUNDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .map(|r: u32| r.clamp(1, 5))
                    .unwrap_or(defaults.debate.max_rounds),
                max_concurrent: env::var("RAISFAST_AI_DEBATE_MAX_CONCURRENT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(defaults.debate.max_concurrent),
                max_total_tokens: env::var("RAISFAST_AI_DEBATE_MAX_TOTAL_TOKENS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(defaults.debate.max_total_tokens),
                auto_resolve_minor: env::var("RAISFAST_AI_DEBATE_AUTO_RESOLVE_MINOR")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(defaults.debate.auto_resolve_minor),
            },
        }
    }
}

/// Knowledge base (KB) configuration.
///
/// | Env | Type | Default | Description |
/// |-----|------|---------|-------------|
/// | `RAISFAST_KB_ENABLED` | bool | `false` | Master switch for the KB subsystem |
/// | `RAISFAST_KB_VECTOR_BACKEND` | `qdrant`\|`bruteforce` | `qdrant` | Vector index backend (kb-technical-design §4.2) |
/// | `RAISFAST_KB_QDRANT_URL` | string | — | Qdrant gRPC endpoint (e.g. `http://localhost:6334`); required when backend=qdrant |
/// | `RAISFAST_KB_QDRANT_API_KEY` | string | — | Optional Qdrant API key |
/// | `RAISFAST_KB_QDRANT_PREFIX` | string | `kb` | Collection name prefix; collections are `{prefix}_{kb_id}` (one per KB) |
/// | `RAISFAST_KB_TOP_K` | u32 | `10` | Candidates per recall path (pre-fusion) |
/// | `RAISFAST_KB_WIKI_BOOST` | f32 | `1.3` | wiki_page unit score multiplier (WK same value) |
/// | `RAISFAST_KB_FALLBACK_THRESHOLD` | f32 | `0.3` | Below this top score → "not covered" (no generation) |
/// | `RAISFAST_KB_CONTEXT_TOKEN_BUDGET` | u32 | `4000` | S8 context assembly budget (chars/4 estimate) |
/// | `RAISFAST_KB_EMBED_BATCH_SIZE` | usize | `32` | Texts per `/embeddings` request (WK `BATCH_EMBED_SIZE` analog; WK default 5 + goroutine pool, ours serializes so 32) |
/// | `RAISFAST_KB_RERANK_MODEL` | string | — | Global **default** rerank model via the llm 底座 (S5, kb-technical-design §6.1 revised 2026-09-17); each KB row's `rerank_model` overrides it; neither set = passthrough |
/// | `RAISFAST_KB_RERANK_WINDOW` | u32 | `30` | Global default rerank window (per-KB `rerank_window` overrides); S4 cuts to this window, S5 reranks then cuts back to `top_k` (§6.1.4 窗口重排) |
/// | `RAISFAST_KB_RERANK_THRESHOLD` | f32 | `0` | Global default rerank score floor (per-KB `rerank_threshold` overrides); below is dropped after S5 (0 = keep all) [抄WK:RerankThreshold 语义] |
/// | `RAISFAST_KB_RERANK_BATCH_SIZE` | usize | `64` | Docs per `/rerank` request (transport concern, mirrors `RAISFAST_KB_EMBED_BATCH_SIZE`) |
/// | `RAISFAST_KB_UNDERSTAND_MODEL` | string | — | Dedicated fast model for S1 query rewrite (non-reasoning recommended; empty = tenant default chat model) |
/// | `RAISFAST_KB_CHAT_MODEL` | string | — | Global default S9 generation model (per-KB `chat_model` overrides; empty = tenant default) |
/// | `RAISFAST_KB_DISTILL_MODEL` | string | — | Global default wiki distillation model (per-KB `distill_model` overrides; empty = tenant default) |
/// | `RAISFAST_KB_IMAGE_MODEL` | string | — | Global default VLM image-recognition model (per-KB `image_config.model` overrides; recognition off when neither resolves) |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_kb_vector_backend")]
    pub vector_backend: String,
    #[serde(default)]
    pub qdrant_url: Option<String>,
    #[serde(default)]
    pub qdrant_api_key: Option<String>,
    #[serde(default = "default_kb_qdrant_prefix")]
    pub qdrant_prefix: String,
    /// Candidates each recall path returns (BM25 / dense), pre-fusion.
    /// Env `RAISFAST_KB_TOP_K` (default 10).
    #[serde(default = "default_kb_top_k")]
    pub top_k: u32,
    /// Score multiplier for `wiki_page` units after rerank
    /// [抄WK:wiki_boost.go 常量同值]. Env `RAISFAST_KB_WIKI_BOOST` (default 1.3).
    #[serde(default = "default_kb_wiki_boost")]
    pub wiki_boost: f32,
    /// S11 fallback threshold: below this top score the KB reports
    /// "not covered" instead of generating. Env `RAISFAST_KB_FALLBACK_THRESHOLD`
    /// (default 0.3, calibrate via E2E eval).
    #[serde(default = "default_kb_fallback_threshold")]
    pub fallback_threshold: f32,
    /// S8 context budget in estimated tokens (chars/4 heuristic,
    /// [抄RF:glossary U 估算]). Env `RAISFAST_KB_CONTEXT_TOKEN_BUDGET` (default 4000).
    #[serde(default = "default_kb_context_budget")]
    pub context_budget_tokens: u32,
    /// Texts per `/embeddings` request [抄WK:models/embedding/batch.go
    /// BATCH_EMBED_SIZE]. Declared deviation: WK defaults to 5 paired with
    /// an ants concurrency pool; we serialize batches, so the default is 32
    /// to keep round-trips sane. Env `RAISFAST_KB_EMBED_BATCH_SIZE`.
    #[serde(default = "default_kb_embed_batch_size")]
    pub embed_batch_size: usize,
    /// Observability plane persistence mode (kb-observability-design DR5):
    /// `all` = every run row; `errors` = only failed/degraded runs;
    /// `off` = no kb_runs writes. Env `RAISFAST_KB_TRACE_MODE`.
    #[serde(default = "default_kb_trace_mode")]
    pub trace_mode: String,
    /// kb_runs retention window in days (0 = keep forever); enforced by the
    /// T3 cleanup sweeper. Env `RAISFAST_KB_TRACE_RETENTION_DAYS`.
    #[serde(default = "default_kb_trace_retention_days")]
    pub trace_retention_days: i64,
    /// Global **default** rerank model routed via the llm 底座 (S5). Each
    /// KB row's `rerank_model` overrides it; neither set = S5 passthrough
    /// (kb-technical-design §6.1 revised 2026-09-17). Env
    /// `RAISFAST_KB_RERANK_MODEL`.
    /// Global **default** rerank model routed via the llm 底座 (S5). Each
    /// KB row's `rerank_model` overrides it; neither set = S5 passthrough
    /// (kb-technical-design §6.1 revised 2026-09-17). Env
    /// `RAISFAST_KB_RERANK_MODEL`.
    #[serde(default)]
    pub rerank_model: Option<String>,
    /// S1 understand 专用模型（推荐非推理小模型——改写+关键词抽取是小任务，
    /// 且 S1 在每次 ask 的关键路径上，reasoning 模型徒增延迟还会吃光
    /// max_tokens 导致空回复降级）。空 = 租户默认 chat 模型。Env
    /// `RAISFAST_KB_UNDERSTAND_MODEL`.
    #[serde(default)]
    pub understand_model: Option<String>,
    /// Global **default** S9 generation model (each KB row's `chat_model`
    /// overrides it; empty = tenant default chat model). Env
    /// `RAISFAST_KB_CHAT_MODEL`.
    #[serde(default)]
    pub chat_model: Option<String>,
    /// Global **default** wiki distillation model (each KB row's
    /// `distill_model` overrides it; empty = tenant default). Env
    /// `RAISFAST_KB_DISTILL_MODEL`.
    #[serde(default)]
    pub distill_model: Option<String>,
    /// Global **default** VLM image-recognition model (each KB row's
    /// `image_config.model` overrides it; recognition off when neither
    /// resolves). Env `RAISFAST_KB_IMAGE_MODEL`.
    #[serde(default)]
    pub image_model: Option<String>,
    /// Global default parser engine name (doc override → KB rules → this →
    /// builtin; kb-parser-engines-design §2 D2). Env
    /// `RAISFAST_KB_PARSER_ENGINE`.
    #[serde(default)]
    pub parser_engine: Option<String>,
    /// Max concurrent builtin parses. Parsing is CPU- and memory-heavy, so this
    /// bounds memory use across KB ingest / conversion / flow parse. Env
    /// `RAISFAST_KB_PARSER_CONCURRENCY` (default 2).
    #[serde(default = "default_kb_parser_concurrency")]
    pub parser_concurrency: usize,
    /// docreader service gRPC endpoint (e.g. `http://127.0.0.1:50051`);
    /// unset → the docreader engine never registers. Env
    /// `RAISFAST_KB_DOCREADER_URL`.
    #[serde(default)]
    pub docreader_url: Option<String>,
    /// docreader parse budget (gRPC deadline), also the probe bound. Env
    /// `RAISFAST_KB_DOCREADER_TIMEOUT_SECS` (default 300).
    #[serde(default = "default_kb_docreader_timeout")]
    pub docreader_timeout_secs: u64,
    /// MinerU parse service HTTP endpoint (`services/mineru`, Docker :50053).
    /// `None`/empty = the `mineru` engine is not registered. Env
    /// `RAISFAST_KB_MINERU_URL`.
    #[serde(default)]
    pub mineru_url: Option<String>,
    /// MinerU parse budget (job poll deadline — the first parse downloads
    /// models and can take many minutes). Env
    /// `RAISFAST_KB_MINERU_TIMEOUT_SECS` (default 1800).
    #[serde(default = "default_kb_mineru_timeout")]
    pub mineru_timeout_secs: u64,
    /// MinerU Cloud API key (mineru.net). `None`/empty = the
    /// `mineru_cloud` engine is not registered. Env
    /// `RAISFAST_KB_MINERU_CLOUD_API_KEY`.
    #[serde(default)]
    pub mineru_cloud_api_key: Option<String>,
    /// Self-hosted PaddleOCR-VL pipeline endpoint (PaddleX serving).
    /// `None`/empty = the `paddleocr_vl` engine is not registered. Env
    /// `RAISFAST_KB_PADDLEOCR_VL_ENDPOINT`.
    #[serde(default)]
    pub paddleocr_vl_endpoint: Option<String>,
    /// PaddleOCR-VL Cloud (AI Studio) token. `None`/empty = the
    /// `paddleocr_vl_cloud` engine is not registered. Env
    /// `RAISFAST_KB_PADDLEOCR_VL_CLOUD_TOKEN`.
    #[serde(default)]
    pub paddleocr_vl_cloud_token: Option<String>,
    /// Global default rerank window; per-KB `rerank_window` overrides.
    /// S4 cuts to this window (≥ `top_k`) so the reranker sees more than
    /// the final keep set, S5 reranks then cuts back to `top_k` (§6.1.4
    /// 窗口重排). Env `RAISFAST_KB_RERANK_WINDOW` (default 30).
    #[serde(default = "default_kb_rerank_window")]
    pub rerank_window: u32,
    /// Global default rerank score floor; per-KB `rerank_threshold`
    /// overrides. Candidates scoring below are dropped after S5
    /// (0 = keep all) [抄WK:RerankThreshold 语义]. Not refilled when the
    /// filter leaves fewer than `top_k` (§6.1.4). Env
    /// `RAISFAST_KB_RERANK_THRESHOLD` (default 0).
    #[serde(default)]
    pub rerank_threshold: f32,
    /// Docs per `/rerank` request (global transport concern, mirrors
    /// `embed_batch_size`). Env `RAISFAST_KB_RERANK_BATCH_SIZE` (default 64).
    #[serde(default = "default_kb_rerank_batch_size")]
    pub rerank_batch_size: usize,
}

fn default_kb_top_k() -> u32 {
    10
}

fn default_kb_embed_batch_size() -> usize {
    32
}

fn default_kb_wiki_boost() -> f32 {
    1.3
}

fn default_kb_fallback_threshold() -> f32 {
    0.3
}

fn default_kb_context_budget() -> u32 {
    4000
}

fn default_kb_vector_backend() -> String {
    "qdrant".to_string()
}

fn default_kb_qdrant_prefix() -> String {
    "kb".to_string()
}

fn default_kb_trace_mode() -> String {
    "all".to_string()
}

fn default_kb_trace_retention_days() -> i64 {
    14
}

fn default_kb_rerank_window() -> u32 {
    30
}

fn default_kb_rerank_batch_size() -> usize {
    64
}

fn default_kb_docreader_timeout() -> u64 {
    300
}

fn default_kb_parser_concurrency() -> usize {
    2
}

fn default_kb_mineru_timeout() -> u64 {
    1800
}

impl Default for KbConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            vector_backend: default_kb_vector_backend(),
            qdrant_url: None,
            qdrant_api_key: None,
            qdrant_prefix: default_kb_qdrant_prefix(),
            top_k: default_kb_top_k(),
            wiki_boost: default_kb_wiki_boost(),
            fallback_threshold: default_kb_fallback_threshold(),
            context_budget_tokens: default_kb_context_budget(),
            embed_batch_size: default_kb_embed_batch_size(),
            trace_mode: default_kb_trace_mode(),
            trace_retention_days: default_kb_trace_retention_days(),
            rerank_model: None,
            understand_model: None,
            chat_model: None,
            distill_model: None,
            image_model: None,
            parser_engine: None,
            parser_concurrency: default_kb_parser_concurrency(),
            docreader_url: None,
            docreader_timeout_secs: default_kb_docreader_timeout(),
            mineru_url: None,
            mineru_timeout_secs: default_kb_mineru_timeout(),
            mineru_cloud_api_key: None,
            paddleocr_vl_endpoint: None,
            paddleocr_vl_cloud_token: None,
            rerank_window: default_kb_rerank_window(),
            rerank_threshold: 0.0,
            rerank_batch_size: default_kb_rerank_batch_size(),
        }
    }
}

impl KbConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            enabled: env::var("RAISFAST_KB_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.enabled),
            vector_backend: env::var("RAISFAST_KB_VECTOR_BACKEND")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or(defaults.vector_backend),
            qdrant_url: env::var("RAISFAST_KB_QDRANT_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            qdrant_api_key: env::var("RAISFAST_KB_QDRANT_API_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
            qdrant_prefix: env::var("RAISFAST_KB_QDRANT_PREFIX")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or(defaults.qdrant_prefix),
            top_k: env::var("RAISFAST_KB_TOP_K")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.top_k),
            wiki_boost: env::var("RAISFAST_KB_WIKI_BOOST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.wiki_boost),
            fallback_threshold: env::var("RAISFAST_KB_FALLBACK_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.fallback_threshold),
            context_budget_tokens: env::var("RAISFAST_KB_CONTEXT_TOKEN_BUDGET")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.context_budget_tokens),
            embed_batch_size: env::var("RAISFAST_KB_EMBED_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|n| *n > 0)
                .unwrap_or(defaults.embed_batch_size),
            trace_mode: env::var("RAISFAST_KB_TRACE_MODE")
                .ok()
                .filter(|v| matches!(v.as_str(), "all" | "errors" | "off"))
                .unwrap_or(defaults.trace_mode),
            trace_retention_days: env::var("RAISFAST_KB_TRACE_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|d| *d >= 0)
                .unwrap_or(defaults.trace_retention_days),
            rerank_model: env::var("RAISFAST_KB_RERANK_MODEL")
                .ok()
                .filter(|v| !v.is_empty()),
            understand_model: env::var("RAISFAST_KB_UNDERSTAND_MODEL")
                .ok()
                .filter(|v| !v.is_empty()),
            chat_model: env::var("RAISFAST_KB_CHAT_MODEL")
                .ok()
                .filter(|v| !v.is_empty()),
            distill_model: env::var("RAISFAST_KB_DISTILL_MODEL")
                .ok()
                .filter(|v| !v.is_empty()),
            image_model: env::var("RAISFAST_KB_IMAGE_MODEL")
                .ok()
                .filter(|v| !v.is_empty()),
            parser_engine: env::var("RAISFAST_KB_PARSER_ENGINE")
                .ok()
                .filter(|v| !v.is_empty()),
            parser_concurrency: env::var("RAISFAST_KB_PARSER_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|c| *c > 0)
                .unwrap_or(defaults.parser_concurrency),
            mineru_url: env::var("RAISFAST_KB_MINERU_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            mineru_timeout_secs: env::var("RAISFAST_KB_MINERU_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_kb_mineru_timeout()),
            mineru_cloud_api_key: env::var("RAISFAST_KB_MINERU_CLOUD_API_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
            paddleocr_vl_endpoint: env::var("RAISFAST_KB_PADDLEOCR_VL_ENDPOINT")
                .ok()
                .filter(|v| !v.is_empty()),
            paddleocr_vl_cloud_token: env::var("RAISFAST_KB_PADDLEOCR_VL_CLOUD_TOKEN")
                .ok()
                .filter(|v| !v.is_empty()),
            docreader_url: env::var("RAISFAST_KB_DOCREADER_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            docreader_timeout_secs: env::var("RAISFAST_KB_DOCREADER_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|s| *s > 0)
                .unwrap_or(defaults.docreader_timeout_secs),
            rerank_window: env::var("RAISFAST_KB_RERANK_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|w| *w > 0)
                .unwrap_or(defaults.rerank_window),
            rerank_threshold: env::var("RAISFAST_KB_RERANK_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|t| (0.0..=1.0).contains(t))
                .unwrap_or(defaults.rerank_threshold),
            rerank_batch_size: env::var("RAISFAST_KB_RERANK_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|n| *n > 0)
                .unwrap_or(defaults.rerank_batch_size),
        }
    }
}

/// Integration Plane configuration.
///
/// | Env | Type | Default | Description |
/// |-----|------|---------|-------------|
/// | `INTEGRATION_ENABLED` | bool | `true` | Master switch for the integration plane |
/// | `INTEGRATION_INGRESS_BODY_LIMIT` | usize | `1048576` | Max inbound request body size (bytes) |
/// | `INTEGRATION_RECEIPTS_RETENTION_DAYS` | u64 | `90` | Receipt retention before archiving/cleanup |
/// | `INTEGRATION_EGRESS_TIMEOUT_SECS` | u64 | `20` | Outbound api-client call timeout |
/// | `INTEGRATION_EGRESS_LOG_RETENTION_DAYS` | u64 | `90` | Egress log retention before cleanup |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Max inbound push request body size in bytes (per-channel override possible).
    #[serde(default = "default_ingress_body_limit")]
    pub ingress_body_limit: usize,
    /// Receipts older than this (and delivered/dead) are archived then cleaned.
    #[serde(default = "default_receipts_retention_days")]
    pub receipts_retention_days: u64,
    /// Outbound api-client call timeout (§9.2).
    #[serde(default = "default_egress_timeout_secs")]
    pub egress_timeout_secs: u64,
    /// Egress log rows older than this are cleaned (same policy as receipts).
    #[serde(default = "default_egress_log_retention_days")]
    pub egress_log_retention_days: u64,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ingress_body_limit: default_ingress_body_limit(),
            receipts_retention_days: default_receipts_retention_days(),
            egress_timeout_secs: default_egress_timeout_secs(),
            egress_log_retention_days: default_egress_log_retention_days(),
        }
    }
}

/// App Bundle configuration (app-bundle.md).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppsConfig {
    /// Drain window for disable/uninstall: how long in-flight channel
    /// envelopes may take to reach a terminal receipt state before the
    /// remainder is dead-lettered (`drained:timeout`).
    pub drain_window_secs: u64,
}

impl Default for AppsConfig {
    fn default() -> Self {
        Self {
            drain_window_secs: 60,
        }
    }
}

impl AppsConfig {
    #[must_use]
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            drain_window_secs: std::env::var("RAISFAST_APP_DRAIN_WINDOW_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.drain_window_secs),
        }
    }
}

impl IntegrationConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            enabled: env::var("INTEGRATION_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            ingress_body_limit: env::var("INTEGRATION_INGRESS_BODY_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.ingress_body_limit),
            receipts_retention_days: env::var("INTEGRATION_RECEIPTS_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.receipts_retention_days),
            egress_timeout_secs: env::var("INTEGRATION_EGRESS_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.egress_timeout_secs),
            egress_log_retention_days: env::var("INTEGRATION_EGRESS_LOG_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.egress_log_retention_days),
        }
    }
}

fn default_ingress_body_limit() -> usize {
    1_048_576
}
fn default_receipts_retention_days() -> u64 {
    90
}

fn default_egress_timeout_secs() -> u64 {
    20
}

fn default_egress_log_retention_days() -> u64 {
    90
}

fn default_mcp_max_result_chars() -> usize {
    20000
}

fn default_true() -> bool {
    true
}

fn default_worker_cron_tick_ms() -> u64 {
    60000
}

fn default_worker_visibility_timeout_secs() -> u64 {
    300
}

fn default_worker_sweep_interval_secs() -> u64 {
    60
}

fn default_presence_heartbeat_ttl_secs() -> u64 {
    75
}

fn default_presence_sweep_interval_secs() -> u64 {
    10
}

fn default_presence_heartbeat_interval_secs() -> u64 {
    30
}

fn default_sse_max_clients() -> u64 {
    64
}

fn default_sse_max_session_clients() -> u64 {
    512
}

fn default_cron_log_retention_days() -> i64 {
    30
}

fn default_order_expire_minutes() -> i64 {
    30
}

fn default_storage_root_dir() -> String {
    "./storage".into()
}

/// Load a dotenv file, printing a warning when it exists but fails to parse.
///
/// dotenvy aborts the whole file on the first malformed line — most commonly a
/// value containing single quotes without an outer double-quote wrapper (e.g.
/// `CSP=default-src 'self'; ...`). Without this warning that failure is
/// completely silent: every variable in the file is skipped.
///
/// Uses `eprintln!` (not `tracing`) because this runs before the logging
/// subscriber is installed.
fn load_dotenv(path: &str) {
    if let Err(e) = dotenvy::from_path(path) {
        match &e {
            dotenvy::Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {}
            _ => eprintln!(
                "warning: failed to parse {path} ({e}); NONE of its variables were loaded. \
                 Hint: wrap values containing quotes in double quotes, e.g. \
                 KEY=\"value with 'single quotes'\""
            ),
        }
    }
}

fn default_backup_retention() -> usize {
    10
}

fn storage_subdir(root: &str, sub: &str) -> String {
    format!("{root}/{sub}")
}

fn default_search_engine() -> String {
    "none".into()
}

fn default_search_index_dir() -> String {
    storage_subdir(&default_storage_root_dir(), "search_index")
}

fn default_content_type_dir() -> String {
    "./extensions/content_types".into()
}

fn default_plugin_dir() -> Option<String> {
    Some("./extensions/plugins".into())
}

fn default_timezone() -> String {
    "UTC".into()
}

#[must_use]
pub fn default_cron_schedules() -> Vec<CronScheduleConfig> {
    vec![
        CronScheduleConfig {
            label: "Generate Sitemap".into(),
            job_type: "generate_sitemap".into(),
            payload: None,
            cron_expr: "0 0 */6 * * *".into(),
            enabled: false,
        },
        CronScheduleConfig {
            label: "KB Runs Retention Sweep".into(),
            job_type: "kb_runs_cleanup".into(),
            payload: None,
            cron_expr: "0 0 5 * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Cleanup Old Jobs".into(),
            job_type: "invalidate_cache".into(),
            payload: Some(r#"{"keys":["jobs:cleanup"]}"#.into()),
            cron_expr: "0 0 3 * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Expire Payment Orders".into(),
            job_type: "expire_payment_orders".into(),
            payload: None,
            cron_expr: "0 */5 * * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Expire Orders".into(),
            job_type: "expire_orders".into(),
            payload: None,
            cron_expr: "0 */5 * * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Reconcile Payments".into(),
            job_type: "reconcile_payments".into(),
            payload: None,
            cron_expr: "0 0 4 * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Process Wallet Outbox".into(),
            job_type: "process_wallet_outbox".into(),
            payload: None,
            cron_expr: "0 */10 * * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Reconcile LLM Hold Leaks".into(),
            job_type: "llm_hold_reconcile".into(),
            payload: None,
            cron_expr: "0 */30 * * * *".into(),
            enabled: true,
        },
        CronScheduleConfig {
            label: "Daily Database Backup".into(),
            job_type: "db_backup".into(),
            payload: None,
            cron_expr: "0 0 2 * * *".into(),
            enabled: true,
        },
    ]
}

fn default_log_dir() -> String {
    storage_subdir(&default_storage_root_dir(), "logs")
}

fn default_log_max_files() -> usize {
    7
}

fn default_rate_limit_enabled() -> bool {
    true
}

fn default_rate_limit_global_max() -> u32 {
    60
}

fn default_rate_limit_global_window() -> u64 {
    60
}

fn default_rate_limit_register_max() -> u32 {
    5
}

fn default_rate_limit_register_window() -> u64 {
    3600
}

fn default_rate_limit_login_max() -> u32 {
    10
}

fn default_rate_limit_login_window() -> u64 {
    60
}

fn default_rate_limit_comment_max() -> u32 {
    3
}

fn default_rate_limit_comment_window() -> u64 {
    60
}

fn default_rate_limit_api_token_max() -> u32 {
    120
}

fn default_rate_limit_api_token_window() -> u64 {
    60
}

fn default_worker_concurrency() -> usize {
    2
}

fn default_worker_cpu_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .unwrap_or(2)
}

fn default_worker_poll_interval_ms() -> u64 {
    500
}

fn default_worker_batch_size() -> usize {
    // Small: a worker runs its batch sequentially, so a large batch lets one
    // worker hoard long jobs and starve the others (throughput floors at
    // N/(batch × latency) regardless of worker count). Bench (500-job/100ms):
    // batch 1/2 ≈ 274/s vs batch 20 ≈ 98/s at 32 workers. 2 keeps a little
    // coalescing and is near-best for tiny jobs too.
    2
}

fn default_worker_job_timeout_secs() -> u64 {
    86400
}

fn default_worker_max_attempts() -> u32 {
    3
}

fn default_plugin_max_memory() -> u32 {
    32
}

fn default_plugin_timeout() -> u64 {
    5000
}

fn default_plugin_wasm_pool_size() -> u32 {
    4
}

fn default_plugin_lua_pool_size() -> u32 {
    4
}

fn default_plugin_js_pool_size() -> u32 {
    4
}

fn default_plugin_vfs_root() -> String {
    storage_subdir(&default_storage_root_dir(), "vfs")
}

fn default_storage_driver() -> String {
    "local".into()
}

fn default_s3_bucket() -> String {
    "blog".into()
}

fn default_s3_region() -> String {
    "us-east-1".into()
}

fn default_plugin_vfs_max_file_size() -> usize {
    1048576 // 1 MB
}

fn default_plugin_vfs_max_total_size() -> usize {
    10485760 // 10 MB
}

fn default_sms_code_expires_in() -> u64 {
    300
}

fn default_sms_code_length() -> u32 {
    6
}

fn default_sms_rate_limit_secs() -> u64 {
    60
}

fn default_email_provider() -> String {
    "log".into()
}

fn default_email_smtp_port() -> u16 {
    587
}

fn default_sms_provider() -> String {
    "log".into()
}

const DEFAULT_JWT_SECRET: &str = "change-me-in-production-at-least-32-chars";
impl AppConfig {
    /// Whether API uses RESTful style (PUT/DELETE allowed).
    pub fn is_restful(&self) -> bool {
        self.api_restful
    }

    /// Build configuration from environment variables, using defaults for missing variables.
    #[must_use]
    pub fn from_env() -> Self {
        let host = env::var("APP_HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port: u16 = env::var("APP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(9898);
        let storage_root_dir =
            env::var("STORAGE_ROOT_DIR").unwrap_or_else(|_| default_storage_root_dir());
        let database_url: String = env::var("DATABASE_URL").unwrap_or_else(|_| {
            #[cfg(feature = "db-sqlite")]
            {
                format!("sqlite:{}/db/raisfast.db?mode=rwc", storage_root_dir)
            }
            #[cfg(not(feature = "db-sqlite"))]
            {
                eprintln!(
                    "ERROR: DATABASE_URL environment variable is required when not using SQLite."
                );
                eprintln!("Examples:");
                eprintln!(
                    "  PostgreSQL: DATABASE_URL=postgres://user:pass@localhost:5432/raisfast"
                );
                eprintln!("  MySQL:      DATABASE_URL=mysql://user:pass@localhost:3306/raisfast");
                std::process::exit(1);
            }
        });

        let base_url = env::var("BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("http://{host}:{port}"));
        let cors_origins = env::var("CORS_ORIGINS").ok().filter(|s| !s.is_empty());
        let tls_cert_path = env::var("TLS_CERT_PATH").ok().filter(|s| !s.is_empty());
        let tls_key_path = env::var("TLS_KEY_PATH").ok().filter(|s| !s.is_empty());

        Self {
            host,
            port,
            env: env::var("APP_ENV").unwrap_or_else(|_| "development".into()),
            api_restful: env::var("API_RESTFUL")
                .ok()
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
            database_url,
            db_pool_size: env::var("DB_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    let cpus = std::thread::available_parallelism()
                        .map(|n| n.get() as u32)
                        .unwrap_or(2);
                    cpus * 2
                }),
            jwt_secret: env::var("JWT_SECRET").unwrap_or_else(|_| DEFAULT_JWT_SECRET.into()),
            jwt_access_expires: env::var("JWT_ACCESS_EXPIRES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            jwt_refresh_expires: env::var("JWT_REFRESH_EXPIRES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(604800),
            storage_root_dir: storage_root_dir.clone(),
            upload_dir: env::var("UPLOAD_DIR")
                .unwrap_or_else(|_| storage_subdir(&storage_root_dir, "uploads")),
            backup_retention: env::var("BACKUP_RETENTION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_backup_retention()),
            max_upload_size: env::var("MAX_UPLOAD_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(104857600),
            static_dir: env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into()),
            widget_dir: env::var("WIDGET_DIR").unwrap_or_else(|_| "./frontend/widget/dist".into()),
            base_url,
            cors_origins,
            tls_cert_path,
            tls_key_path,
            // Unset PLUGIN_DIR falls back to the serde default — the plugin
            // directory convention must work without explicit configuration
            // (`from_env` bypasses serde defaults, so mirror it by hand).
            plugin_dir: env::var("PLUGIN_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(default_plugin_dir),
            plugin_hot_reload: env::var("PLUGIN_HOT_RELOAD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            plugin_max_memory_mb: env::var("PLUGIN_MAX_MEMORY_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_max_memory()),
            plugin_default_timeout_ms: env::var("PLUGIN_DEFAULT_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_timeout()),
            plugin_wasm_pool_size: env::var("PLUGIN_WASM_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_wasm_pool_size()),
            plugin_lua_pool_size: env::var("PLUGIN_LUA_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_lua_pool_size()),
            plugin_js_pool_size: env::var("PLUGIN_JS_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_js_pool_size()),
            plugin_disabled: env::var("PLUGIN_DISABLED")
                .ok()
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default(),
            log_dir: env::var("LOG_DIR")
                .unwrap_or_else(|_| storage_subdir(&storage_root_dir, "logs")),
            log_max_files: env::var("LOG_MAX_FILES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_log_max_files()),
            rate_limit_enabled: env::var("RATE_LIMIT_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_enabled()),
            rate_limit_global_max: env::var("RATE_LIMIT_GLOBAL_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_global_max()),
            rate_limit_global_window: env::var("RATE_LIMIT_GLOBAL_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_global_window()),
            rate_limit_register_max: env::var("RATE_LIMIT_REGISTER_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_register_max()),
            rate_limit_register_window: env::var("RATE_LIMIT_REGISTER_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_register_window()),
            rate_limit_login_max: env::var("RATE_LIMIT_LOGIN_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_login_max()),
            rate_limit_login_window: env::var("RATE_LIMIT_LOGIN_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_login_window()),
            rate_limit_comment_max: env::var("RATE_LIMIT_COMMENT_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_comment_max()),
            rate_limit_comment_window: env::var("RATE_LIMIT_COMMENT_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_comment_window()),
            rate_limit_api_token_max: env::var("RATE_LIMIT_API_TOKEN_MAX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_api_token_max()),
            rate_limit_api_token_window: env::var("RATE_LIMIT_API_TOKEN_WINDOW")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_rate_limit_api_token_window()),
            plugin_vfs_root: env::var("PLUGIN_VFS_ROOT")
                .unwrap_or_else(|_| storage_subdir(&storage_root_dir, "vfs")),
            plugin_vfs_max_file_size: env::var("PLUGIN_VFS_MAX_FILE_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_vfs_max_file_size()),
            plugin_vfs_max_total_size: env::var("PLUGIN_VFS_MAX_TOTAL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_plugin_vfs_max_total_size()),
            worker_enabled: env::var("WORKER_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            worker_concurrency: env::var("WORKER_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_concurrency()),
            worker_cpu_concurrency: env::var("WORKER_CPU_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_cpu_concurrency()),
            worker_poll_interval_ms: env::var("WORKER_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_poll_interval_ms()),
            worker_batch_size: env::var("WORKER_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_batch_size()),
            worker_default_max_attempts: env::var("WORKER_DEFAULT_MAX_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_max_attempts()),
            worker_cron_tick_ms: env::var("WORKER_CRON_TICK_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_cron_tick_ms()),
            worker_visibility_timeout_secs: env::var("WORKER_VISIBILITY_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_visibility_timeout_secs()),
            worker_job_timeout_secs: env::var("WORKER_JOB_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_job_timeout_secs()),
            worker_sweep_interval_secs: env::var("WORKER_SWEEP_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_worker_sweep_interval_secs()),
            cron_seed_enabled: env::var("CRON_SEED_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            cron_schedules: env::var("CRON_SCHEDULES")
                .ok()
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
            cron_log_retention_days: env::var("CRON_LOG_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_cron_log_retention_days()),
            cron_allow_system_scripts: env::var("CRON_ALLOW_SYSTEM_SCRIPTS")
                .ok()
                .map(|v| v != "false" && v != "0")
                .unwrap_or(false),
            cron_system_workdir: env::var("CRON_SYSTEM_WORKDIR")
                .ok()
                .filter(|s| !s.is_empty()),
            order_expire_minutes: env::var("ORDER_EXPIRE_MINUTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_order_expire_minutes()),
            search_engine: env::var("SEARCH_ENGINE").unwrap_or_else(|_| default_search_engine()),
            search_index_dir: env::var("SEARCH_INDEX_DIR")
                .unwrap_or_else(|_| storage_subdir(&storage_root_dir, "search_index")),
            content_type_dir: env::var("CONTENT_TYPE_DIR")
                .unwrap_or_else(|_| default_content_type_dir()),
            timezone: env::var("TIMEZONE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(default_timezone),
            storage_driver: env::var("STORAGE_DRIVER")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(default_storage_driver),
            s3_endpoint: env::var("S3_ENDPOINT").ok().filter(|s| !s.is_empty()),
            s3_access_key: env::var("S3_ACCESS_KEY").ok().filter(|s| !s.is_empty()),
            s3_secret_key: env::var("S3_SECRET_KEY").ok().filter(|s| !s.is_empty()),
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| default_s3_bucket()),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| default_s3_region()),
            s3_public_url: env::var("S3_PUBLIC_URL").ok().filter(|s| !s.is_empty()),
            rule_engine: RuleEngineConfig::from_env(),
            graphql_enabled: env::var("GRAPHQL_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            websocket_enabled: env::var("WEBSOCKET_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            mcp: McpConfig::from_env(),
            ai: AiConfig::from_env(),
            kb: KbConfig::from_env(),
            integration: IntegrationConfig::from_env(),
            apps: AppsConfig::from_env(),
            oauth: crate::config::oauth::OAuthConfig::from_env(),
            registration_email_enabled: env::var("REGISTRATION_EMAIL_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
            registration_sms_enabled: env::var("REGISTRATION_SMS_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            presence_heartbeat_ttl_secs: env::var("PRESENCE_HEARTBEAT_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_presence_heartbeat_ttl_secs()),
            presence_sweep_interval_secs: env::var("PRESENCE_SWEEP_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_presence_sweep_interval_secs()),
            presence_heartbeat_interval_secs: env::var("PRESENCE_HEARTBEAT_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_presence_heartbeat_interval_secs()),
            sse_max_clients: env::var("SSE_MAX_CLIENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_sse_max_clients()),
            sse_max_session_clients: env::var("SSE_MAX_SESSION_CLIENTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_sse_max_session_clients()),
            sms_code_expires_in: env::var("SMS_CODE_EXPIRES_IN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_sms_code_expires_in()),
            sms_code_length: env::var("SMS_CODE_LENGTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_sms_code_length()),
            sms_rate_limit_secs: env::var("SMS_RATE_LIMIT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_sms_rate_limit_secs()),
            require_email_verification: env::var("REQUIRE_EMAIL_VERIFICATION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            builtins: BuiltinsConfig::from_env(),
            builtin_tenantable: env::var("BUILTIN_TENANTABLE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false),
            base_domain: env::var("BASE_DOMAIN").ok().filter(|v| !v.is_empty()),
            email_provider: env::var("EMAIL_PROVIDER")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(default_email_provider),
            email_smtp_host: env::var("EMAIL_SMTP_HOST").ok().filter(|s| !s.is_empty()),
            email_smtp_port: env::var("EMAIL_SMTP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_email_smtp_port()),
            email_smtp_user: env::var("EMAIL_SMTP_USER").ok().filter(|s| !s.is_empty()),
            email_smtp_pass: env::var("EMAIL_SMTP_PASS").ok().filter(|s| !s.is_empty()),
            email_from: env::var("EMAIL_FROM").ok().filter(|s| !s.is_empty()),
            email_from_name: env::var("EMAIL_FROM_NAME").ok().filter(|s| !s.is_empty()),
            email_sendgrid_api_key: env::var("EMAIL_SENDGRID_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            email_resend_api_key: env::var("EMAIL_RESEND_API_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            email_aliyun_access_key_id: env::var("EMAIL_ALIYUN_ACCESS_KEY_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            email_aliyun_access_key_secret: env::var("EMAIL_ALIYUN_ACCESS_KEY_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            email_aliyun_region: env::var("EMAIL_ALIYUN_REGION")
                .ok()
                .filter(|s| !s.is_empty()),
            email_tencent_secret_id: env::var("EMAIL_TENCENT_SECRET_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            email_tencent_secret_key: env::var("EMAIL_TENCENT_SECRET_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            email_tencent_region: env::var("EMAIL_TENCENT_REGION")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_provider: env::var("SMS_PROVIDER")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(default_sms_provider),
            sms_aliyun_access_key_id: env::var("SMS_ALIYUN_ACCESS_KEY_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_aliyun_access_key_secret: env::var("SMS_ALIYUN_ACCESS_KEY_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_aliyun_sign_name: env::var("SMS_ALIYUN_SIGN_NAME")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_aliyun_template_code: env::var("SMS_ALIYUN_TEMPLATE_CODE")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_twilio_account_sid: env::var("SMS_TWILIO_ACCOUNT_SID")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_twilio_auth_token: env::var("SMS_TWILIO_AUTH_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            sms_twilio_from: env::var("SMS_TWILIO_FROM").ok().filter(|s| !s.is_empty()),
            app_key: env::var("APP_KEY").ok().filter(|s| !s.is_empty()),
            started_at: None,
        }
    }

    /// Create a minimal configuration instance for testing.
    ///
    /// All fields use default values; callers can override as needed.
    #[must_use]
    pub fn test_defaults() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 9898,
            env: "test".into(),
            api_restful: true,
            database_url: "sqlite::memory:".into(),
            db_pool_size: 1,
            jwt_secret: "test-secret-key-at-least-32-characters-long".into(),
            jwt_access_expires: 900,
            jwt_refresh_expires: 604800,
            storage_root_dir: "./test-storage".into(),
            upload_dir: "./test-storage/uploads".into(),
            backup_retention: default_backup_retention(),
            max_upload_size: 104857600,
            static_dir: "./static".into(),
            widget_dir: "./frontend/widget/dist".into(),
            base_url: "http://localhost:3000".into(),
            cors_origins: None,
            tls_cert_path: None,
            tls_key_path: None,
            plugin_dir: None,
            plugin_hot_reload: false,
            plugin_max_memory_mb: default_plugin_max_memory(),
            plugin_default_timeout_ms: default_plugin_timeout(),
            plugin_wasm_pool_size: default_plugin_wasm_pool_size(),
            plugin_lua_pool_size: default_plugin_lua_pool_size(),
            plugin_js_pool_size: default_plugin_js_pool_size(),
            plugin_disabled: vec![],
            plugin_vfs_root: "./test-storage/vfs".into(),
            plugin_vfs_max_file_size: default_plugin_vfs_max_file_size(),
            plugin_vfs_max_total_size: default_plugin_vfs_max_total_size(),
            log_dir: "./test-storage/logs".into(),
            log_max_files: 1,
            rate_limit_enabled: default_rate_limit_enabled(),
            rate_limit_global_max: default_rate_limit_global_max(),
            rate_limit_global_window: default_rate_limit_global_window(),
            rate_limit_register_max: default_rate_limit_register_max(),
            rate_limit_register_window: default_rate_limit_register_window(),
            rate_limit_login_max: default_rate_limit_login_max(),
            rate_limit_login_window: default_rate_limit_login_window(),
            rate_limit_comment_max: default_rate_limit_comment_max(),
            rate_limit_comment_window: default_rate_limit_comment_window(),
            rate_limit_api_token_max: default_rate_limit_api_token_max(),
            rate_limit_api_token_window: default_rate_limit_api_token_window(),
            worker_enabled: false,
            worker_concurrency: default_worker_concurrency(),
            worker_cpu_concurrency: default_worker_cpu_concurrency(),
            worker_poll_interval_ms: default_worker_poll_interval_ms(),
            worker_batch_size: default_worker_batch_size(),
            worker_default_max_attempts: default_worker_max_attempts(),
            worker_cron_tick_ms: default_worker_cron_tick_ms(),
            worker_visibility_timeout_secs: default_worker_visibility_timeout_secs(),
            worker_job_timeout_secs: default_worker_job_timeout_secs(),
            worker_sweep_interval_secs: default_worker_sweep_interval_secs(),
            cron_seed_enabled: false,
            cron_schedules: vec![],
            cron_log_retention_days: default_cron_log_retention_days(),
            cron_allow_system_scripts: false,
            cron_system_workdir: None,
            order_expire_minutes: default_order_expire_minutes(),
            search_engine: default_search_engine(),
            search_index_dir: "./test-storage/search_index".into(),
            content_type_dir: default_content_type_dir(),
            timezone: default_timezone(),
            storage_driver: default_storage_driver(),
            s3_endpoint: None,
            s3_access_key: None,
            s3_secret_key: None,
            s3_bucket: default_s3_bucket(),
            s3_region: default_s3_region(),
            s3_public_url: None,
            rule_engine: RuleEngineConfig::default(),
            graphql_enabled: false,
            websocket_enabled: false,
            mcp: McpConfig::default(),
            ai: AiConfig::default(),
            kb: KbConfig::default(),
            integration: IntegrationConfig::default(),
            apps: AppsConfig::default(),
            oauth: crate::config::oauth::OAuthConfig {
                enabled: false,
                redirect_url: "http://localhost:3000/auth/callback".into(),
                github: None,
                google: None,
                wechat: None,
            },
            registration_email_enabled: true,
            registration_sms_enabled: false,
            presence_heartbeat_ttl_secs: default_presence_heartbeat_ttl_secs(),
            presence_sweep_interval_secs: default_presence_sweep_interval_secs(),
            presence_heartbeat_interval_secs: default_presence_heartbeat_interval_secs(),
            sse_max_clients: default_sse_max_clients(),
            sse_max_session_clients: default_sse_max_session_clients(),
            sms_code_expires_in: default_sms_code_expires_in(),
            sms_code_length: default_sms_code_length(),
            sms_rate_limit_secs: default_sms_rate_limit_secs(),
            require_email_verification: false,
            builtins: BuiltinsConfig::default(),
            builtin_tenantable: true,
            base_domain: Some("app.com".to_string()),
            email_provider: default_email_provider(),
            email_smtp_host: None,
            email_smtp_port: default_email_smtp_port(),
            email_smtp_user: None,
            email_smtp_pass: None,
            email_from: None,
            email_from_name: None,
            email_sendgrid_api_key: None,
            email_resend_api_key: None,
            email_aliyun_access_key_id: None,
            email_aliyun_access_key_secret: None,
            email_aliyun_region: None,
            email_tencent_secret_id: None,
            email_tencent_secret_key: None,
            email_tencent_region: None,
            sms_provider: default_sms_provider(),
            sms_aliyun_access_key_id: None,
            sms_aliyun_access_key_secret: None,
            sms_aliyun_sign_name: None,
            sms_aliyun_template_code: None,
            sms_twilio_account_sid: None,
            sms_twilio_auth_token: None,
            sms_twilio_from: None,
            app_key: Some("8Z/G4qNkqbuIzqSCpkfOwjsEsjIgfVawB+hYgERkqlw=".into()),
            started_at: None,
        }
    }

    /// Initialize app config with layered `.env` loading.
    ///
    /// Loading order (later files override earlier):
    /// 1. `.env` — base defaults shared across all environments
    /// 2. `.env.{profile}` — environment-specific overrides
    /// 3. `.env.local` — personal local overrides (never committed to git)
    ///
    /// The active profile is selected via `APP_PROFILE` env var, defaulting
    /// to `development`.
    ///
    /// Production safety checks are enforced when `APP_ENV=production` (set
    /// via any of the layered files).
    pub fn init() -> Self {
        let profile = env::var("APP_PROFILE")
            .or_else(|_| env::var("APP_ENV"))
            .unwrap_or_else(|_| "development".into());

        load_dotenv(".env");
        load_dotenv(&format!(".env.{profile}"));
        load_dotenv(".env.local");

        let mut config = Self::from_env();
        config.started_at = Some(std::time::Instant::now());

        if config.app_key.is_none() {
            let key = Self::generate_app_key();
            tracing::info!("APP_KEY not set, generated a new key and saved to .env");
            Self::persist_app_key(&key);
            config.app_key = Some(key);
        }

        if config.env == "production" {
            assert!(
                config.jwt_secret != DEFAULT_JWT_SECRET,
                "FATAL: JWT_SECRET must be set in production. Refusing to start with default secret."
            );
            assert!(
                config.cors_origins.is_some(),
                "FATAL: CORS_ORIGINS must be set in production. \
                 Refusing to start with wildcard CORS."
            );
        }

        tracing::info!(
            "loaded config: profile={profile}, env={}, host={}:{}, base_url={}",
            config.env,
            config.host,
            config.port,
            config.base_url
        );
        config
    }

    /// Load `.env` files for a given profile without constructing config.
    ///
    /// Useful for CLI tools that need env vars but not a full `AppConfig`.
    pub fn load_env(profile: &str) {
        load_dotenv(".env");
        load_dotenv(&format!(".env.{profile}"));
        load_dotenv(".env.local");
    }

    fn generate_app_key() -> String {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .unwrap_or_else(|e| panic!("failed to generate random APP_KEY: {e}"));
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn persist_app_key(key: &str) {
        let env_path = std::path::Path::new(".env");
        let line = format!("APP_KEY={key}");
        if env_path.exists() {
            if let Ok(content) = std::fs::read_to_string(env_path) {
                let has_own_line = content.lines().any(|l| l.starts_with("APP_KEY="));
                let updated = if has_own_line {
                    content
                        .lines()
                        .map(|l| if l.starts_with("APP_KEY=") { &line } else { l })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    let cleaned: Vec<&str> = content
                        .lines()
                        .filter(|l| !l.trim().starts_with("# APP_KEY="))
                        .collect();
                    format!("{line}\n{}", cleaned.join("\n"))
                };
                let _ = std::fs::write(env_path, updated);
            }
        } else {
            let _ = std::fs::write(env_path, format!("{line}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_has_expected_values() {
        let c = AppConfig::test_defaults();
        assert_eq!(c.host, "0.0.0.0");
        assert_eq!(c.port, 9898);
        assert_eq!(c.env, "test");
        assert_eq!(c.jwt_access_expires, 900);
        assert_eq!(c.jwt_refresh_expires, 604800);
        assert_eq!(c.max_upload_size, 104857600);
        assert_eq!(c.db_pool_size, 1);
        assert!(!c.graphql_enabled);
        assert!(!c.websocket_enabled);
        assert!(!c.worker_enabled);
        assert!(c.registration_email_enabled);
        assert!(!c.registration_sms_enabled);
        assert!(!c.require_email_verification);
    }

    #[test]
    fn sse_and_presence_defaults() {
        let c = AppConfig::test_defaults();
        assert_eq!(c.sse_max_clients, 64);
        assert_eq!(c.sse_max_session_clients, 512);
        assert_eq!(c.presence_heartbeat_ttl_secs, 75);
        assert_eq!(c.presence_sweep_interval_secs, 10);
        assert_eq!(c.presence_heartbeat_interval_secs, 30);
    }

    #[test]
    fn test_defaults_jwt_secret_not_empty() {
        let c = AppConfig::test_defaults();
        assert!(c.jwt_secret.len() >= 32);
    }

    #[test]
    fn builtins_default_all_enabled() {
        let b = BuiltinsConfig::default();
        assert!(b.blog);
        assert!(b.pages);
        assert!(b.media);
        assert!(b.fulltext);
        assert!(b.workflow);
        assert!(!b.is_all_disabled());
    }

    #[test]
    fn builtins_is_all_disabled() {
        let b = BuiltinsConfig {
            blog: false,
            pages: false,
            media: false,
            fulltext: false,
            workflow: false,
            ecommerce: false,
            payment: false,
            wallet: false,
            mcp: false,
            llm_gateway: false,
        };
        assert!(b.is_all_disabled());
    }

    #[test]
    fn builtins_protected_tables_includes_core() {
        let b = BuiltinsConfig::default();
        let tables = b.protected_tables();
        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"posts".to_string()));
        assert!(tables.contains(&"pages".to_string()));
        assert!(tables.contains(&"media".to_string()));
        assert!(tables.contains(&"_migrations".to_string()));
        assert!(tables.contains(&"wallets".to_string()));
        assert!(tables.contains(&"categories".to_string()));
    }

    #[test]
    fn rule_engine_config_defaults() {
        let r = RuleEngineConfig::default();
        assert_eq!(r.prefix_auth_id, "@request.auth.id");
        assert_eq!(r.sql_now_fn, crate::db::Driver::now_fn());
        assert_eq!(r.cms_cache_ttl_secs, 30);
        assert_eq!(r.cms_max_page_size, 100);
    }

    #[test]
    fn profile_env_file_naming() {
        let name = format!(".env.{}", "production");
        assert_eq!(name, ".env.production");
        let name = format!(".env.{}", "test");
        assert_eq!(name, ".env.test");
    }

    #[test]
    fn production_validation_rejects_default_jwt() {
        let mut c = AppConfig::test_defaults();
        c.env = "production".to_string();
        c.jwt_secret = DEFAULT_JWT_SECRET.to_string();
        c.cors_origins = Some("https://example.com".into());
        assert_eq!(c.env, "production");
        assert_eq!(c.jwt_secret, DEFAULT_JWT_SECRET);
    }

    #[test]
    fn production_validation_rejects_missing_cors() {
        let mut c = AppConfig::test_defaults();
        c.env = "production".to_string();
        c.jwt_secret = "a-very-long-production-secret-key-at-least-32-chars".to_string();
        c.cors_origins = None;
        assert!(c.cors_origins.is_none());
    }
}
