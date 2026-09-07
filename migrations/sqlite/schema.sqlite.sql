-- ============================================================
-- raisfast complete database schema (with multi-tenant support)
-- Merged from all migration files for one-click initialization of new deployments
-- Generated date：2026-05-07
-- ============================================================

-- ── Platform foundation layer (always enabled) ──────────────────────────────────

-- Tenants
CREATE TABLE IF NOT EXISTS tenants (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    domain TEXT UNIQUE,
    config TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- Users
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    username TEXT UNIQUE NOT NULL,
    avatar TEXT,
    bio TEXT,
    website TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    registered_via TEXT NOT NULL,
    display_name TEXT,
    slug TEXT UNIQUE,
    locale TEXT,
    social_links TEXT,
    metadata TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_users_tenant ON users(tenant_id);

-- User credentials
CREATE TABLE IF NOT EXISTS user_credentials (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    auth_type TEXT NOT NULL,
    identifier TEXT NOT NULL,
    credential_data TEXT NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(auth_type, identifier)
);

CREATE INDEX IF NOT EXISTS idx_user_credentials_user ON user_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_user_credentials_type ON user_credentials(auth_type);

-- OAuth account bindings
CREATE TABLE IF NOT EXISTS oauth_accounts (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    provider TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    email TEXT,
    display_name TEXT,
    avatar_url TEXT,
    access_token TEXT,
    refresh_token TEXT,
    token_expires_at TEXT,
    profile TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(provider, provider_user_id)
);

CREATE INDEX IF NOT EXISTS idx_oauth_accounts_user ON oauth_accounts(user_id);

-- OAuth short-lived state storage (PKCE)
CREATE TABLE IF NOT EXISTS oauth_states (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL,
    code_verifier TEXT NOT NULL,
    user_id INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oauth_states_expires ON oauth_states(expires_at);

-- Currency configuration
CREATE TABLE IF NOT EXISTS currencies (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    code TEXT NOT NULL CHECK(code = UPPER(code) AND LENGTH(code) BETWEEN 1 AND 10),
    name TEXT NOT NULL,
    decimals INTEGER NOT NULL DEFAULT 0 CHECK(decimals BETWEEN 0 AND 18),
    is_active INTEGER NOT NULL DEFAULT 1,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, code)
);

-- User wallets (one per user per currency)
CREATE TABLE IF NOT EXISTS wallets (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    currency TEXT NOT NULL,
    balance INTEGER NOT NULL DEFAULT 0 CHECK(balance >= 0),
    version INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(user_id, currency)
);

CREATE INDEX IF NOT EXISTS idx_wallets_currency ON wallets(currency);
CREATE INDEX IF NOT EXISTS idx_wallets_tenant ON wallets(tenant_id);

-- Immutable transaction log
CREATE TABLE IF NOT EXISTS wallet_transactions (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    wallet_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    entry_type TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK(amount > 0),
    balance_after INTEGER NOT NULL CHECK(balance_after >= 0),
    tx_type TEXT NOT NULL,
    currency TEXT NOT NULL,
    transaction_no TEXT NOT NULL UNIQUE,
    related_tx_id INTEGER,
    reference_type TEXT,
    reference_id TEXT,
    counterparty_wallet_id INTEGER,
    metadata TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_wallet_tx_wallet ON wallet_transactions(wallet_id);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_user ON wallet_transactions(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_reference ON wallet_transactions(reference_type, reference_id);
CREATE INDEX IF NOT EXISTS idx_wallet_tx_tenant_user ON wallet_transactions(tenant_id, user_id, created_at DESC);

-- Refresh Tokens
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    token TEXT UNIQUE NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);

-- Site options
CREATE TABLE IF NOT EXISTS options (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    option_key TEXT NOT NULL,
    value TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'text',
    group_name TEXT NOT NULL DEFAULT 'general',
    label TEXT NOT NULL DEFAULT '',
    description TEXT,
    validation TEXT,
    is_public BOOLEAN NOT NULL DEFAULT 0,
    autoload BOOLEAN NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, option_key)
);

-- RBAC roles
CREATE TABLE IF NOT EXISTS roles (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    description TEXT,
    is_system BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, name)
);

CREATE INDEX IF NOT EXISTS idx_roles_tenant ON roles(tenant_id);

-- RBAC permissions
CREATE TABLE IF NOT EXISTS permissions (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    role_id INTEGER NOT NULL,
    action TEXT NOT NULL,
    subject TEXT NOT NULL,
     fields TEXT,
    conditions TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);


CREATE UNIQUE INDEX IF NOT EXISTS idx_permissions_role_action_subject
    ON permissions(role_id, action, subject);
CREATE INDEX IF NOT EXISTS idx_permissions_tenant ON permissions(tenant_id);

-- User-role assignments (many-to-many)
CREATE TABLE IF NOT EXISTS user_roles (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    role_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, user_id, role_id)
);

CREATE INDEX IF NOT EXISTS idx_user_roles_user ON user_roles(user_id);
CREATE INDEX IF NOT EXISTS idx_user_roles_role ON user_roles(role_id);
CREATE INDEX IF NOT EXISTS idx_user_roles_tenant ON user_roles(tenant_id);

-- Audit log
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    actor_id INTEGER,
    actor_role TEXT,
    action TEXT NOT NULL,
    subject TEXT NOT NULL,
    subject_id TEXT,
    detail TEXT,
     ip_address TEXT,
    user_agent TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);


CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_tenant_created ON audit_log(tenant_id, created_at DESC);

-- API Token
CREATE TABLE IF NOT EXISTS api_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    token_hash TEXT UNIQUE NOT NULL,
    token_encrypted TEXT NOT NULL,
    description TEXT DEFAULT '',
    scopes TEXT NOT NULL,
    last_used_at TEXT,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_api_tokens_user_id ON api_tokens(user_id);

-- Webhook subscriptions
CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL,
    secret TEXT NOT NULL,
    events TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_webhook_subscriptions_enabled ON webhook_subscriptions(enabled);
CREATE INDEX IF NOT EXISTS idx_webhook_subscriptions_tenant ON webhook_subscriptions(tenant_id);

-- Webhook delivery log
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id INTEGER PRIMARY KEY,
    webhook_id INTEGER NOT NULL,
    event TEXT NOT NULL,
    status TEXT NOT NULL,
    status_code INTEGER,
    error TEXT,
    duration_ms INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_webhook ON webhook_deliveries(webhook_id);
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_created ON webhook_deliveries(created_at DESC);

-- Plugin KV storage
CREATE TABLE IF NOT EXISTS plugin_storage (
    plugin_id TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    value TEXT NOT NULL,
    expires_at TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    PRIMARY KEY (plugin_id, storage_key)
);

CREATE INDEX IF NOT EXISTS idx_plugin_storage_plugin ON plugin_storage(plugin_id);

-- Content revision history
CREATE TABLE IF NOT EXISTS content_revisions (
    id INTEGER PRIMARY KEY,
    content_type TEXT NOT NULL,
    record_id INTEGER NOT NULL,
    revision_number INTEGER NOT NULL,
    snapshot TEXT NOT NULL,
    created_by INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(content_type, record_id, revision_number)
);

CREATE INDEX IF NOT EXISTS idx_revisions_ct_record_rev
    ON content_revisions(content_type, record_id, revision_number DESC);

-- Password reset tokens
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    token TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user_id ON password_reset_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_expires_at ON password_reset_tokens(expires_at);

-- SMS verification codes
CREATE TABLE IF NOT EXISTS sms_codes (
    id INTEGER PRIMARY KEY,
    phone TEXT NOT NULL,
    code TEXT NOT NULL,
    purpose TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    verified_at TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    ip_address TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_sms_codes_phone ON sms_codes(phone);
CREATE INDEX IF NOT EXISTS idx_sms_codes_expires ON sms_codes(expires_at);

-- User device codes (IDE authentication)
CREATE TABLE IF NOT EXISTS user_device_codes (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    code TEXT NOT NULL UNIQUE,
    access_token TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_user_device_codes_code ON user_device_codes(code);
CREATE INDEX IF NOT EXISTS idx_user_device_codes_user_id ON user_device_codes(user_id);
CREATE INDEX IF NOT EXISTS idx_user_device_codes_expires_at ON user_device_codes(expires_at);

-- Email verification tokens
CREATE TABLE IF NOT EXISTS email_verification_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    token TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    verified_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_email_verification_tokens_user_id ON email_verification_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_email_verification_tokens_expires ON email_verification_tokens(expires_at);

-- Background job queue
CREATE TABLE IF NOT EXISTS jobs (
    id               INTEGER PRIMARY KEY,
    job_type         TEXT NOT NULL,
    payload          TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    attempts         INTEGER NOT NULL DEFAULT 0,
    max_attempts     INTEGER NOT NULL DEFAULT 3,
    run_after        TEXT,
    error            TEXT,
    cron_schedule_id INTEGER,
    cron_log_id      INTEGER,
    priority         INTEGER NOT NULL DEFAULT 0,
    timeout_secs     INTEGER,
    dedup_key        TEXT,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_jobs_status_run_after ON jobs(status, run_after) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_jobs_type ON jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_jobs_dedup_key ON jobs(dedup_key) WHERE dedup_key IS NOT NULL;

-- Cron job schedules
CREATE TABLE IF NOT EXISTS cron_schedules (
    id           INTEGER PRIMARY KEY,
    label        TEXT NOT NULL,
    job_type     TEXT NOT NULL,
    payload      TEXT,
    cron_expr    TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    last_run_at  TEXT,
    next_run_at  TEXT NOT NULL,
    plugin_id    TEXT,
    exec_kind    TEXT NOT NULL DEFAULT 'builtin',
    handler_id   TEXT,
    params       TEXT,
    script_lang    TEXT,
    script_source  TEXT,
    script_entry   TEXT NOT NULL DEFAULT 'on_cron_tick',
    use_shell      INTEGER NOT NULL DEFAULT 1,
    timeout_secs   INTEGER,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_cron_enabled ON cron_schedules(enabled);
CREATE INDEX IF NOT EXISTS idx_cron_next_run ON cron_schedules(next_run_at) WHERE enabled = 1;
CREATE INDEX IF NOT EXISTS idx_cron_plugin ON cron_schedules(plugin_id);

-- Cron execution log
CREATE TABLE IF NOT EXISTS cron_execution_log (
    id           INTEGER PRIMARY KEY,
    schedule_id  INTEGER NOT NULL,
    job_type     TEXT NOT NULL,
    label        TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'running',
    duration_ms  INTEGER,
    error        TEXT,
    started_at   TEXT NOT NULL,
    finished_at  TEXT
);

CREATE INDEX IF NOT EXISTS idx_cron_log_schedule ON cron_execution_log(schedule_id);
CREATE INDEX IF NOT EXISTS idx_cron_log_status ON cron_execution_log(status);
CREATE INDEX IF NOT EXISTS idx_cron_log_started ON cron_execution_log(started_at);

-- ── Built-in module: Blog (BUILTIN_BLOG=true) ──────────────────

-- Categories
CREATE TABLE IF NOT EXISTS categories (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    parent_id INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_by INTEGER,
    updated_by INTEGER,
    cover_image TEXT,
    meta_title TEXT,
    meta_description TEXT,
    og_title TEXT,
    og_description TEXT,
    og_image TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, name),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_categories_tenant ON categories(tenant_id);

-- Product categories
CREATE TABLE IF NOT EXISTS product_categories (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    description TEXT,
    cover_image TEXT,
    parent_id INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    meta_title TEXT,
    meta_description TEXT,
    og_title TEXT,
    og_description TEXT,
    og_image TEXT,
    created_by INTEGER,
    updated_by INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, name),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_product_categories_tenant ON product_categories(tenant_id);

-- Tags
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    created_by INTEGER,
    updated_by INTEGER,
    description TEXT,
    cover_image TEXT,
    meta_title TEXT,
    meta_description TEXT,
    og_title TEXT,
    og_description TEXT,
    og_image TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, name),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_tags_tenant ON tags(tenant_id);

-- Posts
CREATE TABLE IF NOT EXISTS posts (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    title TEXT NOT NULL,
    slug TEXT NOT NULL,
    content TEXT NOT NULL,
    excerpt TEXT,
    cover_image TEXT,
    image_ids TEXT,
    status TEXT NOT NULL DEFAULT 'draft',
    created_by INTEGER NOT NULL,
    updated_by INTEGER,
    category_id INTEGER,
    view_count INTEGER NOT NULL DEFAULT 0,
    is_pinned INTEGER NOT NULL DEFAULT 0,
    password TEXT,
    comment_status TEXT NOT NULL DEFAULT 'open',
    format TEXT NOT NULL DEFAULT 'standard',
    template TEXT NOT NULL DEFAULT 'default',
    meta_title TEXT,
    meta_description TEXT,
    og_title TEXT,
    og_description TEXT,
    og_image TEXT,
    canonical_url TEXT,
    reading_time INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    published_at TEXT,
    UNIQUE(tenant_id, slug)
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

-- Posts-Tags (many-to-many)
CREATE TABLE IF NOT EXISTS posts_tags (
    post_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (post_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_posts_tags_tag_id ON posts_tags(tag_id);

CREATE TABLE IF NOT EXISTS taggings (
    id INTEGER PRIMARY KEY,
    tag_id INTEGER NOT NULL,
    taggable_type TEXT NOT NULL,
    taggable_id INTEGER NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    UNIQUE(tenant_id, tag_id, taggable_type, taggable_id)
);

CREATE INDEX IF NOT EXISTS idx_taggings_tag ON taggings(tag_id);
CREATE INDEX IF NOT EXISTS idx_taggings_taggable ON taggings(taggable_type, taggable_id);

-- Comments
CREATE TABLE IF NOT EXISTS comments (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    post_id INTEGER NOT NULL,
    created_by INTEGER,
    updated_by INTEGER,
    nickname TEXT,
    email TEXT,
    content TEXT NOT NULL,
    parent_id INTEGER,
    status TEXT NOT NULL DEFAULT 'pending',
    author_ip TEXT,
    author_url TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
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
    id               INTEGER PRIMARY KEY,
    tenant_id        TEXT NOT NULL DEFAULT 'default',
    title            TEXT NOT NULL,
    slug             TEXT NOT NULL UNIQUE,
    content          TEXT,
    blocks           TEXT,
    meta_title       TEXT,
    meta_description TEXT,
    og_image         TEXT,
    template         TEXT NOT NULL DEFAULT 'default',
    parent_id        INTEGER,
    sort_order       INTEGER NOT NULL DEFAULT 0,
    status           TEXT NOT NULL DEFAULT 'draft',
    created_by       INTEGER NOT NULL,
    updated_by       INTEGER,
    cover_image      TEXT,
    published_at     TEXT,
    password TEXT,
    comment_status TEXT NOT NULL DEFAULT 'closed',
    og_title TEXT,
    og_description TEXT,
    canonical_url TEXT,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(tenant_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_pages_status    ON pages(status);
CREATE INDEX IF NOT EXISTS idx_pages_parent    ON pages(parent_id);
CREATE INDEX IF NOT EXISTS idx_pages_author    ON pages(created_by);
CREATE INDEX IF NOT EXISTS idx_pages_tenant_slug ON pages(tenant_id, slug);
CREATE INDEX IF NOT EXISTS idx_pages_tenant_status ON pages(tenant_id, status);

CREATE TABLE IF NOT EXISTS reusable_blocks (
    id          INTEGER PRIMARY KEY,
    tenant_id   TEXT NOT NULL DEFAULT 'default',
    name        TEXT NOT NULL,
    block_type  TEXT NOT NULL,
    content     TEXT NOT NULL,
    description TEXT,
    created_by  INTEGER,
    updated_by  INTEGER,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_reusable_blocks_tenant ON reusable_blocks(tenant_id);

-- ── Built-in module: Media (BUILTIN_MEDIA=true) ────────────────

CREATE TABLE IF NOT EXISTS media (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    filepath TEXT NOT NULL,
    mimetype TEXT NOT NULL,
    size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    title TEXT,
    alt_text TEXT,
    caption TEXT,
    description TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_media_user_created
    ON media(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_media_tenant ON media(tenant_id);

-- ── Built-in module: Workflow (BUILTIN_WORKFLOW=true) ──────────

CREATE TABLE IF NOT EXISTS workflow_definitions (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    steps TEXT NOT NULL,
    initial_step TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS workflow_instances (
    id INTEGER PRIMARY KEY,
    definition_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    current_step TEXT,
    context TEXT NOT NULL DEFAULT '{}',
    triggered_by INTEGER,
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    completed_at TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_wf_instances_definition ON workflow_instances(definition_id);
CREATE INDEX IF NOT EXISTS idx_wf_instances_status ON workflow_instances(status);

CREATE TABLE IF NOT EXISTS workflow_step_logs (
    id INTEGER PRIMARY KEY,
    instance_id INTEGER NOT NULL,
    step_id TEXT NOT NULL,
    step_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    input TEXT,
    output TEXT,
    error TEXT,
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_wf_step_logs_instance ON workflow_step_logs(instance_id);

-- Products
CREATE TABLE IF NOT EXISTS products (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    category_id INTEGER,
    title TEXT NOT NULL,
    description TEXT,
    cover_url TEXT,
    product_type TEXT NOT NULL DEFAULT 'custom',
    fulfillment_type TEXT NOT NULL DEFAULT 'digital',
    delivery_hook TEXT,
    weight INTEGER,
    shipping_template_id INTEGER,
    price INTEGER NOT NULL CHECK(price >= 0),
    currency TEXT NOT NULL DEFAULT 'USD',
    status TEXT NOT NULL DEFAULT 'draft',
    attributes TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    slug TEXT,
    content TEXT,
    image_ids TEXT,
    original_price INTEGER,
    specs TEXT,
    unit TEXT NOT NULL DEFAULT 'piece',
    min_purchase INTEGER NOT NULL DEFAULT 1,
    max_purchase INTEGER,
    total_sales INTEGER NOT NULL DEFAULT 0,
    virtual_sales INTEGER NOT NULL DEFAULT 0,
    meta_title TEXT,
    meta_description TEXT,
    og_title TEXT,
    og_description TEXT,
    og_image TEXT,
    published_at TEXT,
    stock INTEGER NOT NULL DEFAULT 0,
    cost_price INTEGER,
    sale_price INTEGER,
    has_variants INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_products_status ON products(status);
CREATE INDEX IF NOT EXISTS idx_products_type ON products(product_type);
CREATE INDEX IF NOT EXISTS idx_products_tenant ON products(tenant_id);
CREATE INDEX IF NOT EXISTS idx_products_tenant_status ON products(tenant_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_products_tenant_slug ON products(tenant_id, slug);

-- Product Variants
CREATE TABLE IF NOT EXISTS product_variants (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    product_id INTEGER NOT NULL,
    sku TEXT UNIQUE,
    title TEXT NOT NULL,
    price INTEGER NOT NULL CHECK(price >= 0),
    original_price INTEGER,
    stock INTEGER NOT NULL DEFAULT 0,
    attributes TEXT,
    image_url TEXT,
    weight INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_product_variants_product ON product_variants(product_id);
CREATE INDEX IF NOT EXISTS idx_product_variants_tenant ON product_variants(tenant_id);

-- User Addresses
CREATE TABLE IF NOT EXISTS user_addresses (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    recipient_name TEXT NOT NULL,
    phone TEXT NOT NULL,
    country TEXT NOT NULL DEFAULT 'CN',
    province TEXT NOT NULL DEFAULT '',
    city TEXT NOT NULL DEFAULT '',
    district TEXT NOT NULL DEFAULT '',
    address_line1 TEXT NOT NULL,
    address_line2 TEXT,
    postal_code TEXT,
    is_default INTEGER NOT NULL DEFAULT 0,
    address_type TEXT NOT NULL DEFAULT 'shipping',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_user_addresses_user ON user_addresses(user_id);
CREATE INDEX IF NOT EXISTS idx_user_addresses_tenant ON user_addresses(tenant_id);

-- Orders
CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    order_no TEXT NOT NULL UNIQUE,
    subtotal INTEGER NOT NULL DEFAULT 0,
    discount_amount INTEGER NOT NULL DEFAULT 0,
    shipping_amount INTEGER NOT NULL DEFAULT 0,
    total_amount INTEGER NOT NULL CHECK(total_amount >= 0),
    currency TEXT NOT NULL DEFAULT 'USD',
    status TEXT NOT NULL DEFAULT 'pending',
    buyer_name TEXT,
    buyer_phone TEXT,
    buyer_email TEXT,
    shipping_address TEXT,
    tracking_no TEXT,
    carrier TEXT,
    remark TEXT,
    admin_remark TEXT,
    delivery_data TEXT,
    tax_amount INTEGER NOT NULL DEFAULT 0,
    coupon_id INTEGER,
    shipping_address_id INTEGER,
    billing_address_id INTEGER,
    paid_at TEXT,
    completed_at TEXT,
    cancelled_at TEXT,
    refunding_at TEXT,
    refunded_at TEXT,
    expired_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_orders_user ON orders(user_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
CREATE INDEX IF NOT EXISTS idx_orders_tenant ON orders(tenant_id);
CREATE INDEX IF NOT EXISTS idx_orders_tenant_user_status ON orders(tenant_id, user_id, status);
CREATE INDEX IF NOT EXISTS idx_orders_tenant_status_created ON orders(tenant_id, status, created_at DESC);

-- Order Items
CREATE TABLE IF NOT EXISTS order_items (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    order_id INTEGER NOT NULL,
    product_id INTEGER,
    variant_id INTEGER,
    title TEXT NOT NULL,
    description TEXT,
    sku TEXT,
    unit_price INTEGER NOT NULL CHECK(unit_price >= 0),
    quantity INTEGER NOT NULL CHECK(quantity > 0),
    subtotal INTEGER NOT NULL,
    tax_amount INTEGER NOT NULL DEFAULT 0,
    cover_url TEXT,
    attributes TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_order_items_order ON order_items(order_id);
CREATE INDEX IF NOT EXISTS idx_order_items_product ON order_items(product_id);
CREATE INDEX IF NOT EXISTS idx_order_items_tenant ON order_items(tenant_id);

CREATE TABLE IF NOT EXISTS cart_items (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    product_id INTEGER NOT NULL,
    variant_id INTEGER,
    quantity INTEGER NOT NULL DEFAULT 1,
    attributes TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cart_items_user_product_variant ON cart_items(user_id, product_id, variant_id);
CREATE INDEX IF NOT EXISTS idx_cart_items_tenant ON cart_items(tenant_id);

CREATE TABLE IF NOT EXISTS payment_channels (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    provider TEXT NOT NULL,
    name TEXT NOT NULL,
    is_live INTEGER NOT NULL DEFAULT 0,
    credentials TEXT NOT NULL,
    webhook_secret TEXT,
    settings TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(provider, name)
);

CREATE INDEX IF NOT EXISTS idx_payment_channels_provider ON payment_channels(provider);
CREATE INDEX IF NOT EXISTS idx_payment_channels_active ON payment_channels(is_active);
CREATE INDEX IF NOT EXISTS idx_payment_channels_tenant ON payment_channels(tenant_id);

-- Payment Orders
CREATE TABLE IF NOT EXISTS payment_orders (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    order_id TEXT,
    title TEXT NOT NULL,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    channel_id INTEGER NOT NULL,
    provider TEXT NOT NULL,
    provider_order_id TEXT,
    provider_method TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    reference_type TEXT,
    reference_id TEXT,
    return_url TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    version INTEGER NOT NULL DEFAULT 1,
    provider_data TEXT,
    client_ip TEXT,
    client_language TEXT,
    client_country TEXT,
    client_user_agent TEXT,
    channel_selected_by TEXT,
    metadata TEXT,
    paid_at TEXT,
    cancelled_at TEXT,
    expired_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_payment_orders_user ON payment_orders(user_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_status ON payment_orders(status);
CREATE INDEX IF NOT EXISTS idx_payment_orders_provider ON payment_orders(provider_order_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_order_id ON payment_orders(order_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_tenant ON payment_orders(tenant_id);
CREATE INDEX IF NOT EXISTS idx_payment_orders_tenant_status_created ON payment_orders(tenant_id, status, created_at DESC);

-- Payment Transactions
CREATE TABLE IF NOT EXISTS payment_transactions (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    payment_order_id INTEGER NOT NULL,
    order_id TEXT,
    user_id INTEGER NOT NULL,
    tx_type TEXT NOT NULL,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL,
    provider_tx_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending',
    raw_payload TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_payment_tx_order ON payment_transactions(payment_order_id);
CREATE INDEX IF NOT EXISTS idx_payment_tx_order_id ON payment_transactions(order_id);
CREATE INDEX IF NOT EXISTS idx_payment_transactions_tenant ON payment_transactions(tenant_id);

-- Payment Refunds
CREATE TABLE IF NOT EXISTS payment_refunds (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    payment_order_id INTEGER NOT NULL,
    order_id TEXT,
    user_id INTEGER NOT NULL,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL,
    reason TEXT,
    provider_refund_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    payment_tx_id INTEGER,
    metadata TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_payment_refunds_order ON payment_refunds(payment_order_id);
CREATE INDEX IF NOT EXISTS idx_payment_refunds_order_id ON payment_refunds(order_id);
CREATE INDEX IF NOT EXISTS idx_payment_refunds_tenant ON payment_refunds(tenant_id);

-- Wallet Outbox (ensures wallet operations are never lost)
CREATE TABLE IF NOT EXISTS wallet_outbox (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    currency TEXT NOT NULL,
    amount INTEGER NOT NULL,
    entry_type TEXT NOT NULL,
    tx_type TEXT NOT NULL,
    transaction_no TEXT NOT NULL,
    reference_type TEXT,
    reference_id TEXT,
    metadata TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_wallet_outbox_status ON wallet_outbox(status);
CREATE INDEX IF NOT EXISTS idx_wallet_outbox_transaction_no ON wallet_outbox(transaction_no);
CREATE INDEX IF NOT EXISTS idx_wallet_outbox_tenant ON wallet_outbox(tenant_id);

-- Product Comments (reviews/ratings)
CREATE TABLE IF NOT EXISTS product_comments (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    product_id INTEGER NOT NULL,
    order_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    rating INTEGER NOT NULL DEFAULT 5,
    title TEXT,
    content TEXT NOT NULL,
    images TEXT,
    status TEXT NOT NULL DEFAULT 'approved',
    admin_reply TEXT,
    admin_replied_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_product_comments_unique ON product_comments(product_id, order_id, user_id);
CREATE INDEX IF NOT EXISTS idx_product_comments_user ON product_comments(user_id);
CREATE INDEX IF NOT EXISTS idx_product_comments_status ON product_comments(status);
CREATE INDEX IF NOT EXISTS idx_product_comments_tenant ON product_comments(tenant_id);
-- Product Favorites (wishlist)
CREATE TABLE IF NOT EXISTS product_favorites (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER NOT NULL,
    product_id INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_product_favorites_unique ON product_favorites(user_id, product_id);
CREATE INDEX IF NOT EXISTS idx_product_favorites_user ON product_favorites(user_id);
CREATE INDEX IF NOT EXISTS idx_product_favorites_product ON product_favorites(product_id);
CREATE INDEX IF NOT EXISTS idx_product_favorites_tenant ON product_favorites(tenant_id);


-- Coupons
CREATE TABLE IF NOT EXISTS coupons (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    code TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    coupon_type TEXT NOT NULL DEFAULT 'percent',
    value INTEGER NOT NULL,
    min_order INTEGER NOT NULL DEFAULT 0,
    max_uses INTEGER NOT NULL DEFAULT 0,
    used_count INTEGER NOT NULL DEFAULT 0,
    max_uses_per_user INTEGER NOT NULL DEFAULT 1,
    starts_at TEXT,
    expires_at TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_coupons_status ON coupons(status);
CREATE INDEX IF NOT EXISTS idx_coupons_tenant ON coupons(tenant_id);

-- Shipping Templates
CREATE TABLE IF NOT EXISTS shipping_templates (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'weight',
    first_unit INTEGER NOT NULL DEFAULT 1,
    first_price INTEGER NOT NULL DEFAULT 0,
    additional_unit INTEGER NOT NULL DEFAULT 1,
    additional_price INTEGER NOT NULL DEFAULT 0,
    free_shipping_amount INTEGER NOT NULL DEFAULT 0,
    regions TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_shipping_templates_tenant ON shipping_templates(tenant_id);
CREATE INDEX IF NOT EXISTS idx_shipping_templates_status ON shipping_templates(status);

-- ============================================================
-- Integration Plane (P1): itg_channels / itg_channel_cursors / itg_receipts
-- ============================================================

CREATE TABLE IF NOT EXISTS itg_channels (
    id INTEGER PRIMARY KEY,
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
    verify_config TEXT,
    credentials TEXT,
    mapping TEXT,
    normalizer_plugin TEXT,
    pull_semantics TEXT,
    pull_config TEXT,
    stream_config TEXT,
    ack_kind TEXT NOT NULL DEFAULT 'http-200',
    redelivery_max INTEGER NOT NULL DEFAULT 5,
    backpressure TEXT,
    target_type TEXT NOT NULL,
    route_extra TEXT,
    status TEXT NOT NULL DEFAULT 'idle',
    last_error TEXT,
    lease_owner TEXT,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    version INTEGER NOT NULL DEFAULT 1,
    shadow BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (tenant_id, channel_key, version)
);

CREATE INDEX IF NOT EXISTS idx_itg_channels_tenant ON itg_channels(tenant_id);
CREATE INDEX IF NOT EXISTS idx_itg_channels_app ON itg_channels(app_id);
CREATE INDEX IF NOT EXISTS idx_itg_channels_status ON itg_channels(status);

-- Cursor store: one row per pull channel; advanced by conditional update
CREATE TABLE IF NOT EXISTS itg_channel_cursors (
    channel_id INTEGER PRIMARY KEY,
    cursor_value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS itg_receipts (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    external_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    raw_ref TEXT,
    status TEXT NOT NULL DEFAULT 'received',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT,
    envelope TEXT,
    steps TEXT,
    target_id INTEGER,
    received_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    delivered_at TEXT,
    UNIQUE (channel_id, external_id)
);

CREATE INDEX IF NOT EXISTS idx_itg_receipts_channel_status ON itg_receipts(channel_id, status);
CREATE INDEX IF NOT EXISTS idx_itg_receipts_retry ON itg_receipts(status, next_retry_at);

-- ============================================================
-- Integration Plane (M0): itg_api_clients / itg_egress_log
-- ============================================================

-- Declarative outbound API clients (integration.md §9.2)
CREATE TABLE IF NOT EXISTS itg_api_clients (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    client_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    auth TEXT,
    credentials TEXT,
    rate_limit TEXT,
    ops TEXT,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (tenant_id, client_key)
);

CREATE INDEX IF NOT EXISTS idx_itg_api_clients_tenant ON itg_api_clients(tenant_id);

-- OAuth2 authorization-code tokens (oauth2-egress.md §2): one row per
-- (api-client, tenant); access/refresh tokens are vault-sealed.
CREATE TABLE IF NOT EXISTS itg_oauth_tokens (
    id INTEGER PRIMARY KEY,
    client_key TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    access_token TEXT,
    refresh_token TEXT,
    expires_at TEXT,
    scope TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (client_key, tenant_id)
);

CREATE INDEX IF NOT EXISTS idx_itg_oauth_tokens_tenant ON itg_oauth_tokens(tenant_id);

-- Full outbound call log (integration.md §10.7: trace_id = itg_receipts.id)
CREATE TABLE IF NOT EXISTS itg_egress_log (
    id INTEGER PRIMARY KEY,
    trace_id INTEGER,
    client_key TEXT NOT NULL,
    op TEXT NOT NULL,
    status TEXT NOT NULL,
    http_status INTEGER,
    latency_ms INTEGER NOT NULL DEFAULT 0,
    tokens_in INTEGER,
    tokens_out INTEGER,
    model TEXT,
    error TEXT,
    response_summary TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_itg_egress_log_trace ON itg_egress_log(trace_id);
CREATE INDEX IF NOT EXISTS idx_itg_egress_log_client_time ON itg_egress_log(client_key, created_at);

-- ============================================================
-- App Bundle (M2): apps / app_ct_refs / app_licenses
-- ============================================================

-- Installed app registry + lifecycle state machine (app-bundle.md §3.2)
CREATE TABLE IF NOT EXISTS apps (
    id INTEGER PRIMARY KEY,
    app_id TEXT NOT NULL,
    version TEXT NOT NULL,
    status TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'upload',
    source_ref TEXT,
    signature_ok BOOLEAN NOT NULL DEFAULT 1,
    install_log TEXT,
    last_error TEXT,
    tenant_scope TEXT NOT NULL DEFAULT 'global',
    options TEXT,
    installed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (app_id)
);

CREATE INDEX IF NOT EXISTS idx_apps_status ON apps(status);

-- Materialized CT definitions owned by apps (app-bundle.md §6.3)
CREATE TABLE IF NOT EXISTS app_ct_refs (
    id INTEGER PRIMARY KEY,
    app_id TEXT NOT NULL,
    ct_table TEXT NOT NULL,
    schema_toml TEXT NOT NULL,
    version TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (app_id, ct_table)
);

-- Per-tenant licenses for global apps (app-bundle.md §9)
CREATE TABLE IF NOT EXISTS app_licenses (
    id INTEGER PRIMARY KEY,
    app_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    granted_by INTEGER,
    granted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (app_id, tenant_id)
);

-- ============================================================
-- Seed data
-- ============================================================

-- Default tenant
INSERT OR IGNORE INTO tenants (id, name, domain, config, status, created_at, updated_at) VALUES
    (10001, 'Default', NULL, '{}', 'active', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Default currencies
INSERT OR IGNORE INTO currencies (id, tenant_id, code, name, decimals) VALUES
    (10001, 'default', 'CNY', 'Chinese Yuan', 2),
    (10002, 'default', 'USD', 'US Dollar', 2),
    (10003, 'default', 'EUR', 'Euro', 2),
    (10004, 'default', 'GBP', 'British Pound', 2),
    (10005, 'default', 'JPY', 'Japanese Yen', 0);

-- System roles
INSERT OR IGNORE INTO roles (id, tenant_id, name, description, is_system, created_at, updated_at) VALUES
    (10001, 'default', 'admin', 'Super administrator', 1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10002, 'default', 'editor', 'Editor', 0, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10003, 'default', 'author', 'Author', 0, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10004, 'default', 'reader', 'Reader', 1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Admin global permissions
INSERT OR IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10001, 'default', (SELECT id FROM roles WHERE name = 'admin'), '*', '*', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Editor permissions
INSERT OR IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10002, 'default', (SELECT id FROM roles WHERE name = 'editor'), '*.*', '*', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Author permissions
INSERT OR IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10003, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'posts', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10004, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'posts', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10005, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'posts', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10006, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'posts', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10007, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'pages', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10008, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'pages', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10009, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'pages', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10010, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'pages', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10036, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'media', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10011, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'media', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10012, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'media', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10013, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'tags', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10014, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'tags', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10015, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'tags', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10016, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'categories', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10017, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'categories', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10018, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'categories', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10019, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'reusable_blocks', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10020, 'default', (SELECT id FROM roles WHERE name = 'author'), 'read', 'reusable_blocks', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10021, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'reusable_blocks', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10022, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'reusable_blocks', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10023, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'comments', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10024, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'comments', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10025, 'default', (SELECT id FROM roles WHERE name = 'author'), 'create', 'product_categories', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10026, 'default', (SELECT id FROM roles WHERE name = 'author'), 'update', 'product_categories', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10027, 'default', (SELECT id FROM roles WHERE name = 'author'), 'delete', 'product_categories', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Reader permissions
INSERT OR IGNORE INTO permissions (id, tenant_id, role_id, action, subject, fields, conditions, created_at) VALUES
    (10028, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'posts', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10029, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'pages', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10030, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'comments', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10031, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'comments', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10032, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'user_addresses', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10033, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'user_addresses', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10034, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'update', 'user_addresses', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10035, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'user_addresses', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10050, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'cart_items', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10051, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'cart_items', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10052, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'update', 'cart_items', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10053, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'cart_items', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10054, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'orders', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10055, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'orders', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10056, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'product_comments', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10057, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'product_comments', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10058, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'create', 'product_favorites', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10059, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'read', 'product_favorites', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10060, 'default', (SELECT id FROM roles WHERE name = 'reader'), 'delete', 'product_favorites', NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- Site options
INSERT OR IGNORE INTO options (id, tenant_id, option_key, value, type, group_name, label, description, validation, is_public, autoload, sort_order, updated_at) VALUES
    (10001, 'default', 'site_title', '"My Blog"', 'text', 'general', 'Site title', 'Displayed in browser title bar and page header', '{"max_length":100}', 1, 1, 1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10002, 'default', 'site_description', '""', 'text', 'general', 'Site description', 'Brief description of the site purpose', '{"max_length":500}', 1, 1, 2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10003, 'default', 'site_url', '""', 'url', 'general', 'Site URL', 'e.g. https://example.com', NULL, 1, 1, 3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10004, 'default', 'admin_email', '""', 'email', 'general', 'Admin email', NULL, NULL, 0, 1, 4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10005, 'default', 'timezone', '"UTC"', 'select', 'general', 'Timezone', NULL, '{"values":["UTC","Asia/Shanghai","Asia/Tokyo","US/Eastern","US/Pacific","Europe/London","Europe/Berlin"]}', 1, 1, 5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10006, 'default', 'date_format', '"%Y-%m-%d"', 'select', 'general', 'Date format', NULL, '{"values":["%Y-%m-%d","%d/%m/%Y","%m/%d/%Y","%Y年%m月%d日"]}', 1, 1, 6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10007, 'default', 'posts_per_page', '10', 'integer', 'reading', 'Posts per page', NULL, '{"min":1,"max":100}', 1, 1, 10, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10008, 'default', 'rss_items', '20', 'integer', 'reading', 'RSS item count', NULL, '{"min":1,"max":100}', 1, 1, 11, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10009, 'default', 'permalink_structure', '"/:year/:month/:slug"', 'select', 'reading', 'URL structure', NULL, '{"values":["/:year/:month/:slug","/:slug","/posts/:slug"]}', 1, 1, 12, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10010, 'default', 'comment_moderation', 'true', 'boolean', 'discussion', 'Comments require moderation', 'When enabled, new comments require admin approval', NULL, 0, 1, 20, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10011, 'default', 'comment_order', '"asc"', 'select', 'discussion', 'Comment order', NULL, '{"values":["asc","desc"]}', 1, 1, 21, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10012, 'default', 'default_role', '"reader"', 'select', 'discussion', 'Default role for new users', NULL, '{"values":["reader","author"]}', 0, 1, 22, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10013, 'default', 'theme', '"default"', 'select', 'appearance', 'Current theme', NULL, '{"values":["default","corporate","minimal","warm"]}', 1, 1, 30, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10014, 'default', 'maintenance_mode', 'false', 'boolean', 'appearance', 'Maintenance mode', 'When enabled, a maintenance page is shown to visitors', NULL, 1, 1, 31, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10015, 'default', 'default_currency', '"USD"', 'select', 'ecommerce', 'Default currency', 'Currency code for products and orders', '{"values":["USD","CNY","EUR","GBP","JPY","KRW","HKD","TWD","SGD","AUD","CAD"]}', 1, 1, 40, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    (10017, 'default', 'reserved_usernames', '"admin,administrator,root,system,official,support,staff,moderator,mod,help,info,mail,webmaster,security,billing,sales,owner,superuser,operator"', 'text', 'general', 'Reserved usernames', 'Comma-separated usernames that cannot be registered', '{"max_length":10000}', 0, 1, 5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

-- ============================================================
-- Flow orchestration engine v2 (dev-docs/workflow) — P0-P1 5 tables
-- Data is engine-own (independent namespace); egress only referenced.
-- ============================================================

CREATE TABLE IF NOT EXISTS flow (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    name TEXT NOT NULL,
    description TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    current_version INTEGER,
    extra TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_flow_tenant ON flow(tenant_id);

-- Immutable definition snapshots (publish = append; instance locks a version)
CREATE TABLE IF NOT EXISTS flow_version (
    id INTEGER PRIMARY KEY,
    flow_id INTEGER NOT NULL,
    version_number INTEGER NOT NULL,
    definition TEXT NOT NULL,
    created_by INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (flow_id, version_number)
);
CREATE INDEX IF NOT EXISTS idx_flow_version_flow ON flow_version(flow_id);

-- One run of a flow (wf_trace root = instance id)
CREATE TABLE IF NOT EXISTS flow_instance (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    flow_id INTEGER NOT NULL,
    flow_version_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    has_exceptions INTEGER NOT NULL DEFAULT 0,
    trigger_kind TEXT NOT NULL,
    trigger_payload TEXT,
    inputs_summary TEXT,
    outputs TEXT,
    error TEXT,
    started_by INTEGER,
    started_at TEXT,
    finished_at TEXT,
    waiting_kind TEXT,
    waiting_needed INTEGER,
    waiting_received INTEGER NOT NULL DEFAULT 0,
    resume_until TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_flow_instance_flow_status ON flow_instance(flow_id, status);
CREATE INDEX IF NOT EXISTS idx_flow_instance_status_time ON flow_instance(status, created_at);
CREATE INDEX IF NOT EXISTS idx_flow_instance_waiting ON flow_instance(status, resume_until);

-- Durable runnable state (whole snapshot, 1:1, rewritten each step)
CREATE TABLE IF NOT EXISTS flow_instance_snapshot (
    instance_id INTEGER PRIMARY KEY,
    snapshot TEXT NOT NULL,
    snapshot_version INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

-- Per-node run history (engine-own observability)
CREATE TABLE IF NOT EXISTS flow_node_run (
    id INTEGER PRIMARY KEY,
    instance_id INTEGER NOT NULL,
    node_id TEXT NOT NULL,
    node_type TEXT NOT NULL,
    seq INTEGER NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    latency_ms INTEGER,
    input_summary TEXT,
    output_summary TEXT,
    usage_json TEXT,
    error TEXT,
    egress_log_id INTEGER,
    container_ref TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_flow_node_run_instance_seq ON flow_node_run(instance_id, seq);
CREATE INDEX IF NOT EXISTS idx_flow_node_run_egress ON flow_node_run(egress_log_id);

-- Public API keys for flows (external invocation auth)
CREATE TABLE IF NOT EXISTS flow_api_key (
    id INTEGER PRIMARY KEY,
    flow_id INTEGER NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    token_enc TEXT NOT NULL,
    slug TEXT UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    require_auth INTEGER NOT NULL DEFAULT 1,
    last_used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_flow_api_key_flow ON flow_api_key(flow_id);

-- Internal flow triggers (event/cron): point at a flow, decoupled from flow.
CREATE TABLE IF NOT EXISTS flow_trigger (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    flow_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    event_type TEXT,
    filter TEXT,
    cron_expr TEXT,
    inputs_map TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_triggered_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_flow_trigger_kind_event ON flow_trigger(kind, event_type);
CREATE INDEX IF NOT EXISTS idx_flow_trigger_flow ON flow_trigger(flow_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- AI Agent core (namespace ai_*). See dev-docs/agent/db-schema.md.
-- Multi-tenant; ids are app-assigned Snowflake (BIGINT). JSON stored as TEXT,
-- BOOLEAN as INTEGER(0/1), timestamps as UTC TEXT.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ai_agents (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id INTEGER,
    name TEXT NOT NULL,
    system_prompt TEXT NOT NULL DEFAULT '',
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    temperature REAL,
    max_iterations INTEGER NOT NULL DEFAULT 10,
    tools TEXT NOT NULL,
    memory_enabled INTEGER NOT NULL DEFAULT 1,
    params TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (tenant_id, name)
);
CREATE INDEX IF NOT EXISTS idx_ai_agents_tenant ON ai_agents(tenant_id);

CREATE TABLE IF NOT EXISTS ai_sessions (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    agent_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open',
    meta TEXT,
    last_seq INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    last_active_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);
CREATE INDEX IF NOT EXISTS idx_ai_sessions_tenant_agent ON ai_sessions(tenant_id, agent_id);
CREATE INDEX IF NOT EXISTS idx_ai_sessions_owner_active ON ai_sessions(user_id, last_active_at);

CREATE TABLE IF NOT EXISTS ai_messages (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    session_id INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'chat',
    content TEXT NOT NULL DEFAULT '',
    tool_calls TEXT,
    tool_call_id TEXT,
    tool_name TEXT,
    tool_success INTEGER,
    tool_error TEXT,
    tool_elapsed_ms INTEGER,
    tool_truncated INTEGER,
    reasoning_content TEXT,
    call_usage TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (session_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_ai_messages_session_seq ON ai_messages(session_id, seq);
CREATE INDEX IF NOT EXISTS idx_ai_messages_session_role ON ai_messages(session_id, role, seq);

CREATE TABLE IF NOT EXISTS ai_memories (
    id INTEGER PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    agent_id INTEGER NOT NULL,
    user_id BIGINT,
    session_id INTEGER,
    mem_key TEXT NOT NULL,
    content TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'core',
    importance REAL NOT NULL DEFAULT 0.5,
    superseded_by INTEGER,
    pinned INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (tenant_id, agent_id, user_id, mem_key)
);
CREATE INDEX IF NOT EXISTS idx_ai_memories_agent_live ON ai_memories(agent_id, superseded_by);
CREATE INDEX IF NOT EXISTS idx_ai_memories_agent_category ON ai_memories(agent_id, category);
