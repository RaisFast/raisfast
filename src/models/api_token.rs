//! API Token model and database queries
//!
//! Defines the data structure for API Tokens (long-lived access tokens) and
//! create, find, delete operations on the `api_tokens` table. Tokens are stored
//! as SHA-256 hashes; the plaintext is returned only once at creation time.

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
#[cfg(feature = "export-types")]
use ts_rs::TS;

use crate::db::{DbDriver, Driver};
use crate::errors::app_error::AppResult;
use crate::types::snowflake_id::SnowflakeId;
use crate::utils::tz::Timestamp;

/// API Token full database row model
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: SnowflakeId,
    pub user_id: SnowflakeId,
    pub name: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub scopes: String,
    pub last_used_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

/// API Token list item (sanitized, without token_hash)
#[cfg_attr(feature = "export-types", derive(TS))]
#[derive(Debug, FromRow, Serialize, Deserialize)]
pub struct ApiTokenListItem {
    pub id: SnowflakeId,
    pub name: String,
    pub token_prefix: String,
    #[serde(serialize_with = "serialize_scopes_as_array")]
    pub scopes: String,
    pub last_used_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

/// Custom serializer for `scopes` - parses a JSON string into an array for output
fn serialize_scopes_as_array<S>(scopes: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let parsed: Vec<String> = serde_json::from_str(scopes).unwrap_or_default();
    let mut seq = serializer.serialize_seq(Some(parsed.len()))?;
    for s in &parsed {
        seq.serialize_element(s)?;
    }
    seq.end()
}

/// Create a new API Token record
pub async fn create(
    pool: &crate::db::Pool,
    user_id: SnowflakeId,
    name: &str,
    token_hash: &str,
    token_prefix: &str,
    scopes: &str,
    expires_at: Option<&str>,
) -> AppResult<ApiToken> {
    let (id, now) = (
        crate::utils::id::new_snowflake_id(),
        crate::utils::tz::now_utc(),
    );
    raisfast_derive::crud_insert!(pool, "api_tokens", [
        "id" => id,
        "user_id" => user_id,
        "name" => name,
        "token_hash" => token_hash,
        "token_prefix" => token_prefix,
        "scopes" => scopes,
        "expires_at" => expires_at,
        "created_at" => now
    ])?;
    find_by_id(pool, id).await?.ok_or_else(|| {
        crate::errors::app_error::AppError::Internal(anyhow::anyhow!(
            "failed to fetch newly created api token"
        ))
    })
}

/// Find API Token by token_hash
pub async fn find_by_hash(pool: &crate::db::Pool, token_hash: &str) -> AppResult<Option<ApiToken>> {
    raisfast_derive::crud_find!(pool, "api_tokens", ApiToken, where: ("token_hash", token_hash))
        .map_err(Into::into)
}

/// List all API Tokens for a given user (sanitized)
pub async fn list_by_user(
    pool: &crate::db::Pool,
    user_id: SnowflakeId,
) -> AppResult<Vec<ApiTokenListItem>> {
    raisfast_derive::check_schema!(
        "api_tokens",
        "id",
        "name",
        "token_prefix",
        "scopes",
        "last_used_at",
        "expires_at",
        "created_at",
        "user_id"
    );
    let sql = format!(
        "SELECT id, name, token_prefix, scopes, last_used_at, expires_at, created_at FROM api_tokens WHERE user_id = {} ORDER BY created_at DESC",
        Driver::ph(1)
    );
    let rows = sqlx::query_as::<_, ApiTokenListItem>(&sql)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Find API Token by id
pub async fn find_by_id(pool: &crate::db::Pool, id: SnowflakeId) -> AppResult<Option<ApiToken>> {
    raisfast_derive::crud_find!(pool, "api_tokens", ApiToken, where: ("id", id)).map_err(Into::into)
}

/// Delete API Token by id
pub async fn delete_by_id(pool: &crate::db::Pool, id: SnowflakeId) -> AppResult<()> {
    raisfast_derive::crud_delete!(pool, "api_tokens", where: ("id", id))?;
    Ok(())
}

/// Update last_used_at (by integer primary key)
pub async fn touch_last_used(pool: &crate::db::Pool, id: SnowflakeId) -> AppResult<()> {
    let now = crate::utils::tz::now_utc();
    raisfast_derive::crud_update!(pool, "api_tokens",
        bind: ["last_used_at" => now],
        where: ("id", id)
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::snowflake_id::SnowflakeId;

    async fn setup_pool() -> crate::db::Pool {
        crate::test_pool!()
    }

    async fn insert_user(pool: &crate::db::Pool) -> i64 {
        let user = crate::models::user::create(
            pool,
            &crate::commands::user::CreateUserCmd {
                username: crate::utils::id::new_id().to_string(),
                registered_via: crate::models::user::RegisteredVia::Email,
                role: None,
            },
            None,
        )
        .await
        .unwrap();
        *user.id
    }

    #[tokio::test]
    async fn create_and_find_by_hash() {
        let pool = setup_pool().await;
        let user_id = insert_user(&pool).await;
        let row = create(
            &pool,
            SnowflakeId(user_id),
            "Test",
            "hash123",
            "rblog_ab",
            "[\"read\"]",
            None,
        )
        .await
        .unwrap();
        assert_eq!(row.name, "Test");
        assert_eq!(row.token_hash, "hash123");
        assert_eq!(row.token_prefix, "rblog_ab");
        assert_eq!(row.scopes, "[\"read\"]");
        assert!(row.expires_at.is_none());

        let found = find_by_hash(&pool, "hash123").await.unwrap().unwrap();
        assert_eq!(found.id, row.id);
    }

    #[tokio::test]
    async fn find_by_hash_not_found() {
        let pool = setup_pool().await;
        let result = find_by_hash(&pool, "nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn find_by_id_not_found() {
        let pool = setup_pool().await;
        let result = find_by_id(&pool, SnowflakeId(99999)).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_by_user_returns_tokens() {
        let pool = setup_pool().await;
        let user_id = insert_user(&pool).await;
        create(
            &pool,
            SnowflakeId(user_id),
            "First",
            "h1",
            "rblog_a",
            "[\"read\"]",
            None,
        )
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        create(
            &pool,
            SnowflakeId(user_id),
            "Second",
            "h2",
            "rblog_b",
            "[\"write\"]",
            None,
        )
        .await
        .unwrap();

        let list = list_by_user(&pool, SnowflakeId(user_id)).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Second");
        assert_eq!(list[1].name, "First");
    }

    #[tokio::test]
    async fn list_by_user_empty() {
        let pool = setup_pool().await;
        let list = list_by_user(&pool, SnowflakeId(99999)).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn delete_by_id_removes_token() {
        let pool = setup_pool().await;
        let user_id = insert_user(&pool).await;
        let row = create(
            &pool,
            SnowflakeId(user_id),
            "Del",
            "h3",
            "rblog_c",
            "[\"read\"]",
            None,
        )
        .await
        .unwrap();
        delete_by_id(&pool, row.id).await.unwrap();
        let found = find_by_id(&pool, row.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn touch_last_used_updates_field() {
        let pool = setup_pool().await;
        let user_id = insert_user(&pool).await;
        let row = create(
            &pool,
            SnowflakeId(user_id),
            "Touch",
            "h4",
            "rblog_d",
            "[\"read\"]",
            None,
        )
        .await
        .unwrap();
        assert!(row.last_used_at.is_none());

        touch_last_used(&pool, row.id).await.unwrap();
        let updated = find_by_id(&pool, row.id).await.unwrap().unwrap();
        assert!(updated.last_used_at.is_some());
    }

    #[tokio::test]
    async fn create_with_expires_at() {
        let pool = setup_pool().await;
        let user_id = insert_user(&pool).await;
        let row = create(
            &pool,
            SnowflakeId(user_id),
            "Expiring",
            "h5",
            "rblog_e",
            "[\"admin\"]",
            Some("2099-12-31T00:00:00+00:00"),
        )
        .await
        .unwrap();
        assert_eq!(
            row.expires_at.unwrap(),
            "2099-12-31T00:00:00+00:00".parse::<Timestamp>().unwrap()
        );
    }

    #[tokio::test]
    async fn list_by_user_does_not_include_hash() {
        let pool = setup_pool().await;
        let user_id = insert_user(&pool).await;
        create(
            &pool,
            SnowflakeId(user_id),
            "Safe",
            "secret_hash",
            "rblog_f",
            "[\"read\"]",
            None,
        )
        .await
        .unwrap();
        let list = list_by_user(&pool, SnowflakeId(user_id)).await.unwrap();
        let json = serde_json::to_value(&list[0]).unwrap();
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("token_hash"));
        assert!(!obj.contains_key("user_id"));
    }
}
