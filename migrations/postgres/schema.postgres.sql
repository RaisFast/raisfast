-- ============================================================
-- raisfast complete database schema — PostgreSQL (with multi-tenant support)
-- Merged from all migration files for one-click initialization of new deployments
-- Generated date：2026-05-07
-- ============================================================

-- ── Platform foundation layer (always enabled) ──────────────────────────────────

-- Tenants
CREATE TABLE IF NOT EXISTS tenants (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    domain VARCHAR(255) UNIQUE,
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

-- Users
CREATE TABLE IF NOT EXISTS users (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    username VARCHAR(255) UNIQUE NOT NULL,
    avatar VARCHAR(500),
    bio TEXT,
    website VARCHAR(500),
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    registered_via VARCHAR(100) NOT NULL,
    display_name VARCHAR(100),
    slug VARCHAR(100) UNIQUE,
    locale VARCHAR(10),
    social_links JSONB,
    metadata JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_tenant ON users(tenant_id);

-- User credentials
CREATE TABLE IF NOT EXISTS user_credentials (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    auth_type VARCHAR(100) NOT NULL,
    identifier VARCHAR(500) NOT NULL,
    credential_data JSONB NOT NULL,
    verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(auth_type, identifier)
);

CREATE INDEX IF NOT EXISTS idx_user_credentials_user ON user_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_user_credentials_type ON user_credentials(auth_type);

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
    token_expires_at TIMESTAMPTZ,
    profile TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(provider, provider_user_id)
);

CREATE INDEX IF NOT EXISTS idx_oauth_accounts_user ON oauth_accounts(user_id);

-- OAuth short-lived state storage (PKCE)
CREATE TABLE IF NOT EXISTS oauth_states (
    id BIGINT PRIMARY KEY,
    provider VARCHAR(50) NOT NULL,
    code_verifier VARCHAR(255) NOT NULL,
    user_id BIGINT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oauth_states_expires ON oauth_states(expires_at);

-- Currency configuration
CREATE TABLE IF NOT EXISTS currencies (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL DEFAULT 'default',
    code VARCHAR(10) NOT NULL CHECK(code = UPPER(code) AND LENGTH(code) BETWEEN 1 AND 10),
    name TEXT NOT NULL,
    decimals BIGINT NOT NULL DEFAULT 0 CHECK(decimals BETWEEN 0 AND 18),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    version BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, code)
);

CREATE TABLE IF NOT EXISTS wallets (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    currency VARCHAR(50) NOT NULL,
    balance BIGINT NOT NULL DEFAULT 0 CHECK(balance >= 0),
    version BIGINT NOT NULL DEFAULT 1,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, currency)
);

CREATE INDEX IF NOT EXISTS idx_wallets_currency ON wallets(currency);
CREATE INDEX IF NOT EXISTS idx_wallets_tenant ON wallets(tenant_id);

CREATE TABLE IF NOT EXISTS wallet_transactions (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
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
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_wallet_tx_wallet ON wallet_transactions(wallet_id);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_user ON wallet_transactions(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_reference ON wallet_transactions(reference_type, reference_id);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_tenant_user ON wallet_transactions(tenant_id, user_id, created_at DESC);

-- Refresh Tokens
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    token VARCHAR(500) UNIQUE NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);

-- Site options
CREATE TABLE IF NOT EXISTS options (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    option_key VARCHAR(255) NOT NULL,
    value JSONB NOT NULL,
    type VARCHAR(50) NOT NULL DEFAULT 'text',
    group_name VARCHAR(100) NOT NULL DEFAULT 'general',
    label VARCHAR(255) NOT NULL DEFAULT '',
    description TEXT,
    validation JSONB,
    is_public BOOLEAN NOT NULL DEFAULT FALSE,
    autoload BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, option_key)
);

-- RBAC roles
CREATE TABLE IF NOT EXISTS roles (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    description TEXT,
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, name)
);

CREATE INDEX IF NOT EXISTS idx_roles_tenant ON roles(tenant_id);

-- RBAC permissions
CREATE TABLE IF NOT EXISTS permissions (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    role_id BIGINT NOT NULL,
    action VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    fields JSONB,
    conditions JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(role_id, action, subject)
);

CREATE INDEX IF NOT EXISTS idx_permissions_tenant ON permissions(tenant_id);

-- User-role assignments (many-to-many)
CREATE TABLE IF NOT EXISTS user_roles (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, user_id, role_id)
);

CREATE INDEX IF NOT EXISTS idx_user_roles_user ON user_roles(user_id);
CREATE INDEX IF NOT EXISTS idx_user_roles_role ON user_roles(role_id);
CREATE INDEX IF NOT EXISTS idx_user_roles_tenant ON user_roles(tenant_id);

-- Audit log
CREATE TABLE IF NOT EXISTS audit_log (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    actor_id BIGINT,
    actor_role VARCHAR(50),
    action VARCHAR(255) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    subject_id VARCHAR(36),
    detail JSONB,
    ip_address VARCHAR(45),
    user_agent TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_tenant_created ON audit_log(tenant_id, created_at DESC);

-- API Token
CREATE TABLE IF NOT EXISTS api_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    name VARCHAR(255) NOT NULL,
    token_hash VARCHAR(255) UNIQUE NOT NULL,
    token_encrypted TEXT NOT NULL,
    description TEXT DEFAULT '',
    scopes JSONB NOT NULL,
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_tokens_user_id ON api_tokens(user_id);

-- Webhook subscriptions
CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL DEFAULT '',
    url VARCHAR(1024) NOT NULL,
    secret VARCHAR(255) NOT NULL,
    events JSONB NOT NULL DEFAULT '[]'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    description TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_webhook_subscriptions_enabled ON webhook_subscriptions(enabled);
CREATE INDEX IF NOT EXISTS idx_webhook_subscriptions_tenant ON webhook_subscriptions(tenant_id);

-- Webhook delivery log
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id BIGINT PRIMARY KEY,
    webhook_id BIGINT NOT NULL,
    event VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL,
    status_code INTEGER,
    error TEXT,
    duration_ms BIGINT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_webhook ON webhook_deliveries(webhook_id);
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_created ON webhook_deliveries(created_at DESC);

-- Plugin KV storage
CREATE TABLE IF NOT EXISTS plugin_storage (
    plugin_id VARCHAR(100) NOT NULL,
    storage_key VARCHAR(255) NOT NULL,
    value JSONB NOT NULL,
    expires_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    PRIMARY KEY (plugin_id, storage_key)
);

CREATE INDEX IF NOT EXISTS idx_plugin_storage_plugin ON plugin_storage(plugin_id);

-- Content revision history
CREATE TABLE IF NOT EXISTS content_revisions (
    id BIGINT PRIMARY KEY,
    content_type VARCHAR(100) NOT NULL,
    record_id BIGINT NOT NULL,
    revision_number BIGINT NOT NULL,
    snapshot JSONB NOT NULL,
    created_by BIGINT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(content_type, record_id, revision_number)
);

CREATE INDEX IF NOT EXISTS idx_revisions_ct_record_rev
    ON content_revisions(content_type, record_id, revision_number DESC);

-- Password reset tokens
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    token VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user_id ON password_reset_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_expires_at ON password_reset_tokens(expires_at);

-- SMS verification codes
CREATE TABLE IF NOT EXISTS sms_codes (
    id BIGINT PRIMARY KEY,
    phone VARCHAR(50) NOT NULL,
    code VARCHAR(20) NOT NULL,
    purpose VARCHAR(50) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    verified_at TIMESTAMPTZ,
    attempts BIGINT NOT NULL DEFAULT 0,
    ip_address VARCHAR(45),
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sms_codes_phone ON sms_codes(phone);
CREATE INDEX IF NOT EXISTS idx_sms_codes_expires ON sms_codes(expires_at);

-- User device codes (IDE authentication)
CREATE TABLE IF NOT EXISTS user_device_codes (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    code VARCHAR(255) NOT NULL UNIQUE,
    access_token TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_device_codes_code ON user_device_codes(code);
CREATE INDEX IF NOT EXISTS idx_user_device_codes_user_id ON user_device_codes(user_id);
CREATE INDEX IF NOT EXISTS idx_user_device_codes_expires_at ON user_device_codes(expires_at);

-- Email verification tokens
CREATE TABLE IF NOT EXISTS email_verification_tokens (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    token VARCHAR(255) NOT NULL UNIQUE,
    email VARCHAR(255) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    verified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_email_verification_tokens_user_id ON email_verification_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_email_verification_tokens_expires ON email_verification_tokens(expires_at);

-- Background job queue
CREATE TABLE IF NOT EXISTS jobs (
    id               BIGINT PRIMARY KEY,
    job_type         VARCHAR(100) NOT NULL,
    payload          JSONB NOT NULL,
    status           VARCHAR(50) NOT NULL DEFAULT 'pending',
    attempts         INTEGER NOT NULL DEFAULT 0,
    max_attempts     INTEGER NOT NULL DEFAULT 3,
    run_after        TIMESTAMPTZ,
    error            TEXT,
    cron_schedule_id BIGINT,
    cron_log_id      BIGINT,
    priority         SMALLINT NOT NULL DEFAULT 0,
    timeout_secs     INTEGER,
    dedup_key        VARCHAR(255),
    created_at       TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_jobs_status_run_after ON jobs(status, run_after) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_jobs_type ON jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_jobs_dedup_key ON jobs(dedup_key) WHERE dedup_key IS NOT NULL;

-- Cron job schedules
CREATE TABLE IF NOT EXISTS cron_schedules (
    id           BIGINT PRIMARY KEY,
    label        VARCHAR(255) NOT NULL,
    job_type     VARCHAR(100) NOT NULL,
    payload      JSONB,
    cron_expr    VARCHAR(100) NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at  TIMESTAMPTZ,
    next_run_at  TIMESTAMPTZ NOT NULL,
    plugin_id    VARCHAR(100),
    exec_kind    VARCHAR(20) NOT NULL DEFAULT 'builtin',
    handler_id   VARCHAR(100),
    params       JSONB,
    script_lang    VARCHAR(20),
    script_source  TEXT,
    script_entry   VARCHAR(100) NOT NULL DEFAULT 'on_cron_tick',
    use_shell      BOOLEAN NOT NULL DEFAULT TRUE,
    timeout_secs   INTEGER,
    created_at   TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_cron_enabled ON cron_schedules(enabled);
CREATE INDEX IF NOT EXISTS idx_cron_next_run ON cron_schedules(next_run_at) WHERE enabled = TRUE;
CREATE INDEX IF NOT EXISTS idx_cron_plugin ON cron_schedules(plugin_id);

-- Cron execution log
CREATE TABLE IF NOT EXISTS cron_execution_log (
    id           BIGINT PRIMARY KEY,
    schedule_id  BIGINT NOT NULL,
    job_type     VARCHAR(100) NOT NULL,
    label        VARCHAR(255) NOT NULL,
    status       VARCHAR(50) NOT NULL DEFAULT 'running',
    duration_ms  BIGINT,
    error        TEXT,
    started_at   TIMESTAMPTZ NOT NULL,
    finished_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_cron_log_schedule ON cron_execution_log(schedule_id);
CREATE INDEX IF NOT EXISTS idx_cron_log_status ON cron_execution_log(status);
CREATE INDEX IF NOT EXISTS idx_cron_log_started ON cron_execution_log(started_at);

-- ── Built-in module: Blog (BUILTIN_BLOG=true) ──────────────────

-- Categories
CREATE TABLE IF NOT EXISTS categories (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL,
    description TEXT,
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
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, name),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_categories_tenant ON categories(tenant_id);

-- Product categories
CREATE TABLE IF NOT EXISTS product_categories (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
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
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, name),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_product_categories_tenant ON product_categories(tenant_id);

-- Tags
CREATE TABLE IF NOT EXISTS tags (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL,
    description TEXT,
    cover_image VARCHAR(500),
    meta_title VARCHAR(255),
    meta_description VARCHAR(500),
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    og_image VARCHAR(500),
    created_by BIGINT,
    updated_by BIGINT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, name),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_tags_tenant ON tags(tenant_id);

-- Posts
CREATE TABLE IF NOT EXISTS posts (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    title TEXT NOT NULL,
    slug VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    excerpt TEXT,
    cover_image VARCHAR(500),
    image_ids TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    created_by BIGINT NOT NULL,
    updated_by BIGINT,
    category_id BIGINT,
    view_count BIGINT NOT NULL DEFAULT 0,
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
    reading_time BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_posts_status ON posts(status);
CREATE INDEX IF NOT EXISTS idx_posts_author ON posts(created_by);
CREATE INDEX IF NOT EXISTS idx_posts_category ON posts(category_id);
CREATE INDEX IF NOT EXISTS idx_posts_status_created
    ON posts(status, is_pinned DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_posts_status_category
    ON posts(status, category_id);
CREATE INDEX IF NOT EXISTS idx_posts_status_author
    ON posts(status, created_by);
CREATE INDEX IF NOT EXISTS idx_posts_tenant ON posts(tenant_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_posts_tenant_slug ON posts(tenant_id, slug);

-- Posts-Tags (many-to-many)
CREATE TABLE IF NOT EXISTS posts_tags (
    post_id BIGINT NOT NULL,
    tag_id BIGINT NOT NULL,
    PRIMARY KEY (post_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_posts_tags_tag_id ON posts_tags(tag_id);

CREATE TABLE IF NOT EXISTS taggings (
    id BIGINT PRIMARY KEY,
    tag_id BIGINT NOT NULL,
    taggable_type VARCHAR(50) NOT NULL,
    taggable_id BIGINT NOT NULL,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    UNIQUE(tenant_id, tag_id, taggable_type, taggable_id)
);

CREATE INDEX IF NOT EXISTS idx_taggings_tag ON taggings(tag_id);
CREATE INDEX IF NOT EXISTS idx_taggings_taggable ON taggings(taggable_type, taggable_id);

-- Comments
CREATE TABLE IF NOT EXISTS comments (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
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
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_comments_post ON comments(post_id);
CREATE INDEX IF NOT EXISTS idx_comments_status ON comments(status);
CREATE INDEX IF NOT EXISTS idx_comments_post_status
    ON comments(post_id, status);
CREATE INDEX IF NOT EXISTS idx_comments_parent_id
    ON comments(parent_id);
CREATE INDEX IF NOT EXISTS idx_comments_tenant ON comments(tenant_id);

-- ── Built-in module: Pages (BUILTIN_PAGES=true) ────────────────

CREATE TABLE IF NOT EXISTS pages (
    id               BIGINT PRIMARY KEY,
    tenant_id        TEXT NOT NULL DEFAULT 'default',
    title            TEXT NOT NULL,
    slug             VARCHAR(255) NOT NULL UNIQUE,
    content          TEXT,
    blocks           JSONB,
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
    published_at     TIMESTAMPTZ,
    password VARCHAR(255),
    comment_status VARCHAR(20) NOT NULL DEFAULT 'closed',
    og_title VARCHAR(255),
    og_description VARCHAR(500),
    canonical_url VARCHAR(1024),
    created_at       TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_pages_status    ON pages(status);
CREATE INDEX IF NOT EXISTS idx_pages_parent    ON pages(parent_id);
CREATE INDEX IF NOT EXISTS idx_pages_author    ON pages(created_by);
CREATE INDEX IF NOT EXISTS idx_pages_tenant_slug ON pages(tenant_id, slug);
CREATE INDEX IF NOT EXISTS idx_pages_tenant_status ON pages(tenant_id, status);

CREATE TABLE IF NOT EXISTS reusable_blocks (
    id          BIGINT PRIMARY KEY,
    tenant_id   TEXT NOT NULL DEFAULT 'default',
    name        VARCHAR(255) NOT NULL,
    block_type  TEXT NOT NULL,
    content     TEXT NOT NULL,
    description TEXT,
    created_by  BIGINT,
    updated_by  BIGINT,
    created_at  TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reusable_blocks_tenant ON reusable_blocks(tenant_id);

-- ── Built-in module: Media (BUILTIN_MEDIA=true) ────────────────

CREATE TABLE IF NOT EXISTS media (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    filename VARCHAR(255) NOT NULL,
    filepath VARCHAR(500) NOT NULL,
    mimetype VARCHAR(100) NOT NULL,
size BIGINT NOT NULL,
    width INTEGER,
    height INTEGER,
    title VARCHAR(255),
    alt_text VARCHAR(255),
    caption TEXT,
    description TEXT,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_media_user_created
    ON media(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_media_tenant ON media(tenant_id);

-- ── Built-in module: Workflow (BUILTIN_WORKFLOW=true) ──────────

CREATE TABLE IF NOT EXISTS workflow_definitions (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    steps JSONB NOT NULL,
    initial_step VARCHAR(100) NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS workflow_instances (
    id BIGINT PRIMARY KEY,
    definition_id BIGINT NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'running',
    current_step VARCHAR(100),
    context JSONB NOT NULL DEFAULT '{}'::jsonb,
    triggered_by BIGINT,
    started_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_wf_instances_definition ON workflow_instances(definition_id);
CREATE INDEX IF NOT EXISTS idx_wf_instances_status ON workflow_instances(status);

CREATE TABLE IF NOT EXISTS workflow_step_logs (
    id BIGINT PRIMARY KEY,
    instance_id BIGINT NOT NULL,
    step_id VARCHAR(100) NOT NULL,
    step_name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'running',
    input JSONB,
    output JSONB,
    error TEXT,
    started_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wf_step_logs_instance ON workflow_step_logs(instance_id);

-- Products
CREATE TABLE IF NOT EXISTS products (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    category_id BIGINT,
    title TEXT NOT NULL,
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
    attributes JSONB,
    sort_order BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    slug VARCHAR(255),
    content TEXT,
    image_ids TEXT,
    original_price BIGINT,
    specs JSONB,
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
    published_at TIMESTAMPTZ,
    stock BIGINT NOT NULL DEFAULT 0,
    cost_price BIGINT,
    sale_price BIGINT,
    has_variants BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_products_status ON products(status);
CREATE INDEX IF NOT EXISTS idx_products_type ON products(product_type);
CREATE INDEX IF NOT EXISTS idx_products_tenant ON products(tenant_id);
CREATE INDEX IF NOT EXISTS idx_products_tenant_status ON products(tenant_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_products_tenant_slug ON products(tenant_id, slug);

-- Product Variants
CREATE TABLE IF NOT EXISTS product_variants (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    product_id BIGINT NOT NULL,
    sku VARCHAR(100) UNIQUE,
    title TEXT NOT NULL,
    price BIGINT NOT NULL CHECK(price >= 0),
    original_price BIGINT,
    stock BIGINT NOT NULL DEFAULT 0,
    attributes JSONB,
    image_url TEXT,
    weight BIGINT,
    sort_order BIGINT NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_product_variants_product ON product_variants(product_id);
CREATE INDEX IF NOT EXISTS idx_product_variants_tenant ON product_variants(tenant_id);

-- User Addresses
CREATE TABLE IF NOT EXISTS user_addresses (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
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
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_addresses_user ON user_addresses(user_id);
CREATE INDEX IF NOT EXISTS idx_user_addresses_tenant ON user_addresses(tenant_id);

-- Orders
CREATE TABLE IF NOT EXISTS orders (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
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
    paid_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    refunding_at TIMESTAMPTZ,
    refunded_at TIMESTAMPTZ,
    expired_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_orders_user ON orders(user_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
CREATE INDEX IF NOT EXISTS idx_orders_tenant ON orders(tenant_id);
CREATE INDEX IF NOT EXISTS idx_orders_tenant_user_status ON orders(tenant_id, user_id, status);
CREATE INDEX IF NOT EXISTS idx_orders_tenant_status_created ON orders(tenant_id, status, created_at DESC);

-- Order Items
CREATE TABLE IF NOT EXISTS order_items (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    order_id BIGINT NOT NULL,
    product_id BIGINT,
    variant_id BIGINT,
    title TEXT NOT NULL,
    description TEXT,
    sku VARCHAR(100),
    unit_price BIGINT NOT NULL CHECK(unit_price >= 0),
    quantity BIGINT NOT NULL CHECK(quantity > 0),
    subtotal BIGINT NOT NULL,
    tax_amount BIGINT NOT NULL DEFAULT 0,
    cover_url VARCHAR(500),
    attributes JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_order_items_order ON order_items(order_id);
CREATE INDEX IF NOT EXISTS idx_order_items_product ON order_items(product_id);
CREATE INDEX IF NOT EXISTS idx_order_items_tenant ON order_items(tenant_id);

CREATE TABLE IF NOT EXISTS cart_items (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    variant_id BIGINT,
    quantity BIGINT NOT NULL DEFAULT 1,
    attributes TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, product_id, variant_id)
);
CREATE INDEX IF NOT EXISTS idx_cart_items_tenant ON cart_items(tenant_id);

CREATE TABLE IF NOT EXISTS payment_channels (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    provider VARCHAR(50) NOT NULL,
    name VARCHAR(200) NOT NULL,
    is_live BOOLEAN NOT NULL DEFAULT FALSE,
    credentials JSONB NOT NULL,
    webhook_secret TEXT,
    settings JSONB,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE(provider, name)
);

CREATE INDEX IF NOT EXISTS idx_payment_channels_provider ON payment_channels(provider);
CREATE INDEX IF NOT EXISTS idx_payment_channels_active ON payment_channels(is_active);
CREATE INDEX IF NOT EXISTS idx_payment_channels_tenant ON payment_channels(tenant_id);

-- Payment Orders
CREATE TABLE IF NOT EXISTS payment_orders (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    order_id VARCHAR(36),
    title TEXT NOT NULL,
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
    metadata JSONB,
    paid_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    expired_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_payment_orders_user ON payment_orders(user_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_status ON payment_orders(status);
CREATE INDEX IF NOT EXISTS idx_payment_orders_provider ON payment_orders(provider_order_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_order_id ON payment_orders(order_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_tenant ON payment_orders(tenant_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_tenant_status_created ON payment_orders(tenant_id, status, created_at DESC);

-- Payment Transactions
CREATE TABLE IF NOT EXISTS payment_transactions (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    payment_order_id BIGINT NOT NULL,
    order_id VARCHAR(36),
    user_id BIGINT NOT NULL,
    tx_type VARCHAR(50) NOT NULL,
    amount BIGINT NOT NULL,
    currency VARCHAR(10) NOT NULL,
    provider_tx_id VARCHAR(200) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    raw_payload TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_payment_tx_order ON payment_transactions(payment_order_id);
CREATE INDEX IF NOT EXISTS idx_payment_tx_order_id ON payment_transactions(order_id);
CREATE INDEX IF NOT EXISTS idx_payment_transactions_tenant ON payment_transactions(tenant_id);

-- Payment Refunds
CREATE TABLE IF NOT EXISTS payment_refunds (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
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
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_payment_refunds_order ON payment_refunds(payment_order_id);
CREATE INDEX IF NOT EXISTS idx_payment_refunds_order_id ON payment_refunds(order_id);
CREATE INDEX IF NOT EXISTS idx_payment_refunds_tenant ON payment_refunds(tenant_id);

-- Wallet Outbox (ensures wallet operations are never lost)
CREATE TABLE IF NOT EXISTS wallet_outbox (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    currency TEXT NOT NULL,
    amount BIGINT NOT NULL,
    entry_type TEXT NOT NULL,
    tx_type TEXT NOT NULL,
    transaction_no TEXT NOT NULL,
    reference_type TEXT,
    reference_id TEXT,
    metadata TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts BIGINT NOT NULL DEFAULT 0,
    max_attempts BIGINT NOT NULL DEFAULT 5,
    last_error TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_wallet_outbox_status ON wallet_outbox(status);
CREATE INDEX IF NOT EXISTS idx_wallet_outbox_transaction_no ON wallet_outbox(transaction_no);
CREATE INDEX IF NOT EXISTS idx_wallet_outbox_tenant ON wallet_outbox(tenant_id);

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
    admin_replied_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_product_comments_unique ON product_comments(product_id, order_id, user_id);
CREATE INDEX IF NOT EXISTS idx_product_comments_user ON product_comments(user_id);
CREATE INDEX IF NOT EXISTS idx_product_comments_status ON product_comments(status);
CREATE INDEX IF NOT EXISTS idx_product_comments_tenant ON product_comments(tenant_id);
-- Product Favorites (wishlist)
CREATE TABLE IF NOT EXISTS product_favorites (
    id BIGINT PRIMARY KEY,
    tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_product_favorites_unique ON product_favorites(user_id, product_id);
CREATE INDEX IF NOT EXISTS idx_product_favorites_user ON product_favorites(user_id);
CREATE INDEX IF NOT EXISTS idx_product_favorites_product ON product_favorites(product_id);
CREATE INDEX IF NOT EXISTS idx_product_favorites_tenant ON product_favorites(tenant_id);


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
    starts_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    status VARCHAR(32) NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_coupons_status ON coupons(status);
CREATE INDEX IF NOT EXISTS idx_coupons_tenant ON coupons(tenant_id);

-- Shipping Templates
CREATE TABLE IF NOT EXISTS shipping_templates (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'weight',
    first_unit BIGINT NOT NULL DEFAULT 1,
    first_price BIGINT NOT NULL DEFAULT 0,
    additional_unit BIGINT NOT NULL DEFAULT 1,
    additional_price BIGINT NOT NULL DEFAULT 0,
    free_shipping_amount BIGINT NOT NULL DEFAULT 0,
    regions JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_shipping_templates_tenant ON shipping_templates(tenant_id);
CREATE INDEX IF NOT EXISTS idx_shipping_templates_status ON shipping_templates(status);

-- ============================================================
-- Integration Plane (P1): itg_channels / itg_channel_cursors / itg_receipts
-- ============================================================

CREATE TABLE IF NOT EXISTS itg_channels (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    -- App ownership (channel-app-ownership.md §2): NULL = platform/global channel.
    app_id TEXT,
    channel_key TEXT NOT NULL,
    provider TEXT NOT NULL,
    display_name TEXT NOT NULL,
    mode TEXT NOT NULL,
    transport TEXT NOT NULL,
    framing TEXT NOT NULL,
    codec TEXT NOT NULL,
    endpoint TEXT,
    verify_kind TEXT NOT NULL,
    verify_config JSONB,
    credentials TEXT,
    mapping JSONB,
    normalizer_plugin TEXT,
    pull_semantics TEXT,
    pull_config JSONB,
    stream_config JSONB,
    ack_kind TEXT NOT NULL DEFAULT 'http-200',
    redelivery_max BIGINT NOT NULL DEFAULT 5,
    backpressure JSONB,
    target_type TEXT NOT NULL,
    route_extra JSONB,
    status TEXT NOT NULL DEFAULT 'idle',
    last_error TEXT,
    lease_owner TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    version BIGINT NOT NULL DEFAULT 1,
    shadow BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, channel_key, version)
);

CREATE INDEX IF NOT EXISTS idx_itg_channels_tenant ON itg_channels(tenant_id);
CREATE INDEX IF NOT EXISTS idx_itg_channels_app ON itg_channels(app_id);
CREATE INDEX IF NOT EXISTS idx_itg_channels_status ON itg_channels(status);

-- Cursor store: one row per pull channel; advanced by conditional update
CREATE TABLE IF NOT EXISTS itg_channel_cursors (
    channel_id BIGINT PRIMARY KEY,
    cursor_value JSONB NOT NULL,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS itg_receipts (
    id BIGINT PRIMARY KEY,
    channel_id BIGINT NOT NULL,
    external_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    raw_ref TEXT,
    status TEXT NOT NULL DEFAULT 'received',
    attempts BIGINT NOT NULL DEFAULT 0,
    next_retry_at TIMESTAMPTZ(0),
    envelope JSONB,
    steps JSONB,
    target_id BIGINT,
    received_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ(0),
    UNIQUE (channel_id, external_id)
);

CREATE INDEX IF NOT EXISTS idx_itg_receipts_channel_status ON itg_receipts(channel_id, status);
CREATE INDEX IF NOT EXISTS idx_itg_receipts_retry ON itg_receipts(status, next_retry_at);

-- ============================================================
-- Integration Plane (M0): itg_api_clients / itg_egress_log
-- ============================================================

-- Declarative outbound API clients (integration.md §9.2)
CREATE TABLE IF NOT EXISTS itg_api_clients (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    client_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    auth JSONB,                       -- {kind: bearer|api-key-header|none, header, prefix}
    credentials TEXT,                 -- sealed (AES-256-GCM, base64); never echoed
    rate_limit JSONB,                 -- {per_minute} fixed window
    ops JSONB,                        -- {op: {method, path, output?}}
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, client_key)
);

CREATE INDEX IF NOT EXISTS idx_itg_api_clients_tenant ON itg_api_clients(tenant_id);

-- OAuth2 authorization-code tokens (oauth2-egress.md §2): one row per
-- (api-client, tenant); access/refresh tokens are vault-sealed.
CREATE TABLE IF NOT EXISTS itg_oauth_tokens (
    id BIGINT PRIMARY KEY,
    client_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    access_token TEXT,
    refresh_token TEXT,
    expires_at TIMESTAMPTZ(0),
    scope TEXT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (client_key, tenant_id)
);

CREATE INDEX IF NOT EXISTS idx_itg_oauth_tokens_tenant ON itg_oauth_tokens(tenant_id);

-- Full outbound call log (integration.md §10.7: trace_id = itg_receipts.id)
CREATE TABLE IF NOT EXISTS itg_egress_log (
    id BIGINT PRIMARY KEY,
    trace_id BIGINT,                  -- source receipt id (NULL for standalone calls)
    client_key TEXT NOT NULL,
    op TEXT NOT NULL,
    status TEXT NOT NULL,             -- success | error
    http_status BIGINT,
    latency_ms BIGINT NOT NULL DEFAULT 0,
    tokens_in BIGINT,
    tokens_out BIGINT,
    model TEXT,
    error TEXT,
    response_summary TEXT,            -- truncated response body (first 2000 chars)
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_itg_egress_log_trace ON itg_egress_log(trace_id);
CREATE INDEX IF NOT EXISTS idx_itg_egress_log_client_time ON itg_egress_log(client_key, created_at);

-- ============================================================
-- App Bundle (M2): apps / app_ct_refs / app_licenses
-- ============================================================

-- Installed app registry + lifecycle state machine (app-bundle.md §3.2)
CREATE TABLE IF NOT EXISTS apps (
    id BIGINT PRIMARY KEY,
    app_id TEXT NOT NULL,             -- manifest id (kebab-case), immutable
    version TEXT NOT NULL,            -- installed version (semver)
    status TEXT NOT NULL,             -- installing|installed|enabled|disabled|uninstalling|rolled_back
    source TEXT NOT NULL DEFAULT 'upload',  -- upload | market | directory
    source_ref TEXT,
    signature_ok BOOLEAN NOT NULL DEFAULT TRUE,
    install_log JSONB,                -- compensation log (§4.2), persisted per step
    last_error TEXT,
    tenant_scope TEXT NOT NULL DEFAULT 'global', -- global | per-tenant
    options JSONB,                    -- install-time wizard choices (keep_data, ...)
    installed_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (app_id)
);

CREATE INDEX IF NOT EXISTS idx_apps_status ON apps(status);

-- Materialized CT definitions owned by apps (app-bundle.md §6.3)
CREATE TABLE IF NOT EXISTS app_ct_refs (
    id BIGINT PRIMARY KEY,
    app_id TEXT NOT NULL,
    ct_table TEXT NOT NULL,
    schema_toml TEXT NOT NULL,        -- authoritative CT definition shipped by the app
    version TEXT NOT NULL,            -- app version at materialization time
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (app_id, ct_table)
);

-- Per-tenant licenses for global apps (app-bundle.md §9)
CREATE TABLE IF NOT EXISTS app_licenses (
    id BIGINT PRIMARY KEY,
    app_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    granted_by BIGINT,
    granted_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (app_id, tenant_id)
);

-- ============================================================
-- Seed data
-- ============================================================

-- Default tenant
INSERT INTO tenants (id, name, domain, config, status, created_at, updated_at) VALUES
    (10001, 'Default', NULL, '{}', 'active', NOW(), NOW())
ON CONFLICT (name) DO NOTHING;

-- Default currencies
INSERT INTO currencies (id, tenant_id, code, name, decimals) VALUES
    (10001, 'default', 'CNY', 'Chinese Yuan', 2),
    (10002, 'default', 'USD', 'US Dollar', 2),
    (10003, 'default', 'EUR', 'Euro', 2),
    (10004, 'default', 'GBP', 'British Pound', 2),
    (10005, 'default', 'JPY', 'Japanese Yen', 0)
ON CONFLICT DO NOTHING;

-- System roles
INSERT INTO roles (id, tenant_id, name, description, is_system, created_at, updated_at) VALUES
    (10001, 'default', 'admin', 'Super administrator', TRUE, NOW(), NOW()),
    (10002, 'default', 'editor', 'Editor', FALSE, NOW(), NOW()),
    (10003, 'default', 'author', 'Author', FALSE, NOW(), NOW()),
    (10004, 'default', 'reader', 'Reader', TRUE, NOW(), NOW())
ON CONFLICT (tenant_id, name) DO NOTHING;

-- Admin global permissions
INSERT INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10001, 'default', (SELECT id FROM roles WHERE name = 'admin'), '*', '*', NULL, NULL, NOW())
ON CONFLICT DO NOTHING;

-- Editor permissions
INSERT INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10002, 'default', (SELECT id FROM roles WHERE name = 'editor'), '*.*', '*', NULL, NULL, NOW())
ON CONFLICT DO NOTHING;

-- Author permissions
INSERT INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
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
    (10027, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'product_categories', NULL, NULL, NOW())
ON CONFLICT DO NOTHING;

-- Reader permissions
INSERT INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
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
    (10060, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'product_favorites', NULL, NULL, NOW())
ON CONFLICT DO NOTHING;

-- Site options
INSERT INTO options (id, tenant_id, option_key, value, type, group_name, label, description, validation, is_public, autoload, sort_order, updated_at) VALUES
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
    (10017, 'default', 'reserved_usernames', '"admin,administrator,root,system,official,support,staff,moderator,mod,help,info,mail,webmaster,security,billing,sales,owner,superuser,operator"', 'text', 'general', 'Reserved usernames', 'Comma-separated usernames that cannot be registered', '{"max_length":10000}', FALSE, TRUE, 5, NOW())
ON CONFLICT (tenant_id, option_key) DO NOTHING;

-- ============================================================
-- Flow orchestration engine v2 (dev-docs/workflow) — P0-P1 5 tables
-- Data is engine-own (independent namespace); egress only referenced.
-- ============================================================

CREATE TABLE IF NOT EXISTS flow (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    description TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    current_version BIGINT,
    extra JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_flow_tenant ON flow(tenant_id);

-- Immutable definition snapshots (publish = append; instance locks a version)
CREATE TABLE IF NOT EXISTS flow_version (
    id BIGINT PRIMARY KEY,
    flow_id BIGINT NOT NULL,
    version_number BIGINT NOT NULL,
    definition JSONB NOT NULL,
    created_by BIGINT,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (flow_id, version_number)
);
CREATE INDEX IF NOT EXISTS idx_flow_version_flow ON flow_version(flow_id);

-- One run of a flow (wf_trace root = instance id)
CREATE TABLE IF NOT EXISTS flow_instance (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    flow_id BIGINT NOT NULL,
    flow_version_id BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    has_exceptions BOOLEAN NOT NULL DEFAULT FALSE,
    trigger_kind TEXT NOT NULL,
    trigger_payload JSONB,
    inputs_summary JSONB,
    outputs JSONB,
    error JSONB,
    started_by BIGINT,
    started_at TIMESTAMPTZ(0),
    finished_at TIMESTAMPTZ(0),
    waiting_kind TEXT,
    waiting_needed BIGINT,
    waiting_received BIGINT NOT NULL DEFAULT 0,
    resume_until TIMESTAMPTZ(0),
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_flow_instance_flow_status ON flow_instance(flow_id, status);
CREATE INDEX IF NOT EXISTS idx_flow_instance_status_time ON flow_instance(status, created_at);
CREATE INDEX IF NOT EXISTS idx_flow_instance_waiting ON flow_instance(status, resume_until);

-- Durable runnable state (whole snapshot, 1:1, rewritten each step)
CREATE TABLE IF NOT EXISTS flow_instance_snapshot (
    instance_id BIGINT PRIMARY KEY,
    snapshot JSONB NOT NULL,
    snapshot_version BIGINT NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);

-- Per-node run history (engine-own observability)
CREATE TABLE IF NOT EXISTS flow_node_run (
    id BIGINT PRIMARY KEY,
    instance_id BIGINT NOT NULL,
    node_id TEXT NOT NULL,
    node_type TEXT NOT NULL,
    seq BIGINT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 1,
    status TEXT NOT NULL,
    started_at TIMESTAMPTZ(0),
    finished_at TIMESTAMPTZ(0),
    latency_ms BIGINT,
    input_summary JSONB,
    output_summary JSONB,
    usage_json JSONB,
    error JSONB,
    egress_log_id BIGINT,
    container_ref JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_flow_node_run_instance_seq ON flow_node_run(instance_id, seq);
CREATE INDEX IF NOT EXISTS idx_flow_node_run_egress ON flow_node_run(egress_log_id);

-- Public API keys for flows (external invocation auth)
CREATE TABLE IF NOT EXISTS flow_api_key (
    id BIGINT PRIMARY KEY,
    flow_id BIGINT NOT NULL,
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    token_enc TEXT NOT NULL,
    slug VARCHAR(40) UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    require_auth BOOLEAN NOT NULL DEFAULT TRUE,
    last_used_at TIMESTAMPTZ(0),
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_flow_api_key_flow ON flow_api_key(flow_id);

-- Internal flow triggers (event/cron): point at a flow, decoupled from flow.
CREATE TABLE IF NOT EXISTS flow_trigger (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    flow_id BIGINT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    event_type TEXT,
    filter JSONB,
    cron_expr TEXT,
    inputs_map JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_triggered_at TIMESTAMPTZ(0),
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_flow_trigger_kind_event ON flow_trigger(kind, event_type);
CREATE INDEX IF NOT EXISTS idx_flow_trigger_flow ON flow_trigger(flow_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- AI Agent core (namespace ai_*). See dev-docs/agent/db-schema.md.
-- Multi-tenant: every table is filtered by tenant_id; ids are Snowflake (BIGINT).
-- ─────────────────────────────────────────────────────────────────────────────

-- An agent definition (provider/model/system_prompt/tool allowlist/memory).
CREATE TABLE IF NOT EXISTS ai_agents (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT,
    name TEXT NOT NULL,
    system_prompt TEXT NOT NULL DEFAULT '',
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    temperature DOUBLE PRECISION,
    max_iterations INTEGER NOT NULL DEFAULT 10,
    tools JSONB NOT NULL,
    memory_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    params JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, name)
);
CREATE INDEX IF NOT EXISTS idx_ai_agents_tenant ON ai_agents(tenant_id);

-- A conversation session bound to an agent + owner.
CREATE TABLE IF NOT EXISTS ai_sessions (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    agent_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open',
    meta JSONB,
    last_seq BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    last_active_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_ai_sessions_tenant_agent ON ai_sessions(tenant_id, agent_id);
CREATE INDEX IF NOT EXISTS idx_ai_sessions_owner_active ON ai_sessions(user_id, last_active_at);

-- Append-only conversation event log (externalized session). role: system/user/
-- assistant/tool/meta. kind: chat/assistant_tool_calls/tool_result/turn:meta/
-- context:summary/context:reset. usage on every assistant row.
CREATE TABLE IF NOT EXISTS ai_messages (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    session_id BIGINT NOT NULL,
    seq BIGINT NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'chat',
    content TEXT NOT NULL DEFAULT '',
    tool_calls JSONB,
    tool_call_id TEXT,
    tool_name TEXT,
    tool_success BOOLEAN,
    tool_error TEXT,
    tool_elapsed_ms BIGINT,
    tool_truncated BOOLEAN,
    reasoning_content TEXT,
    call_usage JSONB,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_ai_messages_session_seq ON ai_messages(session_id, seq);
CREATE INDEX IF NOT EXISTS idx_ai_messages_session_role ON ai_messages(session_id, role, seq);

-- Agent long-term memory (ScopedMemory target; tenant+agent is the ownership scope).
CREATE TABLE IF NOT EXISTS ai_memories (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    agent_id BIGINT NOT NULL,
    user_id BIGINT,
    session_id BIGINT,
    mem_key TEXT NOT NULL,
    content TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'core',
    importance DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    superseded_by BIGINT,
    pinned BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, agent_id, user_id, mem_key)
);
CREATE INDEX IF NOT EXISTS idx_ai_memories_agent_live ON ai_memories(agent_id, superseded_by);
CREATE INDEX IF NOT EXISTS idx_ai_memories_agent_category ON ai_memories(agent_id, category);
