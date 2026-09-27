-- ============================================================
-- raisfast complete database schema — MySQL (with multi-tenant support)
-- Merged from all migration files for one-click initialization of new deployments
-- Generated date：2026-05-07
--
-- MySQL notes:
-- - All INDEX definitions are inline in CREATE TABLE for idempotent re-execution
-- - Partial indexes with WHERE clauses are not supported, removed
-- - BOOLEAN is actually TINYINT(1)
-- ============================================================

-- ── Platform foundation layer (always enabled) ──────────────────────────────────

-- Tenants
CREATE TABLE IF NOT EXISTS tenants (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    domain VARCHAR(255) UNIQUE,
    config JSON NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Users
CREATE TABLE IF NOT EXISTS users (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    username VARCHAR(255) UNIQUE NOT NULL,
    avatar VARCHAR(500),
    bio TEXT,
    website VARCHAR(500),
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    registered_via VARCHAR(100) NOT NULL,
    display_name VARCHAR(100),
    slug VARCHAR(100) UNIQUE,
    locale VARCHAR(10),
    social_links JSON,
    metadata JSON,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_users_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- User credentials
CREATE TABLE IF NOT EXISTS user_credentials (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    auth_type VARCHAR(100) NOT NULL,
    identifier VARCHAR(500) NOT NULL,
    credential_data JSON NOT NULL,
    verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_credential_type_id (auth_type, identifier),
    INDEX idx_user_credentials_user (user_id),
    INDEX idx_user_credentials_type (auth_type)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- OAuth account bindings
CREATE TABLE IF NOT EXISTS oauth_accounts (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    provider VARCHAR(50) NOT NULL,
    provider_user_id VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    display_name VARCHAR(255),
    avatar_url VARCHAR(500),
    access_token VARCHAR(1024),
    refresh_token VARCHAR(1024),
    token_expires_at DATETIME,
    profile TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_oauth_provider (provider, provider_user_id),
    INDEX idx_oauth_accounts_user (user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- OAuth short-lived state storage (PKCE)
CREATE TABLE IF NOT EXISTS oauth_states (
    id BIGINT PRIMARY KEY,
    provider VARCHAR(50) NOT NULL,
    code_verifier VARCHAR(255) NOT NULL,
    user_id BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL,
    INDEX idx_oauth_states_expires (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Currency configuration
CREATE TABLE IF NOT EXISTS currencies (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    code VARCHAR(10) NOT NULL,
    name VARCHAR(255) NOT NULL,
    decimals BIGINT NOT NULL DEFAULT 0,
    is_active TINYINT(1) NOT NULL DEFAULT 1,
    version BIGINT NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_currencies_tenant_code (tenant_id, code),
    CONSTRAINT chk_currencies_code CHECK (code = UPPER(code) AND CHAR_LENGTH(code) BETWEEN 1 AND 10),
    CONSTRAINT chk_currencies_decimals CHECK (decimals BETWEEN 0 AND 18)
);

CREATE TABLE IF NOT EXISTS wallets (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    currency VARCHAR(50) NOT NULL,
    balance BIGINT NOT NULL DEFAULT 0 CHECK(balance >= 0),
    llm_carry_quota BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 1,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_wallets_user_currency (user_id, currency),
    INDEX idx_wallets_currency (currency),
    INDEX idx_wallets_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS wallet_transactions (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    wallet_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    entry_type VARCHAR(10) NOT NULL,
    amount BIGINT NOT NULL CHECK(amount > 0),
    balance_after BIGINT NOT NULL CHECK(balance_after >= 0),
    tx_type VARCHAR(50) NOT NULL,
    currency VARCHAR(50) NOT NULL,
    transaction_no VARCHAR(255) NOT NULL UNIQUE,
    related_tx_id BIGINT,
    reference_type VARCHAR(100),
    reference_id VARCHAR(255),
    counterparty_wallet_id BIGINT,
    metadata TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_wallet_tx_wallet (wallet_id),
    INDEX idx_wallet_tx_user (user_id, created_at DESC),
    INDEX idx_wallet_tx_reference (reference_type, reference_id),
    INDEX idx_wallet_tx_tenant_user (tenant_id, user_id, created_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Refresh Tokens
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    token VARCHAR(500) UNIQUE NOT NULL,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_refresh_tokens_user (user_id),
    INDEX idx_refresh_tokens_expires_at (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Site options
CREATE TABLE IF NOT EXISTS options (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    `option_key` VARCHAR(255) NOT NULL,
    value JSON NOT NULL,
    `type` VARCHAR(50) NOT NULL DEFAULT 'text',
    group_name VARCHAR(100) NOT NULL DEFAULT 'general',
    label VARCHAR(255) NOT NULL DEFAULT '',
    description TEXT,
    validation JSON,
    is_public BOOLEAN NOT NULL DEFAULT FALSE,
    autoload BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order BIGINT NOT NULL DEFAULT 0,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_options_tenant_option_key (tenant_id, `option_key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- RBAC roles
CREATE TABLE IF NOT EXISTS roles (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    description TEXT,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_roles_tenant_name (tenant_id, name),
    INDEX idx_roles_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- RBAC permissions
CREATE TABLE IF NOT EXISTS permissions (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    role_id BIGINT NOT NULL,
    action VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    fields JSON,
    conditions JSON,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY idx_permissions_role_action_subject (role_id, action, subject),
    INDEX idx_permissions_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- User-role assignments (many-to-many)
CREATE TABLE IF NOT EXISTS user_roles (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY idx_user_roles_unique (tenant_id, user_id, role_id),
    INDEX idx_user_roles_user (user_id),
    INDEX idx_user_roles_role (role_id),
    INDEX idx_user_roles_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Audit log
CREATE TABLE IF NOT EXISTS audit_log (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    actor_id BIGINT,
    actor_role VARCHAR(50),
    action VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    subject_id VARCHAR(36),
    detail JSON,
    ip_address VARCHAR(45),
    user_agent TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_audit_log_action (action),
    INDEX idx_audit_log_actor (actor_id),
    INDEX idx_audit_log_tenant_created (tenant_id, created_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- API Token
CREATE TABLE IF NOT EXISTS api_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    name VARCHAR(255) NOT NULL,
    token_hash VARCHAR(255) UNIQUE NOT NULL,
    token_encrypted TEXT NOT NULL,
    description VARCHAR(1000) DEFAULT '',
    scopes JSON NOT NULL,
    last_used_at DATETIME,
    expires_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_api_tokens_user_id (user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Webhook subscriptions
CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL DEFAULT '',
    url VARCHAR(1024) NOT NULL,
    secret VARCHAR(255) NOT NULL,
    events JSON NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    description TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_webhook_subscriptions_enabled (enabled),
    INDEX idx_webhook_subscriptions_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Webhook delivery log
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id BIGINT PRIMARY KEY,
    webhook_id BIGINT NOT NULL,
    event VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,
    status_code INT,
    error TEXT,
    duration_ms BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_webhook_deliveries_webhook (webhook_id),
    INDEX idx_webhook_deliveries_created (created_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Plugin KV storage
CREATE TABLE IF NOT EXISTS plugin_storage (
    plugin_id VARCHAR(100) NOT NULL,
    `storage_key` VARCHAR(255) NOT NULL,
    value JSON NOT NULL,
    expires_at DATETIME,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plugin_id, `storage_key`),
    INDEX idx_plugin_storage_plugin (plugin_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Content revision history
CREATE TABLE IF NOT EXISTS content_revisions (
    id BIGINT PRIMARY KEY,
    content_type VARCHAR(100) NOT NULL,
    record_id BIGINT NOT NULL,
    revision_number BIGINT NOT NULL,
    snapshot JSON NOT NULL,
    created_by BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_revision (content_type, record_id, revision_number),
    INDEX idx_revisions_ct_record_rev (content_type, record_id, revision_number DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Password reset tokens
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    token VARCHAR(255) NOT NULL UNIQUE,
    expires_at DATETIME NOT NULL,
    used_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_password_reset_tokens_user_id (user_id),
    INDEX idx_password_reset_tokens_expires_at (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- SMS verification codes
CREATE TABLE IF NOT EXISTS sms_codes (
    id BIGINT PRIMARY KEY,
    phone VARCHAR(50) NOT NULL,
    code VARCHAR(20) NOT NULL,
    purpose VARCHAR(50) NOT NULL,
    expires_at DATETIME NOT NULL,
    verified_at DATETIME,
    attempts BIGINT NOT NULL DEFAULT 0,
    ip_address VARCHAR(45),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_sms_codes_phone (phone),
    INDEX idx_sms_codes_expires (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- User device codes (IDE authentication)
CREATE TABLE IF NOT EXISTS user_device_codes (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    code VARCHAR(255) NOT NULL UNIQUE,
    access_token TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    expires_at DATETIME NOT NULL,
    used_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_user_device_codes_code (code),
    INDEX idx_user_device_codes_user_id (user_id),
    INDEX idx_user_device_codes_expires_at (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Email verification tokens
CREATE TABLE IF NOT EXISTS email_verification_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    token VARCHAR(255) NOT NULL UNIQUE,
    email VARCHAR(255) NOT NULL,
    expires_at DATETIME NOT NULL,
    verified_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_email_verification_tokens_user_id (user_id),
    INDEX idx_email_verification_tokens_expires (expires_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Background job queue
CREATE TABLE IF NOT EXISTS jobs (
    id               BIGINT PRIMARY KEY,
    job_type         VARCHAR(100) NOT NULL,
    payload          JSON NOT NULL,
    status           VARCHAR(50) NOT NULL DEFAULT 'pending',
    attempts         INT NOT NULL DEFAULT 0,
    max_attempts     INT NOT NULL DEFAULT 3,
    run_after        DATETIME,
    error            TEXT,
    cron_schedule_id BIGINT,
    cron_log_id      BIGINT,
    priority         SMALLINT NOT NULL DEFAULT 0,
    timeout_secs     INT,
    dedup_key        VARCHAR(255),
    created_at       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_jobs_status_run_after (status, run_after),
    INDEX idx_jobs_type (job_type),
    INDEX idx_jobs_dedup_key (dedup_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Cron job schedules
CREATE TABLE IF NOT EXISTS cron_schedules (
    id           BIGINT PRIMARY KEY,
    label        VARCHAR(255) NOT NULL,
    job_type     VARCHAR(100) NOT NULL,
    payload      JSON,
    cron_expr    VARCHAR(100) NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at  DATETIME,
    next_run_at  DATETIME NOT NULL,
    plugin_id    VARCHAR(100),
    exec_kind    VARCHAR(20) NOT NULL DEFAULT 'builtin',
    handler_id   VARCHAR(100),
    params       JSON,
    script_lang    VARCHAR(20),
    script_source  TEXT,
    script_entry   VARCHAR(100) NOT NULL DEFAULT 'on_cron_tick',
    use_shell      BOOLEAN NOT NULL DEFAULT TRUE,
    timeout_secs   INTEGER,
    created_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_cron_enabled (enabled),
    INDEX idx_cron_next_run (next_run_at),
    INDEX idx_cron_plugin (plugin_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Cron execution log
CREATE TABLE IF NOT EXISTS cron_execution_log (
    id           BIGINT PRIMARY KEY,
    schedule_id  BIGINT NOT NULL,
    job_type     VARCHAR(100) NOT NULL,
    label        VARCHAR(255) NOT NULL,
    status       VARCHAR(50) NOT NULL DEFAULT 'running',
    duration_ms  BIGINT,
    error        TEXT,
    started_at   DATETIME NOT NULL,
    finished_at  DATETIME,
    INDEX idx_cron_log_schedule (schedule_id),
    INDEX idx_cron_log_status (status),
    INDEX idx_cron_log_started (started_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ── Built-in module: Blog (BUILTIN_BLOG=true) ──────────────────

-- Categories
CREATE TABLE IF NOT EXISTS categories (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    description TEXT,
    slug VARCHAR(255) NOT NULL,
    parent_id BIGINT,
    sort_order BIGINT NOT NULL DEFAULT 0,
    created_by BIGINT,
    updated_by BIGINT,
    cover_image VARCHAR(500),
    meta_title VARCHAR(255),
    meta_description VARCHAR(500),
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    og_image VARCHAR(500),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_categories_tenant_name (tenant_id, name),
    UNIQUE KEY uq_categories_tenant_slug (tenant_id, slug),
    INDEX idx_categories_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Product categories
CREATE TABLE IF NOT EXISTS product_categories (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL,
    description TEXT,
    cover_image VARCHAR(500),
    parent_id BIGINT,
    sort_order BIGINT NOT NULL DEFAULT 0,
    meta_title VARCHAR(255),
    meta_description VARCHAR(500),
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    og_image VARCHAR(500),
    created_by BIGINT,
    updated_by BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_product_categories_tenant_name (tenant_id, name),
    UNIQUE KEY uq_product_categories_tenant_slug (tenant_id, slug),
    INDEX idx_product_categories_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Tags
CREATE TABLE IF NOT EXISTS tags (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL,
    created_by BIGINT,
    updated_by BIGINT,
    description TEXT,
    cover_image VARCHAR(500),
    meta_title VARCHAR(255),
    meta_description VARCHAR(500),
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    og_image VARCHAR(500),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_tags_tenant_name (tenant_id, name),
    UNIQUE KEY uq_tags_tenant_slug (tenant_id, slug),
    INDEX idx_tags_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Posts
CREATE TABLE IF NOT EXISTS posts (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    title VARCHAR(500) NOT NULL,
    slug VARCHAR(255) NOT NULL,
    content LONGTEXT NOT NULL,
    excerpt TEXT,
    cover_image VARCHAR(500),
    image_ids TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    created_by BIGINT NOT NULL,
    updated_by BIGINT,
    category_id BIGINT,
    view_count INT NOT NULL DEFAULT 0,
    is_pinned BOOLEAN NOT NULL DEFAULT FALSE,
    password VARCHAR(255),
    comment_status VARCHAR(20) NOT NULL DEFAULT 'open',
    format VARCHAR(20) NOT NULL DEFAULT 'standard',
    template VARCHAR(100) NOT NULL DEFAULT 'default',
    meta_title VARCHAR(255),
    meta_description VARCHAR(500),
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    og_image VARCHAR(500),
    canonical_url VARCHAR(1024),
    reading_time INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    published_at DATETIME,
    INDEX idx_posts_status (status),
    INDEX idx_posts_author (created_by),
    INDEX idx_posts_category (category_id),
    INDEX idx_posts_status_created (status, is_pinned DESC, created_at DESC),
    INDEX idx_posts_status_category (status, category_id),
    INDEX idx_posts_status_author (status, created_by),
    INDEX idx_posts_tenant (tenant_id),
    UNIQUE KEY uq_posts_tenant_slug (tenant_id, slug)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Posts-Tags (many-to-many)
CREATE TABLE IF NOT EXISTS posts_tags (
    post_id BIGINT NOT NULL,
    tag_id BIGINT NOT NULL,
    PRIMARY KEY (post_id, tag_id),
    INDEX idx_posts_tags_tag_id (tag_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS taggings (
    id BIGINT PRIMARY KEY,
    tag_id BIGINT NOT NULL,
    taggable_type VARCHAR(50) NOT NULL,
    taggable_id BIGINT NOT NULL,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    UNIQUE KEY uq_taggings_tenant (tenant_id, tag_id, taggable_type, taggable_id),
    INDEX idx_taggings_tag (tag_id),
    INDEX idx_taggings_taggable (taggable_type, taggable_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Comments
CREATE TABLE IF NOT EXISTS comments (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    post_id BIGINT NOT NULL,
    created_by BIGINT,
    updated_by BIGINT,
    nickname VARCHAR(100),
    email VARCHAR(255),
    content TEXT NOT NULL,
    parent_id BIGINT,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    author_ip VARCHAR(45),
    author_url VARCHAR(500),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_comments_post (post_id),
    INDEX idx_comments_status (status),
    INDEX idx_comments_post_status (post_id, status),
    INDEX idx_comments_parent_id (parent_id),
    INDEX idx_comments_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ── Built-in module: Pages (BUILTIN_PAGES=true) ────────────────

CREATE TABLE IF NOT EXISTS pages (
    id               BIGINT PRIMARY KEY,
    tenant_id        VARCHAR(36) NOT NULL DEFAULT 'default',
    title            VARCHAR(500) NOT NULL,
    slug             VARCHAR(255) NOT NULL UNIQUE,
    content          LONGTEXT,
    blocks           JSON,
    meta_title       VARCHAR(255),
    meta_description VARCHAR(500),
    og_image         VARCHAR(500),
    template         VARCHAR(100) NOT NULL DEFAULT 'default',
    parent_id        BIGINT,
    sort_order       BIGINT NOT NULL DEFAULT 0,
    status           VARCHAR(50) NOT NULL DEFAULT 'draft',
    created_by       BIGINT NOT NULL,
    updated_by       BIGINT,
    cover_image      VARCHAR(500),
    published_at     DATETIME,
    password         VARCHAR(255),
    comment_status   VARCHAR(20) NOT NULL DEFAULT 'closed',
    og_title         VARCHAR(255),
    og_description   VARCHAR(500),
    canonical_url    VARCHAR(1024),
    created_at       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_pages_status (status),
    INDEX idx_pages_parent (parent_id),
    INDEX idx_pages_author (created_by),
    INDEX idx_pages_tenant_slug (tenant_id, slug),
    INDEX idx_pages_tenant_status (tenant_id, status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS reusable_blocks (
    id          BIGINT PRIMARY KEY,
    tenant_id   VARCHAR(36) NOT NULL DEFAULT 'default',
    name        VARCHAR(255) NOT NULL,
    block_type  VARCHAR(100) NOT NULL,
    content     LONGTEXT NOT NULL,
    description TEXT,
    created_by  BIGINT,
    updated_by  BIGINT,
    created_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_reusable_blocks_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ── Built-in module: Media (BUILTIN_MEDIA=true) ────────────────

CREATE TABLE IF NOT EXISTS media (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    filename VARCHAR(255) NOT NULL,
    filepath VARCHAR(500) NOT NULL,
    mimetype VARCHAR(100) NOT NULL,
    size BIGINT NOT NULL,
    width INT,
    height INT,
    title VARCHAR(255),
    alt_text VARCHAR(255),
    caption TEXT,
    description TEXT,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_media_user_created (user_id, created_at DESC),
    INDEX idx_media_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ── Built-in module: Workflow (BUILTIN_WORKFLOW=true) ──────────

CREATE TABLE IF NOT EXISTS workflow_definitions (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    steps JSON NOT NULL,
    initial_step VARCHAR(100) NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS workflow_instances (
    id BIGINT PRIMARY KEY,
    definition_id BIGINT NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'running',
    current_step VARCHAR(100),
    context JSON NOT NULL,
    triggered_by BIGINT,
    started_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_wf_instances_definition (definition_id),
    INDEX idx_wf_instances_status (status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS workflow_step_logs (
    id BIGINT PRIMARY KEY,
    instance_id BIGINT NOT NULL,
    step_id VARCHAR(100) NOT NULL,
    step_name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'running',
    input JSON,
    output JSON,
    error TEXT,
    started_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    INDEX idx_wf_step_logs_instance (instance_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Products
CREATE TABLE IF NOT EXISTS products (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    category_id BIGINT,
    title VARCHAR(500) NOT NULL,
    description TEXT,
    cover_url VARCHAR(500),
    product_type VARCHAR(50) NOT NULL DEFAULT 'custom',
    fulfillment_type VARCHAR(50) NOT NULL DEFAULT 'digital',
    delivery_hook VARCHAR(255),
    weight BIGINT,
    shipping_template_id BIGINT,
    price BIGINT NOT NULL CHECK(price >= 0),
    currency VARCHAR(50) NOT NULL DEFAULT 'USD',
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    attributes JSON,
    sort_order BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    slug VARCHAR(255),
    content LONGTEXT,
    image_ids TEXT,
    original_price BIGINT,
    specs JSON,
    unit VARCHAR(50) NOT NULL DEFAULT 'piece',
    min_purchase BIGINT NOT NULL DEFAULT 1,
    max_purchase BIGINT,
    total_sales BIGINT NOT NULL DEFAULT 0,
    virtual_sales BIGINT NOT NULL DEFAULT 0,
    meta_title VARCHAR(255),
    meta_description VARCHAR(500),
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    og_image VARCHAR(500),
    published_at DATETIME,
    stock BIGINT NOT NULL DEFAULT 0,
    cost_price BIGINT,
    sale_price BIGINT,
    has_variants BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_products_status (status),
    INDEX idx_products_type (product_type),
    INDEX idx_products_tenant (tenant_id),
    INDEX idx_products_tenant_status (tenant_id, status),
    UNIQUE KEY uq_products_tenant_slug (tenant_id, slug)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Product Variants
CREATE TABLE IF NOT EXISTS product_variants (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    product_id BIGINT NOT NULL,
    sku VARCHAR(100) UNIQUE,
    title VARCHAR(500) NOT NULL,
    price BIGINT NOT NULL CHECK(price >= 0),
    original_price BIGINT,
    stock BIGINT NOT NULL DEFAULT 0,
    attributes JSON,
    image_url VARCHAR(500),
    weight BIGINT,
    sort_order BIGINT NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_product_variants_product (product_id),
    INDEX idx_product_variants_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- User Addresses
CREATE TABLE IF NOT EXISTS user_addresses (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    label VARCHAR(100) NOT NULL DEFAULT '',
    recipient_name VARCHAR(200) NOT NULL,
    phone VARCHAR(50) NOT NULL,
    country VARCHAR(10) NOT NULL DEFAULT 'CN',
    province VARCHAR(100) NOT NULL DEFAULT '',
    city VARCHAR(100) NOT NULL DEFAULT '',
    district VARCHAR(100) NOT NULL DEFAULT '',
    address_line1 TEXT NOT NULL,
    address_line2 TEXT,
    postal_code VARCHAR(20),
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    address_type VARCHAR(20) NOT NULL DEFAULT 'shipping',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_user_addresses_user (user_id),
    INDEX idx_user_addresses_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Orders
CREATE TABLE IF NOT EXISTS orders (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    order_no VARCHAR(255) NOT NULL UNIQUE,
    subtotal BIGINT NOT NULL DEFAULT 0,
    discount_amount BIGINT NOT NULL DEFAULT 0,
    shipping_amount BIGINT NOT NULL DEFAULT 0,
    total_amount BIGINT NOT NULL CHECK(total_amount >= 0),
    currency VARCHAR(50) NOT NULL DEFAULT 'USD',
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    buyer_name VARCHAR(255),
    buyer_phone VARCHAR(50),
    buyer_email VARCHAR(255),
    shipping_address TEXT,
    tracking_no VARCHAR(255),
    carrier VARCHAR(100),
    remark TEXT,
    admin_remark TEXT,
    delivery_data TEXT,
    tax_amount BIGINT NOT NULL DEFAULT 0,
    coupon_id BIGINT,
    shipping_address_id BIGINT,
    billing_address_id BIGINT,
    paid_at DATETIME,
    completed_at DATETIME,
    cancelled_at DATETIME,
    refunding_at DATETIME,
    refunded_at DATETIME,
    expired_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_orders_user (user_id),
    INDEX idx_orders_status (status),
    INDEX idx_orders_tenant (tenant_id),
    INDEX idx_orders_tenant_user_status (tenant_id, user_id, status),
    INDEX idx_orders_tenant_status_created (tenant_id, status, created_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Order Items
CREATE TABLE IF NOT EXISTS order_items (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    order_id BIGINT NOT NULL,
    product_id BIGINT,
    variant_id BIGINT,
    title VARCHAR(500) NOT NULL,
    description TEXT,
    sku VARCHAR(100),
    unit_price BIGINT NOT NULL CHECK(unit_price >= 0),
    quantity BIGINT NOT NULL CHECK(quantity > 0),
    subtotal BIGINT NOT NULL,
    tax_amount BIGINT NOT NULL DEFAULT 0,
    cover_url VARCHAR(500),
    attributes JSON,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_order_items_order (order_id),
    INDEX idx_order_items_product (product_id),
    INDEX idx_order_items_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS cart_items (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    variant_id BIGINT,
    quantity BIGINT NOT NULL DEFAULT 1,
    attributes TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_cart_user_product_variant (user_id, product_id, variant_id),
    INDEX idx_cart_items_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS payment_channels (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    provider VARCHAR(50) NOT NULL,
    name VARCHAR(200) NOT NULL,
    is_live BOOLEAN NOT NULL DEFAULT FALSE,
    credentials JSON NOT NULL,
    webhook_secret TEXT,
    settings JSON,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_channel_provider_name (provider, name),
    INDEX idx_payment_channels_provider (provider),
    INDEX idx_payment_channels_active (is_active),
    INDEX idx_payment_channels_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Payment Orders
CREATE TABLE IF NOT EXISTS payment_orders (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    order_id VARCHAR(36),
    title VARCHAR(500) NOT NULL,
    amount BIGINT NOT NULL,
    currency VARCHAR(10) NOT NULL DEFAULT 'USD',
    channel_id BIGINT NOT NULL,
    provider VARCHAR(50) NOT NULL,
    provider_order_id VARCHAR(200),
    provider_method VARCHAR(50),
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    reference_type VARCHAR(50),
    reference_id VARCHAR(200),
    return_url VARCHAR(500),
    idempotency_key VARCHAR(200) NOT NULL UNIQUE,
    version BIGINT NOT NULL DEFAULT 1,
    provider_data TEXT,
    client_ip VARCHAR(45),
    client_language VARCHAR(50),
    client_country VARCHAR(2),
    client_user_agent VARCHAR(512),
    channel_selected_by VARCHAR(20),
    metadata JSON,
    paid_at DATETIME,
    cancelled_at DATETIME,
    expired_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_payment_orders_user (user_id),
    INDEX idx_payment_orders_status (status),
    INDEX idx_payment_orders_provider (provider_order_id),
    INDEX idx_payment_orders_order_id (order_id),
    INDEX idx_payment_orders_tenant (tenant_id),
    INDEX idx_payment_orders_tenant_status_created (tenant_id, status, created_at DESC)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Payment Transactions
CREATE TABLE IF NOT EXISTS payment_transactions (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    payment_order_id BIGINT NOT NULL,
    order_id VARCHAR(36),
    user_id BIGINT NOT NULL,
    tx_type VARCHAR(50) NOT NULL,
    amount BIGINT NOT NULL,
    currency VARCHAR(10) NOT NULL,
    provider_tx_id VARCHAR(200) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    raw_payload TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_payment_tx_order (payment_order_id),
    INDEX idx_payment_tx_order_id (order_id),
    INDEX idx_payment_transactions_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Payment Refunds
CREATE TABLE IF NOT EXISTS payment_refunds (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    payment_order_id BIGINT NOT NULL,
    order_id VARCHAR(36),
    user_id BIGINT NOT NULL,
    amount BIGINT NOT NULL,
    currency VARCHAR(10) NOT NULL,
    reason VARCHAR(200),
    provider_refund_id VARCHAR(200),
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    payment_tx_id BIGINT,
    metadata TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_payment_refunds_order (payment_order_id),
    INDEX idx_payment_refunds_order_id (order_id),
    INDEX idx_payment_refunds_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Wallet Outbox (ensures wallet operations are never lost)
CREATE TABLE IF NOT EXISTS wallet_outbox (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    currency VARCHAR(10) NOT NULL,
    amount BIGINT NOT NULL,
    entry_type VARCHAR(20) NOT NULL,
    tx_type VARCHAR(20) NOT NULL,
    transaction_no VARCHAR(100) NOT NULL,
    reference_type VARCHAR(30),
    reference_id VARCHAR(100),
    metadata TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    attempts BIGINT NOT NULL DEFAULT 0,
    max_attempts BIGINT NOT NULL DEFAULT 5,
    last_error TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_wallet_outbox_status (status),
    INDEX idx_wallet_outbox_transaction_no (transaction_no),
    INDEX idx_wallet_outbox_tenant (tenant_id)
);

-- Product Comments (reviews/ratings)
CREATE TABLE IF NOT EXISTS product_comments (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    product_id BIGINT NOT NULL,
    order_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    rating BIGINT NOT NULL DEFAULT 5,
    title VARCHAR(255),
    content TEXT NOT NULL,
    images TEXT,
    status VARCHAR(32) NOT NULL DEFAULT 'approved',
    admin_reply TEXT,
    admin_replied_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE INDEX idx_product_comments_unique (product_id, order_id, user_id),
    INDEX idx_product_comments_user (user_id),
    INDEX idx_product_comments_status (status),
    INDEX idx_product_comments_tenant (tenant_id)
);
-- Product Favorites (wishlist)
CREATE TABLE IF NOT EXISTS product_favorites (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    created_at TIMESTAMP(0) NOT NULL DEFAULT NOW(),
    UNIQUE INDEX idx_product_favorites_unique (user_id, product_id),
    INDEX idx_product_favorites_user (user_id),
    INDEX idx_product_favorites_product (product_id),
    INDEX idx_product_favorites_tenant (tenant_id)
);


-- Coupons
CREATE TABLE IF NOT EXISTS coupons (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    code VARCHAR(64) NOT NULL UNIQUE,
    title VARCHAR(255) NOT NULL,
    coupon_type VARCHAR(32) NOT NULL DEFAULT 'percent',
    value BIGINT NOT NULL,
    min_order BIGINT NOT NULL DEFAULT 0,
    max_uses BIGINT NOT NULL DEFAULT 0,
    used_count BIGINT NOT NULL DEFAULT 0,
    max_uses_per_user BIGINT NOT NULL DEFAULT 1,
    starts_at DATETIME,
    expires_at DATETIME,
    status VARCHAR(32) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_coupons_status (status),
    INDEX idx_coupons_tenant (tenant_id)
);

-- Shipping Templates
CREATE TABLE IF NOT EXISTS shipping_templates (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    type VARCHAR(50) NOT NULL DEFAULT 'weight',
    first_unit BIGINT NOT NULL DEFAULT 1,
    first_price BIGINT NOT NULL DEFAULT 0,
    additional_unit BIGINT NOT NULL DEFAULT 1,
    additional_price BIGINT NOT NULL DEFAULT 0,
    free_shipping_amount BIGINT NOT NULL DEFAULT 0,
    regions JSON NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_shipping_templates_tenant (tenant_id),
    INDEX idx_shipping_templates_status (status)
);

-- ============================================================
-- Integration Plane (P1): itg_channels / itg_channel_cursors / itg_receipts
-- ============================================================

CREATE TABLE IF NOT EXISTS itg_channels (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    app_id VARCHAR(100),
    channel_key VARCHAR(255) NOT NULL,
    provider VARCHAR(100) NOT NULL,
    display_name VARCHAR(255) NOT NULL,
    mode VARCHAR(20) NOT NULL,
    transport VARCHAR(20) NOT NULL,
    framing VARCHAR(20) NOT NULL,
    codec VARCHAR(20) NOT NULL,
    endpoint VARCHAR(500),
    verify_kind VARCHAR(100) NOT NULL,
    verify_config JSON,
    credentials TEXT,
    mapping JSON,
    normalizer_plugin VARCHAR(255),
    pull_semantics VARCHAR(20),
    pull_config JSON,
    stream_config JSON,
    ack_kind VARCHAR(20) NOT NULL DEFAULT 'http-200',
    redelivery_max INT NOT NULL DEFAULT 5,
    backpressure JSON,
    target_type VARCHAR(255) NOT NULL,
    route_extra JSON,
    status VARCHAR(20) NOT NULL DEFAULT 'idle',
    last_error TEXT,
    lease_owner VARCHAR(100),
    enabled BOOLEAN NOT NULL DEFAULT 1,
    version BIGINT NOT NULL DEFAULT 1,
    shadow BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_itg_channels_tenant_key_version (tenant_id, channel_key, version),
    INDEX idx_itg_channels_tenant (tenant_id),
    INDEX idx_itg_channels_app (app_id),
    INDEX idx_itg_channels_status (status)
);

-- Cursor store: one row per pull channel; advanced by conditional update
CREATE TABLE IF NOT EXISTS itg_channel_cursors (
    channel_id BIGINT PRIMARY KEY,
    cursor_value JSON NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS itg_receipts (
    id BIGINT PRIMARY KEY,
    channel_id BIGINT NOT NULL,
    external_id VARCHAR(255) NOT NULL,
    kind VARCHAR(30) NOT NULL,
    payload_hash VARCHAR(128) NOT NULL,
    raw_ref VARCHAR(500),
    status VARCHAR(20) NOT NULL DEFAULT 'received',
    attempts INT NOT NULL DEFAULT 0,
    next_retry_at DATETIME NULL,
    envelope JSON,
    steps JSON,
    target_id BIGINT,
    received_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    delivered_at DATETIME NULL,
    UNIQUE KEY uq_itg_receipts_channel_external (channel_id, external_id),
    INDEX idx_itg_receipts_channel_status (channel_id, status),
    INDEX idx_itg_receipts_retry (status, next_retry_at)
);

-- ============================================================
-- Integration Plane (M0): itg_api_clients / itg_egress_log
-- ============================================================

-- Declarative outbound API clients (integration.md §9.2)
CREATE TABLE IF NOT EXISTS itg_api_clients (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    client_key VARCHAR(255) NOT NULL,
    display_name VARCHAR(255) NOT NULL,
    base_url VARCHAR(500) NOT NULL,
    auth JSON,
    credentials TEXT,
    rate_limit JSON,
    ops JSON,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_itg_api_clients_tenant_key (tenant_id, client_key),
    INDEX idx_itg_api_clients_tenant (tenant_id)
);

-- OAuth2 authorization-code tokens (oauth2-egress.md §2): one row per
-- (api-client, tenant); access/refresh tokens are vault-sealed.
CREATE TABLE IF NOT EXISTS itg_oauth_tokens (
    id BIGINT PRIMARY KEY,
    client_key VARCHAR(255) NOT NULL,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    access_token TEXT,
    refresh_token TEXT,
    expires_at DATETIME,
    scope VARCHAR(1000),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_itg_oauth_tokens (client_key, tenant_id),
    INDEX idx_itg_oauth_tokens_tenant (tenant_id)
);

-- Full outbound call log (integration.md §10.7: trace_id = itg_receipts.id)
CREATE TABLE IF NOT EXISTS itg_egress_log (
    id BIGINT PRIMARY KEY,
    trace_id BIGINT NULL,
    client_key VARCHAR(255) NOT NULL,
    op VARCHAR(255) NOT NULL,
    status VARCHAR(20) NOT NULL,
    http_status BIGINT NULL,
    latency_ms BIGINT NOT NULL DEFAULT 0,
    tokens_in BIGINT NULL,
    tokens_out BIGINT NULL,
    model VARCHAR(255) NULL,
    error TEXT,
    response_summary TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_itg_egress_log_trace (trace_id),
    INDEX idx_itg_egress_log_client_time (client_key, created_at)
);

-- ============================================================
-- App Bundle (M2): apps / app_ct_refs / app_licenses
-- ============================================================

-- Installed app registry + lifecycle state machine (app-bundle.md §3.2)
CREATE TABLE IF NOT EXISTS apps (
    id BIGINT PRIMARY KEY,
    app_id VARCHAR(255) NOT NULL,
    version VARCHAR(64) NOT NULL,
    status VARCHAR(32) NOT NULL,
    source VARCHAR(32) NOT NULL DEFAULT 'upload',
    source_ref VARCHAR(500) NULL,
    signature_ok BOOLEAN NOT NULL DEFAULT 1,
    install_log JSON,
    last_error TEXT,
    tenant_scope VARCHAR(32) NOT NULL DEFAULT 'global',
    options JSON,
    installed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_apps_app_id (app_id),
    INDEX idx_apps_status (status)
);

-- Materialized CT definitions owned by apps (app-bundle.md §6.3)
CREATE TABLE IF NOT EXISTS app_ct_refs (
    id BIGINT PRIMARY KEY,
    app_id VARCHAR(255) NOT NULL,
    ct_table VARCHAR(255) NOT NULL,
    schema_toml MEDIUMTEXT NOT NULL,
    version VARCHAR(64) NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_app_ct_refs_app_table (app_id, ct_table)
);

-- Per-tenant licenses for global apps (app-bundle.md §9)
CREATE TABLE IF NOT EXISTS app_licenses (
    id BIGINT PRIMARY KEY,
    app_id VARCHAR(255) NOT NULL,
    tenant_id VARCHAR(36) NOT NULL,
    granted_by BIGINT NULL,
    granted_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_app_licenses_app_tenant (app_id, tenant_id)
);

-- ============================================================
-- Seed data
-- ============================================================

-- Default tenant
INSERT IGNORE INTO tenants (id, name, domain, config, status, created_at, updated_at) VALUES
    (10001, 'Default', NULL, '{}', 'active', NOW(), NOW());

-- Default currencies
INSERT IGNORE INTO currencies (id, tenant_id, code, name, decimals) VALUES
    (10001, 'default', 'CNY', 'Chinese Yuan', 2),
    (10002, 'default', 'USD', 'US Dollar', 2),
    (10003, 'default', 'EUR', 'Euro', 2),
    (10004, 'default', 'GBP', 'British Pound', 2),
    (10005, 'default', 'JPY', 'Japanese Yen', 0);

-- System roles
INSERT IGNORE INTO roles (id, tenant_id, name, description, is_system, created_at, updated_at) VALUES
    (10001, 'default', 'admin', 'Super administrator', TRUE, NOW(), NOW()),
    (10002, 'default', 'editor', 'Editor', FALSE, NOW(), NOW()),
    (10003, 'default', 'author', 'Author', FALSE, NOW(), NOW()),
    (10004, 'default', 'reader', 'Reader', TRUE, NOW(), NOW());

-- Admin global permissions
INSERT IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10001, 'default', (SELECT id FROM roles WHERE name = 'admin'), '*', '*', NULL, NULL, NOW());

-- Editor permissions
INSERT IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10002, 'default', (SELECT id FROM roles WHERE name = 'editor'), '*.*', '*', NULL, NULL, NOW());

-- Author permissions
INSERT IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10003, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'posts', NULL, NULL, NOW()),
    (10004, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'posts', NULL, NULL, NOW()),
    (10005, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'posts', NULL, NULL, NOW()),
    (10006, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'posts', NULL, NULL, NOW()),
    (10007, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'pages', NULL, NULL, NOW()),
    (10008, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'pages', NULL, NULL, NOW()),
    (10009, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'pages', NULL, NULL, NOW()),
    (10010, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'pages', NULL, NULL, NOW()),
    (10036, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'media', NULL, NULL, NOW()),
    (10011, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'media', NULL, NULL, NOW()),
    (10012, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'media', NULL, NULL, NOW()),
    (10013, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'tags', NULL, NULL, NOW()),
    (10014, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'tags', NULL, NULL, NOW()),
    (10015, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'tags', NULL, NULL, NOW()),
    (10016, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'categories', NULL, NULL, NOW()),
    (10017, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'categories', NULL, NULL, NOW()),
    (10018, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'categories', NULL, NULL, NOW()),
    (10019, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'reusable_blocks', NULL, NULL, NOW()),
    (10020, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'reusable_blocks', NULL, NULL, NOW()),
    (10021, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'reusable_blocks', NULL, NULL, NOW()),
    (10022, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'reusable_blocks', NULL, NULL, NOW()),
    (10023, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'comments', NULL, NULL, NOW()),
    (10024, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'comments', NULL, NULL, NOW()),
    (10025, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'product_categories', NULL, NULL, NOW()),
    (10026, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'product_categories', NULL, NULL, NOW()),
    (10027, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'product_categories', NULL, NULL, NOW());

-- Reader permissions
INSERT IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10028, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'posts', NULL, NULL, NOW()),
    (10029, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'pages', NULL, NULL, NOW()),
    (10030, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'comments', NULL, NULL, NOW()),
    (10031, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'comments', NULL, NULL, NOW()),
    (10032, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'user_addresses', NULL, NULL, NOW()),
    (10033, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'user_addresses', NULL, NULL, NOW()),
    (10034, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'update', 'user_addresses', NULL, NULL, NOW()),
    (10035, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'user_addresses', NULL, NULL, NOW()),
    (10050, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'cart_items', NULL, NULL, NOW()),
    (10051, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'cart_items', NULL, NULL, NOW()),
    (10052, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'update', 'cart_items', NULL, NULL, NOW()),
    (10053, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'cart_items', NULL, NULL, NOW()),
    (10054, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'orders', NULL, NULL, NOW()),
    (10055, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'orders', NULL, NULL, NOW()),
    (10056, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'product_comments', NULL, NULL, NOW()),
    (10057, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'product_comments', NULL, NULL, NOW()),
    (10058, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'product_favorites', NULL, NULL, NOW()),
    (10059, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'product_favorites', NULL, NULL, NOW()),
    (10060, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'product_favorites', NULL, NULL, NOW());

-- Site options
INSERT IGNORE INTO options (id, tenant_id, `option_key`, value, `type`, group_name, label, description, validation, is_public, autoload, sort_order, updated_at) VALUES
    (10001, 'default', 'site_title', '"My Blog"', 'text', 'general', 'Site title', 'Displayed in browser title bar and page header', '{"max_length":100}', TRUE, TRUE, 1, NOW()),
    (10002, 'default', 'site_description', '""', 'text', 'general', 'Site description', 'Brief description of the site purpose', '{"max_length":500}', TRUE, TRUE, 2, NOW()),
    (10003, 'default', 'site_url', '""', 'url', 'general', 'Site URL', 'e.g. https://example.com', NULL, TRUE, TRUE, 3, NOW()),
    (10004, 'default', 'admin_email', '""', 'email', 'general', 'Admin email', NULL, NULL, FALSE, TRUE, 4, NOW()),
    (10005, 'default', 'timezone', '"UTC"', 'select', 'general', 'Timezone', NULL, '{"values":["UTC","Asia/Shanghai","Asia/Tokyo","US/Eastern","US/Pacific","Europe/London","Europe/Berlin"]}', TRUE, TRUE, 5, NOW()),
    (10006, 'default', 'date_format', '"%Y-%m-%d"', 'select', 'general', 'Date format', NULL, '{"values":["%Y-%m-%d","%d/%m/%Y","%m/%d/%Y","%Y年%m月%d日"]}', TRUE, TRUE, 6, NOW()),
    (10007, 'default', 'posts_per_page', '10', 'integer', 'reading', 'Posts per page', NULL, '{"min":1,"max":100}', TRUE, TRUE, 10, NOW()),
    (10008, 'default', 'rss_items', '20', 'integer', 'reading', 'RSS item count', NULL, '{"min":1,"max":100}', TRUE, TRUE, 11, NOW()),
    (10009, 'default', 'permalink_structure', '"/:year/:month/:slug"', 'select', 'reading', 'URL structure', NULL, '{"values":["/:year/:month/:slug","/:slug","/posts/:slug"]}', TRUE, TRUE, 12, NOW()),
    (10010, 'default', 'comment_moderation', 'true', 'boolean', 'discussion', 'Comments require moderation', 'When enabled, new comments require admin approval', NULL, FALSE, TRUE, 20, NOW()),
    (10011, 'default', 'comment_order', '"asc"', 'select', 'discussion', 'Comment order', NULL, '{"values":["asc","desc"]}', TRUE, TRUE, 21, NOW()),
    (10012, 'default', 'default_role', '"reader"', 'select', 'discussion', 'Default role for new users', NULL, '{"values":["reader","author"]}', FALSE, TRUE, 22, NOW()),
    (10013, 'default', 'theme', '"default"', 'select', 'appearance', 'Current theme', NULL, '{"values":["default","corporate","minimal","warm"]}', TRUE, TRUE, 30, NOW()),
    (10014, 'default', 'maintenance_mode', 'false', 'boolean', 'appearance', 'Maintenance mode', 'When enabled, a maintenance page is shown to visitors', NULL, TRUE, TRUE, 31, NOW()),
    (10015, 'default', 'default_currency', '"USD"', 'select', 'ecommerce', 'Default currency', 'Currency code for products and orders', '{"values":["USD","CNY","EUR","GBP","JPY","KRW","HKD","TWD","SGD","AUD","CAD"]}', TRUE, TRUE, 40, NOW()),
    (10017, 'default', 'reserved_usernames', '"admin,administrator,root,system,official,support,staff,moderator,mod,help,info,mail,webmaster,security,billing,sales,owner,superuser,operator"', 'text', 'general', 'Reserved usernames', 'Comma-separated usernames that cannot be registered', '{"max_length":10000}', FALSE, TRUE, 5, NOW()),
    (10021, 'default', 'llm.billing.mode', '"free"', 'select', 'llm', 'LLM billing mode', 'free = metering only (llm_logs.quota); metered = wallet pre-hold + settle on every request (design §10.3)', '{"values":["free","metered"]}', FALSE, TRUE, 10, NOW()),
    (10022, 'default', 'llm.billing.currency', '"CNY"', 'select', 'llm', 'LLM billing currency', 'Wallet currency charged in metered mode (tenant scope, global fallback; default CNY)', '{"values":["CNY","USD","EUR","GBP","JPY"]}', FALSE, TRUE, 11, NOW()),
    (10023, 'default', 'llm.billing.free_daily_user_quota', '0', 'integer', 'llm', 'Free-mode daily cap per user', 'Quota units per user per day in free mode (1000000 = $1); 0 = unlimited', '{"min":0,"max":10000000000}', FALSE, TRUE, 12, NOW()),
    (10024, 'default', 'llm.default_chat_model', 'null', 'text', 'llm', 'Default chat model', 'Chat/VLM model for internal callers without an explicit model (agent/flows/kb); blank clears', NULL, FALSE, TRUE, 13, NOW()),
    (10025, 'default', 'llm.default_embedding_model', 'null', 'text', 'llm', 'Default embedding model', 'Embedding model for internal callers without an explicit model; blank clears', NULL, FALSE, TRUE, 14, NOW()),
    (10018, 'default', 'llm_group_ratios', '"{\\"default\\":1.0}"', 'text', 'llm', 'LLM billing group ratios', 'JSON map of group name to sell-price multiplier (pricing.md §2)', NULL, FALSE, TRUE, 15, NOW()),
        (10026, 'default', 'llm.hold_leak_refund_percent', '0', 'integer', 'llm', 'Hold leak refund percent', 'Unknown-usage leaked holds: percent of the hold refunded by the reconcile sweep (0 = keep held for admin review, 100 = full refund)', '{"min":0,"max":100}', FALSE, TRUE, 18, NOW()),
    (10019, 'default', 'llm.log_retention_days', '90', 'integer', 'llm', 'LLM log retention (days)', 'Detail rows older than this are rolled up into llm_logs_summary then deleted (design §9)', '{"min":1,"max":3650}', FALSE, TRUE, 16, NOW()),
    (10020, 'default', 'llm.log_retention_days_test', '7', 'integer', 'llm', 'LLM test-log retention (days)', 'Shorter retention applied to source=test probe logs', '{"min":1,"max":3650}', FALSE, TRUE, 17, NOW());

-- ============================================================
-- Flow orchestration engine v2 (dev-docs/workflow) — P0-P1 5 tables
-- Data is engine-own (independent namespace); egress only referenced.
-- ============================================================

CREATE TABLE IF NOT EXISTS flow (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    description TEXT,
    enabled TINYINT(1) NOT NULL DEFAULT 1,
    current_version BIGINT NULL,
    extra JSON,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_flow_tenant (tenant_id)
);

-- Immutable definition snapshots (publish = append; instance locks a version)
CREATE TABLE IF NOT EXISTS flow_version (
    id BIGINT PRIMARY KEY,
    flow_id BIGINT NOT NULL,
    version_number BIGINT NOT NULL,
    definition JSON NOT NULL,
    expires_at DATETIME NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_flow_version (flow_id, version_number),
    INDEX idx_flow_version_flow (flow_id)
);

-- One run of a flow (wf_trace root = instance id)
CREATE TABLE IF NOT EXISTS flow_instance (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    flow_id BIGINT NOT NULL,
    flow_version_id BIGINT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'running',
    has_exceptions TINYINT(1) NOT NULL DEFAULT 0,
    trigger_kind VARCHAR(10) NOT NULL,
    trigger_payload JSON,
    inputs_summary JSON,
    outputs JSON,
    error JSON,
    started_by BIGINT NULL,
    started_at DATETIME NULL,
    finished_at DATETIME NULL,
    waiting_kind VARCHAR(10) NULL,
    waiting_needed BIGINT NULL,
    waiting_received BIGINT NOT NULL DEFAULT 0,
    resume_until DATETIME NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_flow_instance_flow_status (flow_id, status),
    INDEX idx_flow_instance_status_time (status, created_at),
    INDEX idx_flow_instance_waiting (status, resume_until)
);

-- Durable runnable state (whole snapshot, 1:1, rewritten each step)
CREATE TABLE IF NOT EXISTS flow_instance_snapshot (
    instance_id BIGINT PRIMARY KEY,
    snapshot JSON NOT NULL,
    snapshot_version BIGINT NOT NULL DEFAULT 1,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

-- Per-node run history (engine-own observability)
CREATE TABLE IF NOT EXISTS flow_node_run (
    id BIGINT PRIMARY KEY,
    instance_id BIGINT NOT NULL,
    node_id VARCHAR(64) NOT NULL,
    node_type VARCHAR(32) NOT NULL,
    seq BIGINT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 1,
    status VARCHAR(20) NOT NULL,
    started_at DATETIME NULL,
    finished_at DATETIME NULL,
    latency_ms BIGINT NULL,
    input_summary JSON,
    output_summary JSON,
    usage_json JSON,
    error JSON,
    egress_log_id BIGINT NULL,
    container_ref JSON,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_flow_node_run_instance_seq (instance_id, seq),
    INDEX idx_flow_node_run_egress (egress_log_id)
);

-- Public API keys for flows (external invocation auth)
CREATE TABLE IF NOT EXISTS flow_api_key (
    id BIGINT PRIMARY KEY,
    flow_id BIGINT NOT NULL,
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    token_enc TEXT NOT NULL,
    slug VARCHAR(40) NULL UNIQUE,
    enabled TINYINT(1) NOT NULL DEFAULT 1,
    require_auth TINYINT(1) NOT NULL DEFAULT 1,
    last_used_at DATETIME NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_flow_api_key_hash (token_hash),
    INDEX idx_flow_api_key_flow (flow_id)
);

-- Internal flow triggers (event/cron): point at a flow, decoupled from flow.
CREATE TABLE IF NOT EXISTS flow_trigger (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    flow_id BIGINT NOT NULL,
    kind VARCHAR(20) NOT NULL,
    name TEXT NOT NULL,
    event_type VARCHAR(255) NULL,
    filter JSON NULL,
    cron_expr VARCHAR(255) NULL,
    inputs_map JSON NULL,
    enabled TINYINT(1) NOT NULL DEFAULT 1,
    last_triggered_at DATETIME NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_flow_trigger_kind_event (kind, event_type),
    INDEX idx_flow_trigger_flow (flow_id)
);

-- Await claim ledger (dev-docs/workflow/await-node.md §3): one `open` row per
-- parked await node; resume closes it conditionally (idempotency, 409 on race).
CREATE TABLE IF NOT EXISTS flow_resume (
    id BIGINT PRIMARY KEY,
    instance_id BIGINT NOT NULL,
    node_id VARCHAR(64) NOT NULL,
    kind VARCHAR(10) NOT NULL,
    status VARCHAR(10) NOT NULL DEFAULT 'open',
    token_hash VARCHAR(64) NULL UNIQUE,
    token_enc TEXT NULL,
    resume_until DATETIME NULL,
    payload JSON NULL,
    resumed_by BIGINT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_flow_resume_instance_node (instance_id, node_id),
    INDEX idx_flow_resume_expiry (status, resume_until)
);

-- ─────────────────────────────────────────────────────────────────────────────
-- AI Agent core (namespace ai_*). See dev-docs/agent/db-schema.md.
-- Multi-tenant; ids are app-assigned Snowflake (BIGINT). JSON as JSON,
-- BOOLEAN as TINYINT(1), timestamps DATETIME.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ai_agents (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NULL,
    name VARCHAR(255) NOT NULL,
    system_prompt TEXT NOT NULL,
    provider VARCHAR(64) NOT NULL,
    model VARCHAR(255) NOT NULL,
    channel_id BIGINT NULL,
    temperature DOUBLE NULL,
    max_iterations INT NOT NULL DEFAULT 10,
    tools JSON NOT NULL,
    memory_enabled TINYINT(1) NOT NULL DEFAULT 1,
    params JSON NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, name),
    INDEX idx_ai_agents_tenant (tenant_id)
);

CREATE TABLE IF NOT EXISTS ai_sessions (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    agent_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    parent_id BIGINT NULL,
    title VARCHAR(500) NOT NULL DEFAULT '',
    status VARCHAR(32) NOT NULL DEFAULT 'open',
    meta JSON NULL,
    last_seq BIGINT NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    last_active_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_ai_sessions_tenant_agent (tenant_id, agent_id),
    INDEX idx_ai_sessions_owner_active (user_id, last_active_at)
);

CREATE TABLE IF NOT EXISTS ai_messages (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    session_id BIGINT NOT NULL,
    seq BIGINT NOT NULL,
    role VARCHAR(16) NOT NULL,
    kind VARCHAR(40) NOT NULL DEFAULT 'chat',
    content TEXT NOT NULL,
    tool_calls JSON NULL,
    tool_call_id VARCHAR(128) NULL,
    tool_name VARCHAR(128) NULL,
    tool_success TINYINT(1) NULL,
    tool_error TEXT NULL,
    tool_elapsed_ms BIGINT NULL,
    tool_truncated TINYINT(1) NULL,
    reasoning_content TEXT NULL,
    call_usage JSON NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (session_id, seq),
    INDEX idx_ai_messages_session_seq (session_id, seq),
    INDEX idx_ai_messages_session_role (session_id, role, seq)
);

CREATE TABLE IF NOT EXISTS ai_memories (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    agent_id BIGINT NOT NULL,
    user_id BIGINT,
    session_id BIGINT NULL,
    mem_key VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    category VARCHAR(40) NOT NULL DEFAULT 'core',
    importance DOUBLE NOT NULL DEFAULT 0.5,
    superseded_by BIGINT NULL,
    pinned BOOLEAN NOT NULL DEFAULT FALSE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, agent_id, user_id, mem_key),
    INDEX idx_ai_memories_agent_live (agent_id, superseded_by),
    INDEX idx_ai_memories_agent_category (agent_id, category)
);

-- A/B review debate run (multi-agent §6-§7, dev-docs/agent/multi-agent.md).
CREATE TABLE IF NOT EXISTS ai_debates (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'running',
    agent_a_id BIGINT NOT NULL,
    agent_b_id BIGINT NOT NULL,
    origin_session_id BIGINT NULL,
    requirement TEXT NOT NULL,
    params JSON NULL,
    ledger JSON NOT NULL,
    rounds_done INT NOT NULL DEFAULT 0,
    report TEXT NULL,
    usage_total JSON NULL,
    error TEXT NULL,
    heartbeat_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_ai_debates_tenant_status (tenant_id, status),
    INDEX idx_ai_debates_agent_a (agent_a_id),
    INDEX idx_ai_debates_agent_b (agent_b_id)
);

-- ── Knowledge base (kb-technical-design §2) ─────────────────────────
CREATE TABLE IF NOT EXISTS kb_knowledge_bases (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    description TEXT,
    slug VARCHAR(255) NOT NULL,
    kind VARCHAR(20) NOT NULL DEFAULT 'document',
    indexing_strategy JSON,
    chunking_config JSON,
    embedding_model VARCHAR(100),
    embedding_dim BIGINT,
    rerank_model VARCHAR(100),
    rerank_window BIGINT,
    rerank_threshold DOUBLE,
    chat_model VARCHAR(100),
    distill_model VARCHAR(100),
    image_config JSON,
    parser_config JSON,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_knowledge_bases_slug (slug),
    INDEX idx_kb_knowledge_bases_kind (kind, status)
);

CREATE TABLE IF NOT EXISTS kb_documents (
    id BIGINT PRIMARY KEY,
    kb_id BIGINT NOT NULL,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    title VARCHAR(255) NOT NULL,
    source VARCHAR(20) NOT NULL DEFAULT 'upload',
    storage_key VARCHAR(512),
    mime_type VARCHAR(100),
    size BIGINT NOT NULL DEFAULT 0,
    parse_format VARCHAR(20) NOT NULL DEFAULT 'markdown',
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    error TEXT,
    parse_degraded TEXT,
    pages BIGINT,
    parser_engine VARCHAR(50),
    chunk_count BIGINT NOT NULL DEFAULT 0,
    steps JSON,
    created_by BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_documents_kb (kb_id, status)
);

CREATE TABLE IF NOT EXISTS kb_chunks (
    id BIGINT PRIMARY KEY,
    kb_id BIGINT NOT NULL,
    doc_id BIGINT NULL,
    faq_id BIGINT NULL,
    wiki_page_id BIGINT NULL,
    kind VARCHAR(20) NOT NULL DEFAULT 'document',
    parent_id BIGINT NULL,
    seq BIGINT NOT NULL DEFAULT 0,
    content TEXT NOT NULL,
    breadcrumb VARCHAR(512),
    byte_start BIGINT NOT NULL DEFAULT 0,
    byte_end BIGINT NOT NULL DEFAULT 0,
    questions JSON,
    embedding BLOB,
    embedding_model VARCHAR(100),
    image_info JSON,
    page BIGINT,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_chunks_doc (doc_id),
    INDEX idx_kb_chunks_parent (parent_id),
    INDEX idx_kb_chunks_kind (kb_id, kind, status)
);

CREATE TABLE IF NOT EXISTS kb_wiki_pages (
    id BIGINT PRIMARY KEY,
    kb_id BIGINT NOT NULL,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    title VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL,
    folder VARCHAR(255),
    status VARCHAR(20) NOT NULL DEFAULT 'draft',
    content TEXT NOT NULL,
    summary TEXT,
    linked_page_ids JSON,
    current_revision BIGINT NOT NULL DEFAULT 1,
    reviewed_by BIGINT,
    created_by BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_wiki_pages_kb (kb_id, status)
);

CREATE TABLE IF NOT EXISTS kb_wiki_sources (
    id BIGINT PRIMARY KEY,
    page_id BIGINT NOT NULL,
    page_revision BIGINT NOT NULL DEFAULT 1,
    doc_id BIGINT NOT NULL,
    chunk_id BIGINT NULL,
    span_start BIGINT NOT NULL DEFAULT 0,
    span_end BIGINT NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_kb_wiki_sources_page (page_id, page_revision),
    INDEX idx_kb_wiki_sources_doc (doc_id)
);

CREATE TABLE IF NOT EXISTS kb_faqs (
    id BIGINT PRIMARY KEY,
    kb_id BIGINT NOT NULL,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    standard_question TEXT NOT NULL,
    similar_questions JSON,
    answers JSON NOT NULL,
    tags JSON,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    hit_count BIGINT NOT NULL DEFAULT 0,
    created_by BIGINT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_faqs_kb (kb_id, enabled)
);

CREATE TABLE IF NOT EXISTS kb_images (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    kb_id BIGINT NOT NULL,
    doc_id BIGINT NOT NULL,
    chunk_id BIGINT,
    page BIGINT,
    storage_key VARCHAR(255),
    mime_type VARCHAR(100) NOT NULL,
    bytes BIGINT,
    source VARCHAR(20) NOT NULL DEFAULT 'embedded',
    original_url TEXT,
    caption TEXT,
    ocr_text TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    error TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_images_doc (doc_id),
    INDEX idx_kb_images_status (kb_id, status)
);

CREATE TABLE IF NOT EXISTS kb_query_logs (
    id BIGINT PRIMARY KEY,
    kb_id BIGINT NULL,
    question TEXT NOT NULL,
    answer TEXT,
    cited_units JSON,
    status VARCHAR(20) NOT NULL DEFAULT 'answered',
    top_score DOUBLE,
    feedback BIGINT,
    user_id BIGINT,
    rewritten_question TEXT,
    kb_ids JSON,
    latency_ms BIGINT,
    run_id BIGINT,
    error TEXT,
    source VARCHAR(16) NOT NULL DEFAULT 'ask',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_kb_query_logs_status (status, created_at)
);

CREATE TABLE IF NOT EXISTS kb_runs (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    kind VARCHAR(32) NOT NULL,
    trigger_src VARCHAR(16) NOT NULL,
    kb_id BIGINT,
    doc_id BIGINT,
    agent_id BIGINT,
    session_id BIGINT,
    job_id BIGINT,
    attempt BIGINT NOT NULL DEFAULT 1,
    status VARCHAR(16) NOT NULL DEFAULT 'running',
    latency_ms BIGINT,
    error TEXT,
    config_snapshot JSON,
    stages JSON,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_kb_runs_doc (doc_id, created_at),
    INDEX idx_kb_runs_kb (kb_id, kind, created_at),
    INDEX idx_kb_runs_agent (agent_id, created_at),
    INDEX idx_kb_runs_kind (kind, status, created_at),
    INDEX idx_kb_runs_sweep (status, created_at)
);

-- ===== LLM foundation (dev-docs/llm/design.md §5) =====

CREATE TABLE IF NOT EXISTS llm_channels (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    provider VARCHAR(50) NOT NULL,
    base_url VARCHAR(500) NOT NULL,
    api_keys JSON NOT NULL,
    key_mode VARCHAR(20) NOT NULL DEFAULT 'polling',
    status VARCHAR(20) NOT NULL DEFAULT 'enabled',
    models TEXT NOT NULL,
    model_mapping JSON,
    priority BIGINT NOT NULL DEFAULT 0,
    weight INT NOT NULL DEFAULT 0,
    channel_groups TEXT NOT NULL,
    auto_ban TINYINT(1) NOT NULL DEFAULT 1,
    param_override JSON,
    header_override JSON,
    config JSON,
    used_quota BIGINT NOT NULL DEFAULT 0,
    cost_mode VARCHAR(10) NOT NULL DEFAULT 'usage',
    cost_discount DOUBLE NOT NULL DEFAULT 1.0,
    monthly_cost DOUBLE,
    test_model VARCHAR(255),
    test_time DATETIME,
    response_time INT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, name),
    INDEX idx_llm_channels_tenant (tenant_id, status)
);

CREATE TABLE IF NOT EXISTS llm_tokens (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    name VARCHAR(255) NOT NULL,
    key_hash VARCHAR(128) NOT NULL UNIQUE,
    key_enc TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'enabled',
    remain_quota BIGINT NOT NULL DEFAULT 0,
    used_quota BIGINT NOT NULL DEFAULT 0,
    unlimited_quota TINYINT(1) NOT NULL DEFAULT 0,
    expired_at DATETIME,
    allowed_models TEXT,
    allowed_ips TEXT,
    token_group VARCHAR(255),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    accessed_at DATETIME,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_llm_tokens_user (user_id)
);

CREATE TABLE IF NOT EXISTS llm_models (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    model_type VARCHAR(20) NOT NULL DEFAULT 'chat',
    price_mode VARCHAR(20) NOT NULL DEFAULT 'token',
    input_price DOUBLE NOT NULL DEFAULT 1.0,
    output_price DOUBLE NOT NULL DEFAULT 1.0,
    cache_read_price DOUBLE,
    cache_write_price DOUBLE,
    call_price DOUBLE,
    params JSON,
    status VARCHAR(20) NOT NULL DEFAULT 'active',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, name)
);

CREATE TABLE IF NOT EXISTS llm_logs (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    request_id VARCHAR(64),
    user_id BIGINT,
    token_id BIGINT,
    source VARCHAR(20) NOT NULL DEFAULT 'relay',
    channel_id BIGINT,
    key_index INT,
    model_name VARCHAR(255) NOT NULL,
    is_stream TINYINT(1) NOT NULL DEFAULT 0,
    prompt_tokens INT NOT NULL DEFAULT 0,
    completion_tokens INT NOT NULL DEFAULT 0,
    cache_read_tokens INT NOT NULL DEFAULT 0,
    cache_write_tokens INT NOT NULL DEFAULT 0,
    quota BIGINT NOT NULL DEFAULT 0,
    cost_quota BIGINT NOT NULL DEFAULT 0,
    detail JSON,
    elapsed_ms INT,
    status_code INT,
    error_message TEXT,
    day VARCHAR(10) NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_llm_logs_tenant (tenant_id, created_at),
    INDEX idx_llm_logs_day (tenant_id, day),
    INDEX idx_llm_logs_channel (channel_id, created_at),
    INDEX idx_llm_logs_token (token_id, created_at)
);

-- Archived daily rollup of llm_logs (design §9 retention / §16 汇总表).
CREATE TABLE IF NOT EXISTS llm_logs_summary (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    day VARCHAR(10) NOT NULL,
    source VARCHAR(20) NOT NULL DEFAULT 'relay',
    model_name VARCHAR(255) NOT NULL DEFAULT '',
    channel_id BIGINT NOT NULL DEFAULT 0,
    user_id BIGINT NOT NULL DEFAULT 0,
    request_count BIGINT NOT NULL DEFAULT 0,
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    cache_write_tokens BIGINT NOT NULL DEFAULT 0,
    quota BIGINT NOT NULL DEFAULT 0,
    cost_quota BIGINT NOT NULL DEFAULT 0,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_llm_logs_summary (tenant_id, day, source, model_name, channel_id, user_id),
    INDEX idx_llm_logs_summary_day (tenant_id, day)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Generic LLM async tasks (video now; batch/image-async later). Lifecycle
-- columns are strongly typed; kind-specific request/response live in
-- payload/result JSON (llm_logs source+detail pattern).
CREATE TABLE IF NOT EXISTS llm_tasks (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    kind VARCHAR(20) NOT NULL,
    user_id BIGINT,
    token_id BIGINT,
    channel_id BIGINT,
    key_index INT,
    upstream_task_id VARCHAR(128),
    status VARCHAR(20) NOT NULL DEFAULT 'queued',
    progress INT NOT NULL DEFAULT 0,
    model_name VARCHAR(255) NOT NULL,
    pre_consumed BIGINT NOT NULL DEFAULT 0,
    quota BIGINT NOT NULL DEFAULT 0,
    cost_quota BIGINT NOT NULL DEFAULT 0,
    unlimited_quota TINYINT(1) NOT NULL DEFAULT 0,
    payload JSON,
    result JSON,
    error_message TEXT,
    expires_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_llm_tasks_status (status, updated_at),
    INDEX idx_llm_tasks_token (token_id, created_at),
    INDEX idx_llm_tasks_upstream (upstream_task_id)
);

CREATE TABLE IF NOT EXISTS redemption_codes (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    code_hash VARCHAR(128) NOT NULL UNIQUE,
    code_enc TEXT,
    user_id BIGINT NULL,
    currency VARCHAR(50) NOT NULL,
    amount BIGINT NOT NULL CHECK(amount > 0),
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    redeemed_by BIGINT NULL,
    redeemed_at DATETIME NULL,
    redemption_tx_no TEXT,
    created_by BIGINT NULL,
    expires_at DATETIME NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

-- ── 文档转换/图像识别用量账本（dev-docs/document/service-design.md §7）────
CREATE TABLE IF NOT EXISTS docparse_job_logs (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    kind VARCHAR(32) NOT NULL,
    status VARCHAR(32) NOT NULL,
    engine VARCHAR(64) NULL,
    model VARCHAR(128) NULL,
    filename VARCHAR(500) NULL,
    input_bytes BIGINT,
    pages BIGINT,
    chars BIGINT,
    duration_ms BIGINT,
    error TEXT NULL,
    result_key VARCHAR(500) NULL,
    created_at DATETIME NOT NULL,
    finished_at DATETIME NULL,
    INDEX idx_docparse_job_logs_tenant (tenant_id, created_at)
);
CREATE TABLE IF NOT EXISTS docparse_engines (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    engine_name VARCHAR(64) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    price_per_page BIGINT NOT NULL DEFAULT 0,
    price_per_call BIGINT NOT NULL DEFAULT 0,
    cost_per_page BIGINT NOT NULL DEFAULT 0,
    category VARCHAR(16) NOT NULL DEFAULT 'document',
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_docparse_engines_tenant_engine (tenant_id, engine_name)
);
ALTER TABLE docparse_job_logs ADD COLUMN billing_mode VARCHAR(16) DEFAULT 'post';
ALTER TABLE docparse_job_logs ADD COLUMN price_charged BIGINT;
ALTER TABLE docparse_job_logs ADD COLUMN payment_status VARCHAR(16);

CREATE TABLE IF NOT EXISTS docparse_tokens (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    user_id BIGINT NOT NULL DEFAULT 0,
    name VARCHAR(200) NOT NULL,
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    token_prefix VARCHAR(16) NOT NULL,
    token_enc TEXT,
    status VARCHAR(32) NOT NULL DEFAULT 'active',
    daily_page_quota BIGINT NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME NULL,
    INDEX idx_docparse_tokens_hash (token_hash),
    INDEX idx_docparse_tokens_tenant (tenant_id)
);

CREATE TABLE IF NOT EXISTS docparse_usage_summary (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL,
    kind VARCHAR(32) NOT NULL,
    engine VARCHAR(64) NOT NULL,
    date VARCHAR(16) NOT NULL,
    total_jobs INTEGER NOT NULL DEFAULT 0,
    completed_jobs INTEGER NOT NULL DEFAULT 0,
    failed_jobs INTEGER NOT NULL DEFAULT 0,
    total_pages BIGINT NOT NULL DEFAULT 0,
    total_chars BIGINT NOT NULL DEFAULT 0,
    total_duration_ms BIGINT NOT NULL DEFAULT 0,
    total_price_charged BIGINT NOT NULL DEFAULT 0,
    UNIQUE KEY uk_parse_summary (tenant_id, kind, engine, date)
);
