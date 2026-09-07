//! raisfast full-stack development platform core library
//!
//! A high-performance full-stack development platform built with Rust + Axum,
//! supporting `SQLite` / `PostgreSQL` / `MySQL`.
//! Architecture layers: Handler → Service → Model → DB.
//!
//! Supports two runtime modes:
//! - **server** — Standalone HTTP server (Axum)
//! - **tauri** — Tauri desktop application backend (shared Service layer)

#![deny(unsafe_code)]
#![allow(clippy::missing_errors_doc)]

#[macro_use]
mod macros;

#[cfg(feature = "export-types")]
pub mod export_type;

pub mod agent;
pub mod app;
pub mod apps;
pub mod cache;
pub mod commands;
pub mod config;
pub mod constants;
pub mod content_type;
pub mod db;
pub use db::DbDriver;
pub mod dto;
pub mod errors;
pub mod event;
pub mod eventbus;
pub mod flows;
pub mod graphql;
pub mod handlers;
pub mod integration;
#[cfg(feature = "mcp")]
pub mod mcp;
#[cfg(feature = "mcp")]
pub mod mcp_client;
pub mod middleware;
pub mod models;
pub mod notifier;
pub mod oauth;
pub mod panic_hook;
pub mod payment;
pub mod plugins;
pub mod policy;
pub mod presence;
pub mod protocols;
pub mod search;
pub mod server;
pub mod services;
pub mod storage;
pub mod types;
pub mod utils;
pub mod webhook;
pub mod worker;
pub mod workflow;

pub mod admin_spa;

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(all(feature = "proxy", unix))]
pub mod proxy;

#[inline]
pub(crate) fn _brand() -> String {
    let k0: u8 = 0x5A;
    let k1: u8 = 0xA5;
    let p0 = utils::tz::_B0;
    let p1: [u8; 4] = [70 ^ 0xA5, 97 ^ 0xA5, 115 ^ 0xA5, 116 ^ 0xA5];
    let mut v = Vec::with_capacity(p0.len() + p1.len());
    for b in p0 {
        v.push(b ^ k0);
    }
    for b in p1 {
        v.push(b ^ k1);
    }
    String::from_utf8(v).unwrap_or_default()
}

use app::ServiceRegistry;
use config::app::AppConfig;
use content_type::ContentTypeRegistry;
use db::Pool;
use eventbus::EventBus;
use notifier::{EmailSender, SmsSender};
use oauth::OAuthProviderRegistry;
use plugins::PluginManager;
use search::SearchEngine;
use services::audit::AuditService;
use services::options::OptionsService;
use services::rbac::RbacService;
use services::tenant::TenantService;
use std::sync::Arc;
use storage::Storage;
use webhook::WebhookService;
use workflow::WorkflowService;

pub use cache::CacheStore;

rust_i18n::i18n!("../../locales", fallback = "en");

/// Application global shared state
///
/// Injected into all handlers via axum `State`.
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub config: Arc<AppConfig>,
    pub jwt_decoding_key: jsonwebtoken::DecodingKey,
    pub plugins: Arc<PluginManager>,
    pub eventbus: EventBus,
    pub post_service: Arc<dyn crate::services::post::PostService>,
    pub page_service: Arc<dyn crate::services::page::PageService>,
    pub category_service: Arc<dyn crate::services::category::CategoryService>,
    pub product_category_service:
        Arc<dyn crate::services::product_category::ProductCategoryService>,
    pub tag_service: Arc<dyn crate::services::tag::TagService>,
    pub comment_service: Arc<dyn crate::services::comment::CommentService>,
    pub user_service: Arc<dyn crate::services::user::UserService>,
    pub wallet_service: Arc<dyn crate::services::wallet::WalletService>,
    pub product_service: Arc<dyn crate::services::product::ProductService>,
    pub order_service: Arc<dyn crate::services::order::OrderService>,
    pub cart_service: Arc<dyn crate::services::cart::CartService>,
    pub product_variant_service: Arc<dyn crate::services::product_variant::ProductVariantService>,
    pub product_comment_service: Arc<dyn crate::services::product_comment::ProductCommentService>,
    pub coupon_service: Arc<dyn crate::services::coupon::CouponService>,
    pub shipping_template_service:
        Arc<dyn crate::services::shipping_template::ShippingTemplateService>,
    pub user_address_service: Arc<dyn crate::services::user_address::UserAddressService>,
    pub payment_service: Arc<dyn crate::services::payment::PaymentService>,
    pub search: Arc<dyn SearchEngine>,
    pub content_type_registry: Arc<ContentTypeRegistry>,
    pub emitter: crate::event::EventEmitter,
    pub protocol_registry: Arc<crate::protocols::ProtocolRegistry>,
    pub options: Arc<OptionsService>,
    pub rbac: Arc<RbacService>,
    pub tenant: Arc<TenantService>,
    pub audit: Arc<AuditService>,
    pub webhook: Arc<WebhookService>,
    pub presence: Arc<dyn crate::presence::PresenceStore>,
    pub integration: Option<Arc<integration::IntegrationPlane>>,
    pub apps: Arc<apps::AppRegistry>,
    pub workflow: Arc<WorkflowService>,
    pub storage: Arc<dyn Storage>,
    pub cache: Arc<dyn CacheStore>,
    pub cms_cache: Arc<dashmap::DashMap<String, (serde_json::Value, std::time::Instant)>>,
    pub oauth_registry: Arc<OAuthProviderRegistry>,
    pub email_sender: Arc<dyn EmailSender>,
    pub sms_sender: Arc<dyn SmsSender>,
    pub route_registry: Arc<Vec<crate::server::RouteInfo>>,
    pub route_perms: Arc<crate::middleware::permission_guard::RoutePermissionMap>,
    pub services: ServiceRegistry,
    pub handler_registry: Arc<crate::worker::JobHandlerRegistry>,
}

/// Install the process-wide rustls CryptoProvider (ring).
///
/// The dependency tree pulls BOTH `ring` (tungstenite/lettre) and
/// `aws-lc-rs` (reqwest chains), so rustls cannot auto-detect a default —
/// the first TLS handshake of a fresh runtime would PANIC
/// ("Could not automatically determine the process-level CryptoProvider").
/// Installing once, up front, keeps every consumer (reqwest, axum-server,
/// tokio-tungstenite, lettre) on one provider.
pub fn install_tls_provider() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let provider = rustls::crypto::ring::default_provider();
        if provider.install_default().is_err() {
            // Already installed by another path — fine, same process.
            tracing::debug!("rustls provider already installed (ring)");
        }
    });
}

/// Build AppState (shared by HTTP server and Tauri)
pub async fn build_app_state(
    config: &AppConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<AppState> {
    let pool = crate::db::connection::init_pool(&config.database_url, config.db_pool_size).await?;
    crate::db::connection::ensure_schema(&pool).await?;

    let live_tables = crate::db::connection::fetch_table_names(&pool).await;

    let eventbus = EventBus::new(256);
    crate::panic_hook::set_event_bus(eventbus.clone());

    let cache: Arc<dyn crate::cache::CacheStore> = Arc::new(crate::cache::MemoryCache::new());

    let search: Arc<dyn SearchEngine> = build_search_engine(config);

    let mut protocol_registry = crate::protocols::ProtocolRegistry::new();
    protocol_registry.register_from_inventory();
    let protocol_registry = Arc::new(protocol_registry);

    let user_service: Arc<dyn crate::services::user::UserService> = Arc::new(
        crate::services::user::UserServiceImpl::new(Arc::new(pool.clone())),
    );

    let options_service =
        Arc::new(OptionsService::new(Arc::new(pool.clone()), config.builtin_tenantable).await);

    let cart_service: Arc<dyn crate::services::cart::CartService> = Arc::new(
        crate::services::cart::CartServiceImpl::new(Arc::new(pool.clone())),
    );

    let product_variant_service: Arc<dyn crate::services::product_variant::ProductVariantService> =
        Arc::new(
            crate::services::product_variant::ProductVariantServiceImpl::new(Arc::new(
                pool.clone(),
            )),
        );

    let product_comment_service: Arc<dyn crate::services::product_comment::ProductCommentService> =
        Arc::new(
            crate::services::product_comment::ProductCommentServiceImpl::new(Arc::new(
                pool.clone(),
            )),
        );

    let coupon_service: Arc<dyn crate::services::coupon::CouponService> = Arc::new(
        crate::services::coupon::CouponServiceImpl::new(Arc::new(pool.clone())),
    );

    let shipping_template_service: Arc<
        dyn crate::services::shipping_template::ShippingTemplateService,
    > = Arc::new(
        crate::services::shipping_template::ShippingTemplateServiceImpl::new(Arc::new(
            pool.clone(),
        )),
    );

    let user_address_service: Arc<dyn crate::services::user_address::UserAddressService> = Arc::new(
        crate::services::user_address::UserAddressServiceImpl::new(Arc::new(pool.clone())),
    );

    let reserved = config.builtins.reserved_route_segments();
    let protocol_names: Vec<&str> = protocol_registry.names();
    let ct_registry = Arc::new(ContentTypeRegistry::load_from_dir(
        std::path::Path::new(&config.content_type_dir),
        &config.rule_engine,
        &reserved,
        &protocol_names,
        &protocol_registry,
    )?);

    // App Bundle: kill -9 self-heal + replay app CTs from `app_ct_refs`
    // BEFORE the protected-table snapshot and the migrate loop (three-source
    // rebuild: builtins → directory scan (above) → DB app CTs).
    let apps_registry = crate::apps::AppRegistry::init(
        pool.clone(),
        Arc::new(config.clone()),
        ct_registry.clone(),
        protocol_registry.clone(),
    )
    .await?;
    crate::apps::set_shared(apps_registry.clone());

    let ct_tables: Vec<String> = ct_registry
        .all()
        .iter()
        .map(|ct| ct.table.clone())
        .collect();
    crate::db::schema::set_protected_tables(live_tables, &ct_tables);
    ct_registry.set_protected_tables(crate::db::schema::get_protected_tables());

    // Validate relation targets after all CTs are loaded and protected tables
    // are known. CTs with dangling relation targets are unregistered here so
    // that migrate() and runtime queries never hit a missing-table error.
    ct_registry.validate_relations();

    {
        let repo = crate::content_type::repository::ContentRepository::new(pool.clone());
        for schema in ct_registry.all() {
            repo.migrate(&schema, &protocol_registry).await?;
        }
    }

    let presence_store: Arc<dyn crate::presence::PresenceStore> =
        Arc::new(crate::presence::InMemoryPresenceStore::new());

    let plugin_manager = PluginManager::new_with_options(
        Arc::new(config.clone()),
        crate::plugins::PluginManagerOptions {
            pool: Some(pool.clone()),
            event_bus: Some(eventbus.clone()),
            content_registry: Some(ct_registry.clone()),
            presence_store: Some(presence_store.clone()),
        },
    )
    .await;

    let emitter = crate::event::EventEmitter::new(eventbus.clone(), &plugin_manager);
    let integration_plane = if config.integration.enabled {
        let plane = Arc::new(
            integration::IntegrationPlane::init(
                pool.clone(),
                config.integration.clone(),
                config.storage_root_dir.clone(),
                ct_registry.clone(),
                emitter.clone(),
                config.jwt_secret.clone(),
            )
            .await?,
        );
        integration::set_shared(plane.clone());
        Some(plane)
    } else {
        None
    };

    // Shared LLM runtime for the flows `llm` node (mirrors the integration
    // shared-handle pattern; llm-node.md §3 W1 — built once, never per call).
    crate::agent::service::set_shared_llm(if config.ai.enabled {
        crate::agent::service::provider_from_config(&config.ai)
            .ok()
            .map(|provider| {
                std::sync::Arc::new(crate::agent::service::SharedLlm {
                    provider,
                    default_model: config.ai.model.clone(),
                    timeout_ms: config.ai.timeout_secs.saturating_mul(1000),
                })
            })
    } else {
        None
    });

    // App Bundle late attach: reconcile plugin state with app status
    // (non-enabled apps unload the plugins load_all picked up from disk).
    apps_registry
        .attach(plugin_manager.clone(), integration_plane.clone())
        .await?;

    let order_service: Arc<dyn crate::services::order::OrderService> =
        Arc::new(crate::services::order::OrderServiceImpl::new(
            emitter.clone(),
            Arc::new(pool.clone()),
            options_service.clone(),
        ));

    let wallet_service: Arc<dyn crate::services::wallet::WalletService> = Arc::new(
        crate::services::wallet::WalletServiceImpl::new(emitter.clone(), Arc::new(pool.clone())),
    );

    let payment_service: Arc<dyn crate::services::payment::PaymentService> =
        Arc::new(crate::services::payment::PaymentServiceImpl::new(
            Arc::new(config.clone()),
            emitter.clone(),
            Arc::new(pool.clone()),
        ));

    tracing::info!(
        "app state initialized with {} protocol(s)",
        protocol_registry.names().len()
    );

    let post_service: Arc<dyn crate::services::post::PostService> =
        Arc::new(crate::services::post::PostServiceImpl::new(
            Arc::new(pool.clone()),
            emitter.clone(),
            search.clone(),
        ));

    let tag_service: Arc<dyn crate::services::tag::TagService> = Arc::new(
        crate::services::tag::TagServiceImpl::new(emitter.clone(), Arc::new(pool.clone())),
    );
    let category_service: Arc<dyn crate::services::category::CategoryService> =
        Arc::new(crate::services::category::CategoryServiceImpl::new(
            emitter.clone(),
            Arc::new(pool.clone()),
        ));
    let product_category_service: Arc<
        dyn crate::services::product_category::ProductCategoryService,
    > = Arc::new(
        crate::services::product_category::ProductCategoryServiceImpl::new(
            emitter.clone(),
            Arc::new(pool.clone()),
        ),
    );
    let page_service: Arc<dyn crate::services::page::PageService> = Arc::new(
        crate::services::page::PageServiceImpl::new(emitter.clone(), Arc::new(pool.clone())),
    );
    let comment_service: Arc<dyn crate::services::comment::CommentService> = Arc::new(
        crate::services::comment::CommentServiceImpl::new(Arc::new(pool.clone()), emitter.clone()),
    );

    let product_service: Arc<dyn crate::services::product::ProductService> =
        Arc::new(crate::services::product::ProductServiceImpl::new(
            emitter.clone(),
            Arc::new(pool.clone()),
            options_service.clone(),
        ));

    let rbac_service = Arc::new(RbacService::new(Arc::new(pool.clone()), Arc::clone(&cache)));

    let tenant_service = Arc::new(TenantService::new(Arc::new(pool.clone())));
    let audit_service = Arc::new(crate::services::audit::AuditService::new(pool.clone()));
    let webhook_service = Arc::new(crate::webhook::WebhookService::new(pool.clone()));

    let storage = crate::storage::create_storage(config)?;

    let mut svc_builder = app::ServiceRegistryBuilder::new();
    svc_builder.register(search.clone());
    svc_builder.register(protocol_registry.clone());
    svc_builder.register(ct_registry.clone());
    svc_builder.register(options_service.clone());
    svc_builder.register(rbac_service.clone());
    svc_builder.register(tenant_service.clone());
    svc_builder.register(audit_service.clone());
    svc_builder.register(webhook_service.clone());
    svc_builder.register(cache.clone());
    svc_builder.register(storage.clone());
    let services = svc_builder.build();

    let state = AppState {
        pool: pool.clone(),
        config: Arc::new(config.clone()),
        jwt_decoding_key: jsonwebtoken::DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        plugins: plugin_manager,
        eventbus: eventbus.clone(),
        post_service,
        page_service,
        category_service,
        product_category_service,
        tag_service,
        comment_service,
        user_service,
        wallet_service,
        product_service,
        order_service,
        cart_service,
        product_variant_service,
        product_comment_service,
        coupon_service,
        shipping_template_service,
        user_address_service,
        payment_service,
        search,
        content_type_registry: ct_registry,
        emitter,
        protocol_registry,
        options: options_service,
        rbac: rbac_service,
        tenant: tenant_service,
        audit: audit_service,
        webhook: webhook_service.clone(),
        presence: presence_store.clone(),
        integration: integration_plane,
        apps: apps_registry,
        workflow: Arc::new(WorkflowService::new(pool.clone())),
        storage,
        cache: cache.clone(),
        cms_cache: Arc::new(dashmap::DashMap::new()),
        oauth_registry: Arc::new(build_oauth_registry(config)),
        email_sender: crate::notifier::build_email_sender(config),
        sms_sender: crate::notifier::build_sms_sender(config),
        route_registry: Arc::new(Vec::new()),
        route_perms: Arc::new(
            crate::middleware::permission_guard::RoutePermissionMap::from_routes(&[]),
        ),
        services,
        handler_registry: Arc::new(crate::worker::JobHandlerRegistry::new()),
    };

    crate::server::spawn_audit_subscriber(
        eventbus.clone(),
        state.audit.clone(),
        state.tenant.clone(),
        shutdown_rx.clone(),
    );
    crate::server::spawn_webhook_subscriber(
        eventbus.clone(),
        state.webhook.clone(),
        pool.clone(),
        shutdown_rx.clone(),
    );

    crate::flows::trigger::spawn_flow_event_subscriber(
        eventbus.clone(),
        state.pool.clone(),
        state.plugins.clone(),
        shutdown_rx.clone(),
    );
    crate::flows::trigger::spawn_flow_cron_subscriber(
        state.pool.clone(),
        state.plugins.clone(),
        shutdown_rx.clone(),
    );

    // Presence reaper: converts stale heartbeats into offline transitions
    // (architecture §5.3). Cold-start = everyone offline; reconnect revives.
    crate::presence::spawn_reaper(
        presence_store.clone(),
        eventbus.clone(),
        std::time::Duration::from_secs(config.presence_heartbeat_ttl_secs),
        std::time::Duration::from_secs(config.presence_sweep_interval_secs),
        shutdown_rx,
    );

    Ok(state)
}

/// Build search engine instance
pub fn build_search_engine(config: &AppConfig) -> Arc<dyn SearchEngine> {
    match config.search_engine.as_str() {
        #[cfg(feature = "search-tantivy")]
        "tantivy" => match crate::search::TantivyEngine::open(&config.search_index_dir) {
            Ok(engine) => {
                tracing::info!(
                    "search engine: tantivy (index: {})",
                    config.search_index_dir
                );
                Arc::new(engine)
            }
            Err(e) => {
                tracing::error!("failed to open tantivy index: {e}, falling back to noop");
                Arc::new(crate::search::NoopSearchEngine)
            }
        },
        _ => Arc::new(crate::search::NoopSearchEngine),
    }
}

/// Build OAuth Provider registry
pub fn build_oauth_registry(config: &AppConfig) -> OAuthProviderRegistry {
    let mut registry = OAuthProviderRegistry::new();
    if let Some(gh) = &config.oauth.github {
        registry.register(Box::new(crate::oauth::github::GitHubProvider::new(
            gh.client_id.clone(),
            gh.client_secret.clone(),
        )));
        tracing::info!("OAuth provider registered: github");
    }
    if let Some(google) = &config.oauth.google {
        registry.register(Box::new(crate::oauth::google::GoogleProvider::new(
            google.client_id.clone(),
            google.client_secret.clone(),
        )));
        tracing::info!("OAuth provider registered: google");
    }
    if let Some(wechat) = &config.oauth.wechat {
        registry.register(Box::new(crate::oauth::wechat::WechatProvider::new(
            wechat.app_id.clone(),
            wechat.app_secret.clone(),
            config.base_url.clone(),
        )));
        tracing::info!("OAuth provider registered: wechat");
    }
    registry
}
