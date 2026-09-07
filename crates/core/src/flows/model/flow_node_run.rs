//! flow_node_run model + queries — per-node execution history (observability).
//! Engine writes one row per node (latest state, upserted on resume/retry);
//! UI lists them under an instance.

use serde::Serialize;

use crate::db::{DbDriver, Driver};
use crate::errors::app_error::AppResult;
use crate::types::snowflake_id::SnowflakeId;
use crate::utils::tz::Timestamp;

const RUN_COLS: &str = "id, instance_id, node_id, node_type, seq, attempt, status, \
     started_at, finished_at, latency_ms, input_summary, output_summary, usage_json, error, created_at";

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FlowNodeRun {
    pub id: SnowflakeId,
    pub instance_id: SnowflakeId,
    pub node_id: String,
    pub node_type: String,
    pub seq: i64,
    pub attempt: i64,
    pub status: String,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub latency_ms: Option<i64>,
    pub input_summary: Option<String>,
    pub output_summary: Option<String>,
    /// LLM usage payload ({"prompt_tokens","completion_tokens","total_tokens"})
    /// for billing (llm-node.md §6); serialized JSON text.
    pub usage_json: Option<String>,
    pub error: Option<String>,
    pub created_at: Timestamp,
}

fn now() -> Timestamp {
    crate::utils::tz::now_utc()
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "success" | "failed" | "skipped" | "canceled" | "error_output"
    )
}

/// Record the latest state of a node for an instance (upsert by instance+node).
/// On first sight it inserts a row with the next `seq`; on resume/retry it
/// updates the same row so the UI shows the final outcome per node.
#[allow(clippy::too_many_arguments)]
pub async fn record_node_run(
    pool: &crate::db::Pool,
    instance_id: SnowflakeId,
    node_id: &str,
    node_type: &str,
    status: &str,
    attempt: i64,
    input: Option<&str>,
    output: Option<&str>,
    error: Option<&str>,
    usage: Option<&str>,
    latency_ms: Option<i64>,
) -> AppResult<()> {
    let found: Option<i64> = sqlx::query_scalar(crate::db::safe_sql(&format!(
        "SELECT id FROM flow_node_run WHERE instance_id = {} AND node_id = {} \
             ORDER BY seq DESC LIMIT 1",
        Driver::ph(1),
        Driver::ph(2)
    )))
    .bind(*instance_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await?;

    match found {
        Some(row_id) => {
            let finished = if is_terminal(status) {
                Some(now())
            } else {
                None
            };
            let sql = format!(
                "UPDATE flow_node_run SET status = {}, attempt = {}, input_summary = {}, \
                 output_summary = {}, error = {}, usage_json = {}, latency_ms = {}, \
                 finished_at = {} WHERE id = {}",
                Driver::ph(1),
                Driver::ph(2),
                Driver::ph(3),
                Driver::ph(4),
                Driver::ph(5),
                Driver::ph(6),
                Driver::ph(7),
                Driver::ph(8),
                Driver::ph(9)
            );
            sqlx::query(crate::db::safe_sql(&sql))
                .bind(status)
                .bind(attempt)
                .bind(input)
                .bind(output)
                .bind(error)
                .bind(usage)
                .bind(latency_ms)
                .bind(finished)
                .bind(row_id)
                .execute(pool)
                .await?;
        }
        None => {
            let next_seq: i64 = sqlx::query_scalar(crate::db::safe_sql(&format!(
                "SELECT {} FROM flow_node_run WHERE instance_id = {}",
                Driver::cast_int("COALESCE(MAX(seq), 0) + 1"),
                Driver::ph(1)
            )))
            .bind(*instance_id)
            .fetch_one(pool)
            .await?;
            let sql = format!(
                "INSERT INTO flow_node_run (id, instance_id, node_id, node_type, seq, attempt, \
                 status, finished_at, input_summary, output_summary, error, usage_json, \
                 latency_ms, created_at) \
                 VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                Driver::ph(1),
                Driver::ph(2),
                Driver::ph(3),
                Driver::ph(4),
                Driver::ph(5),
                Driver::ph(6),
                Driver::ph(7),
                Driver::ph(8),
                Driver::ph(9),
                Driver::ph(10),
                Driver::ph(11),
                Driver::ph(12),
                Driver::ph(13),
                Driver::ph(14)
            );
            let finished = if is_terminal(status) {
                Some(now())
            } else {
                None
            };
            let started = now();
            sqlx::query(crate::db::safe_sql(&sql))
                .bind(*crate::utils::id::new_snowflake_id())
                .bind(*instance_id)
                .bind(node_id)
                .bind(node_type)
                .bind(next_seq)
                .bind(attempt)
                .bind(status)
                .bind(finished)
                .bind(input)
                .bind(output)
                .bind(error)
                .bind(usage)
                .bind(latency_ms)
                .bind(started)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Node runs for one instance, oldest first.
pub async fn find_node_runs(
    pool: &crate::db::Pool,
    instance_id: SnowflakeId,
) -> AppResult<Vec<FlowNodeRun>> {
    let sql = format!(
        "SELECT {RUN_COLS} FROM flow_node_run WHERE instance_id = {} ORDER BY seq ASC, id ASC",
        Driver::ph(1)
    );
    Ok(
        sqlx::query_as::<crate::db::pool::Db, FlowNodeRun>(crate::db::safe_sql(&sql))
            .bind(*instance_id)
            .fetch_all(pool)
            .await?,
    )
}
