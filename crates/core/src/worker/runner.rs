//! Worker background polling executor
//!
//! Dispatch chain: built-in Handler Registry → plugin Cron Dispatcher → mark dead

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::cancellation::{JOB_CANCEL, JOB_CANCELS};
use crate::constants::COL_ID;
use crate::db::{DbDriver, Driver, Pool};
use crate::errors::app_error::{AppError, AppResult};
use crate::types::snowflake_id::SnowflakeId;

use super::{
    CronExecStatus, JobHandlerRegistry, JobQueue, JobStatus, JobTypeFilter, PluginCronDispatcher,
    QueuedJob,
};

/// Worker executor
pub struct WorkerRunner {
    queue: Arc<dyn JobQueue>,
    handlers: Arc<JobHandlerRegistry>,
    plugin_dispatcher: Option<Arc<PluginCronDispatcher>>,
    pool: Pool,
    poll_interval: Duration,
    batch_size: usize,
    /// Global job visibility timeout, used as the heartbeat cadence basis for
    /// jobs without a per-job `timeout_secs`. Mirrors
    /// `worker_visibility_timeout_secs`.
    visibility_timeout: Duration,
    /// Which job types this pool may claim. `Any` for a single-pool setup.
    claim: JobTypeFilter,
    /// Shutdown signal. When it flips true the worker stops claiming and exits
    /// after finishing the job it is currently running.
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
    /// Global hard runtime cap applied to jobs without their own `timeout_secs`
    /// (`None` = no global cap). Bounds runaway jobs.
    hard_timeout: Option<Duration>,
}

impl WorkerRunner {
    /// Creates a new `WorkerRunner`
    ///
    /// When `plugin_dispatcher` is `None`, unmatched jobs are directly marked dead.
    pub fn new(
        queue: Arc<dyn JobQueue>,
        handlers: Arc<JobHandlerRegistry>,
        pool: Pool,
        poll_interval: Duration,
        batch_size: usize,
    ) -> Self {
        Self {
            queue,
            handlers,
            plugin_dispatcher: None,
            pool,
            poll_interval,
            batch_size,
            visibility_timeout: Duration::from_secs(300),
            claim: JobTypeFilter::Any,
            shutdown: None,
            hard_timeout: None,
        }
    }

    /// Attaches a shutdown signal so the worker drains and exits cleanly.
    #[must_use]
    pub fn with_shutdown(mut self, shutdown: tokio::sync::watch::Receiver<bool>) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Sets a global hard runtime cap for jobs without their own `timeout_secs`.
    #[must_use]
    pub fn with_hard_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.hard_timeout = timeout;
        self
    }

    /// Sets the global visibility timeout used as the heartbeat cadence basis
    /// for jobs without a per-job `timeout_secs`.
    #[must_use]
    pub fn with_visibility_timeout(mut self, timeout: Duration) -> Self {
        self.visibility_timeout = timeout;
        self
    }

    /// Restricts which job types this pool claims (IO pool = `Except(cpu)`,
    /// CPU pool = `Only(cpu)`).
    #[must_use]
    pub fn with_claim_filter(mut self, claim: JobTypeFilter) -> Self {
        self.claim = claim;
        self
    }

    /// Sets the plugin Cron dispatcher
    #[must_use]
    pub fn with_plugin_dispatcher(mut self, dispatcher: Arc<PluginCronDispatcher>) -> Self {
        self.plugin_dispatcher = Some(dispatcher);
        self
    }

    /// Spawns N concurrent workers
    pub fn spawn(self, concurrency: usize) {
        for i in 0..concurrency {
            let runner = self.clone_for_worker();
            tokio::spawn(async move {
                tracing::info!("worker-{i} started");
                runner.run(i).await;
                tracing::error!("worker-{i} exited unexpectedly");
            });
        }
    }

    async fn run(self, worker_id: usize) {
        let mut interval = tokio::time::interval(self.poll_interval);

        loop {
            // Graceful shutdown: stop claiming once signalled. `execute_batch`
            // requeues whatever it had claimed but not started.
            if self.is_shutdown() {
                tracing::info!("worker-{worker_id} shutting down (drained)");
                return;
            }

            match self
                .queue
                .dequeue_filtered(self.batch_size, &self.claim)
                .await
            {
                Ok(jobs) if jobs.is_empty() => {
                    // Idle: throttle to the poll interval.
                    interval.tick().await;
                }
                Ok(jobs) => {
                    // Busy: claim again immediately instead of paying the poll
                    // interval per batch. Combined with a small batch size this
                    // keeps throughput while bounding crash exposure.
                    self.execute_batch(&jobs, worker_id).await;
                }
                Err(e) => {
                    tracing::error!("worker-{worker_id} dequeue error: {e}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.as_ref().is_some_and(|rx| *rx.borrow())
    }

    /// Returns claimed-but-unstarted jobs to `pending` (graceful shutdown).
    async fn requeue_jobs(&self, jobs: &[&super::QueuedJob]) {
        for job in jobs {
            if let Err(e) = self.queue.requeue(&job.id).await {
                tracing::error!("failed to requeue job {} on shutdown: {e}", job.id);
            }
        }
    }

    async fn execute_batch(&self, jobs: &[super::QueuedJob], worker_id: usize) {
        // Partition by coalesce key first (no execution yet) so a graceful
        // shutdown can requeue everything that has not started.
        use std::collections::BTreeMap;
        let mut plain: Vec<&super::QueuedJob> = Vec::new();
        let mut coalesce_groups: BTreeMap<String, Vec<&super::QueuedJob>> = BTreeMap::new();
        for job in jobs {
            let handler = self.handlers.get_handler(job.job.job_type());
            match handler.and_then(|h| h.coalesce_key(&job.job)) {
                Some(k) => coalesce_groups.entry(k).or_default().push(job),
                None => plain.push(job),
            }
        }

        for (i, job) in plain.iter().enumerate() {
            if self.is_shutdown() {
                tracing::info!("worker-{worker_id} shutdown: requeueing unstarted jobs");
                self.requeue_jobs(&plain[i..]).await;
                for group in coalesce_groups.values() {
                    self.requeue_jobs(group).await;
                }
                return;
            }
            if let Err(e) = self.execute(job).await {
                tracing::error!("worker-{worker_id} job {} error: {e}", job.id);
            }
        }

        let groups: Vec<(String, Vec<&super::QueuedJob>)> = coalesce_groups.into_iter().collect();
        for (i, (key, group)) in groups.iter().enumerate() {
            if self.is_shutdown() {
                tracing::info!("worker-{worker_id} shutdown: requeueing unstarted coalesced jobs");
                for (_, g) in &groups[i..] {
                    self.requeue_jobs(g).await;
                }
                return;
            }
            let job_type = group[0].job.job_type().to_string();
            let Some(h) = self.handlers.get_handler(&job_type) else {
                tracing::warn!("no handler for coalesced key '{key}'");
                continue;
            };
            // A handler that defines coalesce_key but returns None from
            // coalesce must not silently drop claimed jobs — fall back to
            // executing the first job of the group.
            let merged = match h.coalesce(group.iter().map(|q| q.job.clone()).collect()) {
                Some(merged) => merged,
                None => group[0].job.clone(),
            };
            let handler_start = std::time::Instant::now();
            let heartbeat = self.spawn_heartbeat(
                group.iter().map(|q| q.id.clone()).collect(),
                heartbeat_interval(self.effective_timeout(group[0])),
            );
            let enforced = group[0]
                .timeout_secs
                .filter(|s| *s > 0)
                .map(|s| Duration::from_secs(s as u64))
                .or(self.hard_timeout);
            let ids: Vec<String> = group.iter().map(|q| q.id.clone()).collect();
            let cancel_rx = JOB_CANCELS.register_many(&ids);
            let (result, timed_out, cancelled) = JOB_CANCEL
                .scope(cancel_rx.clone(), async {
                    run_cancellable(enforced, self.handlers.handle(&merged), cancel_rx)
                        .await
                        .into_result()
                })
                .await;
            JOB_CANCELS.finish(&ids);
            heartbeat.abort();
            let elapsed_ms = handler_start.elapsed().as_millis() as i64;

            if cancelled {
                for q in group {
                    if let Err(er) = self.queue.cancel(&q.id).await {
                        tracing::error!(
                            "worker-{worker_id} failed to cancel coalesced job {}: {er}",
                            q.id
                        );
                    }
                    self.writeback_cron_log(
                        q,
                        Err(format!("cancelled (elapsed {elapsed_ms}ms)")),
                        false,
                    )
                    .await;
                }
            } else if let Err(e) = result {
                tracing::error!("worker-{worker_id} coalesced '{key}' error: {e}");
                for q in group {
                    if let Err(er) = self.queue.fail(&q.id, &format!("{e}")).await {
                        tracing::error!(
                            "worker-{worker_id} failed to fail coalesced job {}: {er}",
                            q.id
                        );
                    }
                    self.writeback_cron_log(
                        q,
                        Err(format!("{e} (elapsed {elapsed_ms}ms)")),
                        timed_out,
                    )
                    .await;
                }
            } else {
                for q in group {
                    if let Err(er) = self.queue.complete(&q.id).await {
                        tracing::error!(
                            "worker-{worker_id} failed to complete coalesced job {}: {er}",
                            q.id
                        );
                    }
                    self.writeback_cron_log(q, Ok(elapsed_ms), false).await;
                }
            }
        }
    }

    async fn execute(&self, job: &super::QueuedJob) -> super::AppResult<()> {
        let job_type = job.job.job_type();

        tracing::debug!(
            "executing job {} type={} attempt={}/{}",
            job.id,
            job_type,
            job.attempts,
            job.max_attempts,
        );

        // Measure handler execution time for cron log writeback.
        let handler_start = std::time::Instant::now();

        let heartbeat = self.spawn_heartbeat(
            vec![job.id.clone()],
            heartbeat_interval(self.effective_timeout(job)),
        );

        // Per-job timeout wins; otherwise fall back to the global hard cap.
        // Either way the heartbeat keeps the visibility lease alive while the
        // job runs (see §10.1).
        let enforced = job
            .timeout_secs
            .filter(|s| *s > 0)
            .map(|s| Duration::from_secs(s as u64))
            .or(self.hard_timeout);

        let uses_handler = self.handlers.has_handler(job_type);
        if !uses_handler && self.plugin_dispatcher.is_none() {
            tracing::warn!("no handler for job type '{job_type}', marking dead");
            heartbeat.abort();
            self.queue.dead(&job.id, "no handler registered").await?;
            self.trace_flip(job, false, "no handler registered".to_string())
                .await;
            self.writeback_cron_log(job, Err("no handler registered".to_string()), false)
                .await;
            return Ok(());
        }

        let cancel_rx = JOB_CANCELS.register(&job.id);
        let outcome = JOB_CANCEL
            .scope(cancel_rx.clone(), async {
                if uses_handler {
                    run_cancellable(enforced, self.handlers.handle_queued(job), cancel_rx).await
                } else if let Some(ref dispatcher) = self.plugin_dispatcher {
                    tracing::info!("no built-in handler for '{job_type}', dispatching to plugins");
                    run_cancellable(enforced, dispatcher.dispatch(&job.job), cancel_rx).await
                } else {
                    // Unreachable: `plugin_dispatcher` was checked above.
                    ExecOutcome::Failed(AppError::Internal(anyhow::anyhow!(
                        "no handler registered"
                    )))
                }
            })
            .await;
        JOB_CANCELS.finish(std::slice::from_ref(&job.id));

        heartbeat.abort();

        let elapsed_ms = handler_start.elapsed().as_millis() as i64;

        match outcome {
            ExecOutcome::Done => {
                self.queue.complete(&job.id).await?;
                self.trace_flip(job, true, elapsed_ms.to_string()).await;
                self.writeback_cron_log(job, Ok(elapsed_ms), false).await;
            }
            ExecOutcome::TimedOut(d) => {
                let msg = format!("job timed out after {}s", d.as_secs());
                self.settle_failure(job, msg, true, elapsed_ms).await?;
            }
            ExecOutcome::Cancelled => {
                self.settle_cancelled(job, elapsed_ms).await?;
            }
            ExecOutcome::Failed(e) => {
                self.settle_failure(job, format!("{e}"), false, elapsed_ms)
                    .await?;
            }
        }
        Ok(())
    }

    /// Terminal transition for an admin-cancelled job. Idempotent: the admin
    /// endpoint may have already written `cancelled`.
    async fn settle_cancelled(&self, job: &QueuedJob, elapsed_ms: i64) -> AppResult<()> {
        self.queue.cancel(&job.id).await?;
        self.trace_flip(job, false, "cancelled".to_string()).await;
        self.writeback_cron_log(
            job,
            Err(format!("cancelled (elapsed {elapsed_ms}ms)")),
            false,
        )
        .await;
        Ok(())
    }

    /// Applies the terminal transition for a failed job: `dead` once retries
    /// are exhausted, otherwise `fail` (backoff retry). `timed_out` is written
    /// to the cron execution log as `CronExecStatus::TimedOut`.
    async fn settle_failure(
        &self,
        job: &QueuedJob,
        err_msg: String,
        timed_out: bool,
        elapsed_ms: i64,
    ) -> AppResult<()> {
        let became_dead = job.attempts >= job.max_attempts;
        if became_dead {
            self.queue.dead(&job.id, &err_msg).await?;
        } else {
            self.queue.fail(&job.id, &err_msg).await?;
        }
        self.trace_flip(job, false, err_msg.clone()).await;
        self.writeback_cron_log(
            job,
            Err(format!(
                "{err_msg} (elapsed {elapsed_ms}ms, dead={became_dead})"
            )),
            timed_out,
        )
        .await;
        Ok(())
    }

    /// Effective visibility timeout for a job: its own `timeout_secs` when set,
    /// otherwise the global `visibility_timeout`.
    fn effective_timeout(&self, job: &QueuedJob) -> Duration {
        job.timeout_secs
            .filter(|s| *s > 0)
            .map(|s| Duration::from_secs(s as u64))
            .unwrap_or(self.visibility_timeout)
    }

    /// Spawns a task that periodically bumps `updated_at` for the given running
    /// jobs. Without it, `StuckJobSweeper` reclaims a long-running job as stuck
    /// and re-dispatches it while the original execution is still in flight.
    /// The caller must `abort()` the returned handle once the handler returns.
    fn spawn_heartbeat(&self, ids: Vec<String>, interval: Duration) -> tokio::task::JoinHandle<()> {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                for id in &ids {
                    if let Err(e) = touch_running_job(&pool, id).await {
                        tracing::warn!("heartbeat for job {id} failed: {e}");
                    }
                }
            }
        })
    }

    /// Integration-plane trace writeback: flip the receipt's pending
    /// `job:{type}` placeholder to its terminal state (§10.7). No-op for
    /// jobs without a `trace_id` payload.
    async fn trace_flip(&self, job: &super::QueuedJob, ok: bool, detail: String) {
        if let crate::worker::Job::Custom { job_type, payload } = &job.job
            && let Some(trace_id) = crate::types::snowflake_id::parse_id_value(
                payload.get("trace_id").unwrap_or(&serde_json::Value::Null),
            )
        {
            let res = crate::integration::receipt::flip_pending_step(
                &self.pool,
                crate::types::snowflake_id::SnowflakeId::new(trace_id),
                job_type,
                ok,
                &detail,
            )
            .await;
            if let Err(err) = res {
                tracing::warn!(trace_id, job_type, error = %err, "trace flip failed");
            }
        }
    }

    /// Write back the real execution outcome to `cron_execution_log`.
    ///
    /// Only called when the job has cron provenance (`cron_log_id` is Some).
    /// The `Ok(i64)` arm carries duration_ms; the `Err` arm carries the error string.
    /// `timed_out` marks the failure as `CronExecStatus::TimedOut` (takes priority
    /// over `Dead`, since the cause is more specific than exhausted retries).
    async fn writeback_cron_log(
        &self,
        job: &super::QueuedJob,
        outcome: Result<i64, String>,
        timed_out: bool,
    ) {
        let Some(log_id) = job.cron_log_id else {
            return; // Not a cron-originated job (EventBus / ad-hoc enqueue)
        };
        let now = crate::utils::tz::now_utc();
        let log_id: SnowflakeId = log_id;
        match outcome {
            Ok(duration_ms) => {
                let res = crate::worker::complete_execution_log_with(
                    &self.pool,
                    log_id,
                    duration_ms,
                    now,
                )
                .await;
                if let Err(e) = res {
                    tracing::warn!("failed to writeback cron log {log_id}: {e}");
                }
            }
            Err(err_str) => {
                let became_dead = job.attempts >= job.max_attempts;
                let status = if timed_out {
                    CronExecStatus::TimedOut
                } else if became_dead {
                    CronExecStatus::Dead
                } else {
                    CronExecStatus::Failed
                };
                let res = crate::worker::fail_execution_log_with(
                    &self.pool, log_id, status, &err_str, now,
                )
                .await;
                if let Err(e) = res {
                    tracing::warn!("failed to writeback cron log {log_id}: {e}");
                }
            }
        }
    }

    fn clone_for_worker(&self) -> Self {
        Self {
            queue: self.queue.clone(),
            handlers: self.handlers.clone(),
            plugin_dispatcher: self.plugin_dispatcher.clone(),
            pool: self.pool.clone(),
            poll_interval: self.poll_interval,
            batch_size: self.batch_size,
            visibility_timeout: self.visibility_timeout,
            claim: self.claim.clone(),
            shutdown: self.shutdown.clone(),
            hard_timeout: self.hard_timeout,
        }
    }
}

/// Outcome of a handler run under the optional hard timeout.
enum ExecOutcome {
    Done,
    Failed(AppError),
    TimedOut(Duration),
    Cancelled,
}

impl ExecOutcome {
    /// Collapses to `(result, timed_out, cancelled)` for the coalesced path,
    /// which settles the whole group uniformly.
    fn into_result(self) -> (AppResult<()>, bool, bool) {
        match self {
            ExecOutcome::Done => (Ok(()), false, false),
            ExecOutcome::Failed(e) => (Err(e), false, false),
            ExecOutcome::TimedOut(d) => (
                Err(AppError::Internal(anyhow::anyhow!(
                    "job timed out after {}s",
                    d.as_secs()
                ))),
                true,
                false,
            ),
            ExecOutcome::Cancelled => (
                Err(AppError::Internal(anyhow::anyhow!("job cancelled"))),
                false,
                true,
            ),
        }
    }
}

/// Runs `fut` under the optional timeout, aborting early if the job is
/// cancelled. Cancellation is cooperative: the future is dropped at its next
/// await; out-of-process executors read [`JOB_CANCEL`] to hard-kill their child.
async fn run_cancellable<F>(
    limit: Option<Duration>,
    fut: F,
    mut cancel_rx: watch::Receiver<bool>,
) -> ExecOutcome
where
    F: std::future::Future<Output = AppResult<()>>,
{
    tokio::select! {
        outcome = run_with_timeout(limit, fut) => outcome,
        _ = cancel_rx.changed() => ExecOutcome::Cancelled,
    }
}

/// Runs `fut` under an optional hard timeout. A timeout is reported distinctly
/// so it can be logged as `CronExecStatus::TimedOut`. Note: the underlying sync
/// CPU work (e.g. `spawn_blocking` parse) is not cancellable, so it keeps
/// running in the background — the heartbeat keeps the row alive until it ends.
async fn run_with_timeout<F>(limit: Option<Duration>, fut: F) -> ExecOutcome
where
    F: std::future::Future<Output = AppResult<()>>,
{
    match limit {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(Ok(())) => ExecOutcome::Done,
            Ok(Err(e)) => ExecOutcome::Failed(e),
            Err(_) => ExecOutcome::TimedOut(d),
        },
        None => match fut.await {
            Ok(()) => ExecOutcome::Done,
            Err(e) => ExecOutcome::Failed(e),
        },
    }
}

/// Heartbeat cadence: roughly three beats per visibility window, clamped to
/// `[1s, 60s]` so short timeouts are still respected without hammering the DB.
fn heartbeat_interval(timeout: Duration) -> Duration {
    (timeout / 3).clamp(Duration::from_secs(1), Duration::from_secs(60))
}

/// Bumps `updated_at` on a still-`running` job. No-op once the job reached a
/// terminal state, so it can never resurrect a completed/failed row.
async fn touch_running_job(pool: &Pool, job_id: &str) -> AppResult<()> {
    let id: i64 = job_id
        .parse()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid id: {e}")))?;
    let now = crate::utils::tz::now_utc();
    let sql = format!(
        "UPDATE jobs SET updated_at = {} WHERE {COL_ID} = {} AND status = {}",
        Driver::ph(1),
        Driver::ph(2),
        Driver::ph(3)
    );
    sqlx::query::<crate::db::pool::Db>(crate::db::safe_sql(&sql))
        .bind(now)
        .bind(id)
        .bind(JobStatus::Running.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::snowflake_id::SnowflakeId;
    use crate::worker::{DefaultJobQueue, Job, LogJobHandler, NewJob};

    struct FailHandler;

    #[async_trait::async_trait]
    impl crate::worker::JobHandler for FailHandler {
        async fn handle(&self, _job: &Job) -> crate::errors::app_error::AppResult<()> {
            Err(crate::errors::app_error::AppError::BadRequest(
                "fail".into(),
            ))
        }
    }

    async fn setup() -> (
        Arc<DefaultJobQueue>,
        Arc<JobHandlerRegistry>,
        crate::db::Pool,
    ) {
        let pool = crate::test_pool!();
        // Clear leftover rows from previous test runs (shared PG database).
        sqlx::query("DELETE FROM jobs")
            .execute(&pool)
            .await
            .unwrap();
        let queue = Arc::new(DefaultJobQueue::new(pool.clone()));
        let mut registry = JobHandlerRegistry::new();
        registry.register("generate_sitemap", Box::new(LogJobHandler));
        registry.register("send_welcome_email", Box::new(FailHandler));
        registry.register("rebuild_search_index", Box::new(LogJobHandler));
        (queue, Arc::new(registry), pool)
    }

    #[test]
    fn heartbeat_interval_is_clamped() {
        assert_eq!(
            heartbeat_interval(Duration::from_secs(300)),
            Duration::from_secs(60)
        );
        assert_eq!(
            heartbeat_interval(Duration::from_secs(90)),
            Duration::from_secs(30)
        );
        assert_eq!(
            heartbeat_interval(Duration::from_secs(3)),
            Duration::from_secs(1)
        );
    }

    #[tokio::test]
    async fn heartbeat_does_not_resurrect_terminal_job() {
        let (queue, _registry, pool) = setup().await;
        queue
            .enqueue(NewJob::from(Job::GenerateSitemap))
            .await
            .unwrap();
        let jobs = queue.dequeue(10).await.unwrap();
        let id = jobs[0].id.clone();
        queue.complete(&id).await.unwrap();

        assert!(touch_running_job(&pool, &id).await.is_ok());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.running, 0);
    }

    struct SlowHandler;

    #[async_trait::async_trait]
    impl crate::worker::JobHandler for SlowHandler {
        async fn handle(&self, _job: &Job) -> AppResult<()> {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn execute_enforces_timeout() {
        let (queue, _registry, pool) = setup().await;
        let mut registry = JobHandlerRegistry::new();
        registry.register("slow_job", Box::new(SlowHandler));
        let runner = WorkerRunner::new(
            queue.clone(),
            Arc::new(registry),
            pool,
            Duration::from_millis(50),
            5,
        );

        queue
            .enqueue(NewJob {
                job: Job::Custom {
                    job_type: "slow_job".into(),
                    payload: serde_json::json!({}),
                },
                max_attempts: Some(3),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: Some(1),
                dedup_key: None,
            })
            .await
            .unwrap();
        let jobs = queue.dequeue(10).await.unwrap();

        let start = std::time::Instant::now();
        assert!(runner.execute(&jobs[0]).await.is_ok());
        assert!(start.elapsed() < Duration::from_secs(3));

        // Timed out → retryable → back to pending (attempt 1 of 3).
        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.completed, 0);
    }

    struct CpuGateHandler {
        gate: Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl crate::worker::JobHandler for CpuGateHandler {
        async fn handle(&self, _job: &Job) -> AppResult<()> {
            self.gate.notified().await;
            Ok(())
        }

        fn execution_class(&self) -> crate::worker::ExecutionClass {
            crate::worker::ExecutionClass::Cpu
        }
    }

    /// A CPU-bound job held open must not prevent the IO pool from draining.
    /// Fails if the two pools ever share worker slots (the pre-P2 behavior).
    #[tokio::test]
    async fn timeout_is_logged_as_timed_out() {
        let (queue, _registry, pool) = setup().await;
        let schedule_id = crate::utils::id::new_snowflake_id();
        let log_id = crate::worker::create_execution_log(&pool, schedule_id, "slow_job", "test")
            .await
            .unwrap();

        let mut registry = JobHandlerRegistry::new();
        registry.register("slow_job", Box::new(SlowHandler));
        let runner = WorkerRunner::new(
            queue.clone(),
            Arc::new(registry),
            pool.clone(),
            Duration::from_millis(50),
            5,
        );

        queue
            .enqueue(NewJob {
                job: Job::Custom {
                    job_type: "slow_job".into(),
                    payload: serde_json::json!({}),
                },
                max_attempts: Some(3),
                run_after: None,
                cron_schedule_id: Some(schedule_id),
                cron_log_id: Some(crate::types::snowflake_id::SnowflakeId(log_id)),
                priority: 0,
                timeout_secs: Some(1),
                dedup_key: None,
            })
            .await
            .unwrap();

        let jobs = queue.dequeue(10).await.unwrap();
        runner.execute(&jobs[0]).await.unwrap();

        let (logs, _total) = crate::worker::list_execution_logs(&pool, schedule_id, 10, 0)
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status.as_str(), "timed_out");
    }

    #[tokio::test]
    async fn global_hard_timeout_applies_without_job_timeout() {
        let (queue, _registry, pool) = setup().await;
        let mut registry = JobHandlerRegistry::new();
        registry.register("slow_job", Box::new(SlowHandler));
        let runner = WorkerRunner::new(
            queue.clone(),
            Arc::new(registry),
            pool,
            Duration::from_millis(50),
            5,
        )
        .with_hard_timeout(Some(Duration::from_secs(1)));

        queue
            .enqueue(NewJob {
                job: Job::Custom {
                    job_type: "slow_job".into(),
                    payload: serde_json::json!({}),
                },
                max_attempts: Some(3),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: None, // no per-job timeout → global cap applies
                dedup_key: None,
            })
            .await
            .unwrap();

        let jobs = queue.dequeue(10).await.unwrap();
        let start = std::time::Instant::now();
        assert!(runner.execute(&jobs[0]).await.is_ok());
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(queue.stats().await.unwrap().pending, 1);
    }

    /// A job whose handler waits `delay` (simulates network/IO latency).
    struct BenchIo {
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl crate::worker::JobHandler for BenchIo {
        async fn handle(&self, _job: &Job) -> AppResult<()> {
            tokio::time::sleep(self.delay).await;
            Ok(())
        }
    }

    /// One benchmark scenario; returns elapsed. Run with:
    /// `just test 'bench_worker -- --ignored --nocapture'`
    async fn bench_scenario(
        delay: Duration,
        n: usize,
        concurrency: usize,
        batch: usize,
    ) -> Duration {
        let pool = crate::test_pool!();
        sqlx::query("DELETE FROM jobs")
            .execute(&pool)
            .await
            .unwrap();
        let queue = Arc::new(DefaultJobQueue::new(pool.clone()));
        let mut registry = JobHandlerRegistry::new();
        registry.register("bench_io", Box::new(BenchIo { delay }));
        let (tx, rx) = tokio::sync::watch::channel(false);
        let runner = WorkerRunner::new(
            queue.clone(),
            Arc::new(registry),
            pool.clone(),
            Duration::from_millis(50),
            batch,
        )
        .with_shutdown(rx);
        for _ in 0..n {
            queue
                .enqueue(NewJob::from(Job::Custom {
                    job_type: "bench_io".into(),
                    payload: serde_json::json!({}),
                }))
                .await
                .unwrap();
        }

        let start = std::time::Instant::now();
        runner.spawn(concurrency);
        loop {
            let s = queue.stats().await.unwrap();
            if s.completed + s.failed + s.dead >= n as i64 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let elapsed = start.elapsed();
        eprintln!(
            "[bench] delay={:>5}ms  concurrency={concurrency:>2}  jobs={n}  elapsed={elapsed:?}  throughput={:.0}/s",
            delay.as_millis(),
            n as f64 / elapsed.as_secs_f64()
        );

        // Stop this iteration's workers before the next scenario (shared DB on
        // PG/MySQL; harmless on SQLite).
        let _ = tx.send(true);
        tokio::time::sleep(Duration::from_millis(200)).await;
        elapsed
    }

    /// Microbenchmark: 1ms jobs (overhead-dominated). Run with
    /// `just test 'bench_worker -- --ignored --nocapture'`.
    #[tokio::test]
    #[ignore = "manual worker benchmark"]
    async fn bench_worker_overhead() {
        for batch in [20usize, 1] {
            for concurrency in [2usize, 8, 32] {
                let _ = bench_scenario(Duration::from_millis(1), 500, concurrency, batch).await;
            }
        }
    }

    /// Realistic IO benchmark: 100ms jobs (simulates a network call), default
    /// batch. Run with `just test 'bench_worker -- --ignored --nocapture'`.
    #[tokio::test]
    #[ignore = "manual worker benchmark"]
    async fn bench_worker_io_latency() {
        for concurrency in [8usize, 32, 64] {
            let _ = bench_scenario(Duration::from_millis(100), 100, concurrency, 20).await;
        }
    }

    /// Same as above but `batch_size = 1`: shows whether the flat throughput is
    /// caused by greedy batch claiming (a worker hoards 20 long jobs and runs
    /// them sequentially) rather than worker count.
    #[tokio::test]
    #[ignore = "manual worker benchmark"]
    async fn bench_worker_io_latency_batch1() {
        for concurrency in [8usize, 32, 64] {
            let _ = bench_scenario(Duration::from_millis(100), 100, concurrency, 1).await;
        }
    }

    /// Batch-size sweep for a tiny (overhead-bound) and a long (latency-bound)
    /// job, to pick a default. Run:
    /// `just test 'bench_worker_batch_sweep -- --ignored --nocapture'`.
    #[tokio::test]
    #[ignore = "manual worker benchmark"]
    async fn bench_worker_batch_sweep() {
        for delay_ms in [1u64, 100] {
            for batch in [1usize, 2, 5, 20] {
                for concurrency in [8usize, 32] {
                    let _ =
                        bench_scenario(Duration::from_millis(delay_ms), 200, concurrency, batch)
                            .await;
                }
            }
        }
    }

    #[tokio::test]
    async fn cancel_stops_running_job() {
        use crate::worker::{JobFilter, JobStatus};

        let (queue, _registry, pool) = setup().await;
        let gate = Arc::new(tokio::sync::Notify::new());
        let mut registry = JobHandlerRegistry::new();
        registry.register("gate", Box::new(CpuGateHandler { gate }));
        let runner = WorkerRunner::new(
            queue.clone(),
            Arc::new(registry),
            pool,
            Duration::from_millis(20),
            5,
        );
        queue
            .enqueue(NewJob {
                job: Job::Custom {
                    job_type: "gate".into(),
                    payload: serde_json::json!({}),
                },
                max_attempts: Some(3),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: None,
                dedup_key: None,
            })
            .await
            .unwrap();
        runner.spawn(1);

        // Wait until the runner has claimed and registered the job.
        let mut id = None;
        for _ in 0..500 {
            let (rows, _) = queue
                .list(
                    JobFilter {
                        status: Some(JobStatus::Running),
                        job_type: None,
                    },
                    1,
                    10,
                )
                .await
                .unwrap();
            if let Some(r) = rows.first() {
                id = Some(r.id.clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let id = id.expect("job was not claimed");
        assert!(crate::cancellation::JOB_CANCELS.cancel(&id));

        let mut cancelled = false;
        for _ in 0..500 {
            if queue.stats().await.unwrap().cancelled == 1 {
                cancelled = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(cancelled, "job was not cancelled");
        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.running, 0);
    }

    #[tokio::test]
    async fn shutdown_requeues_unstarted_jobs() {
        let (queue, _registry, pool) = setup().await;
        let gate = Arc::new(tokio::sync::Notify::new());
        let mut registry = JobHandlerRegistry::new();
        registry.register("gate", Box::new(CpuGateHandler { gate: gate.clone() }));
        registry.register("io_fast", Box::new(LogJobHandler));

        // First job blocks; the remaining four are claimed but not started.
        queue
            .enqueue(NewJob {
                job: Job::Custom {
                    job_type: "gate".into(),
                    payload: serde_json::json!({}),
                },
                max_attempts: Some(3),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: None,
                dedup_key: None,
            })
            .await
            .unwrap();
        for _ in 0..4 {
            queue
                .enqueue(NewJob::from(Job::Custom {
                    job_type: "io_fast".into(),
                    payload: serde_json::json!({}),
                }))
                .await
                .unwrap();
        }

        let (tx, rx) = tokio::sync::watch::channel(false);
        let runner = WorkerRunner::new(
            queue.clone(),
            Arc::new(registry),
            pool,
            Duration::from_millis(20),
            5,
        )
        .with_shutdown(rx);
        runner.spawn(1);

        // All five claimed; the gate job holds the worker.
        let mut claimed = false;
        for _ in 0..500 {
            if queue.stats().await.unwrap().running == 5 {
                claimed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(claimed, "batch was not claimed");

        tx.send(true).unwrap();
        gate.notify_one();

        let mut requeued = false;
        for _ in 0..500 {
            if queue.stats().await.unwrap().pending == 4 {
                requeued = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(requeued, "unstarted jobs were not requeued on shutdown");
        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.running, 0);
        assert_eq!(stats.completed, 1);
    }

    #[tokio::test]
    async fn cpu_pool_does_not_starve_io_pool() {
        let (queue, _registry, pool) = setup().await;

        let gate = Arc::new(tokio::sync::Notify::new());
        let mut registry = JobHandlerRegistry::new();
        registry.register("cpu_gate", Box::new(CpuGateHandler { gate: gate.clone() }));
        registry.register("io_fast", Box::new(LogJobHandler));
        let registry = Arc::new(registry);

        queue
            .enqueue(NewJob {
                job: Job::Custom {
                    job_type: "cpu_gate".into(),
                    payload: serde_json::json!({}),
                },
                max_attempts: Some(1),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: None,
                dedup_key: None,
            })
            .await
            .unwrap();
        for _ in 0..3 {
            queue
                .enqueue(NewJob::from(Job::Custom {
                    job_type: "io_fast".into(),
                    payload: serde_json::json!({}),
                }))
                .await
                .unwrap();
        }

        let cpu_runner = WorkerRunner::new(
            queue.clone(),
            registry.clone(),
            pool.clone(),
            Duration::from_millis(10),
            10,
        )
        .with_claim_filter(JobTypeFilter::Only(vec!["cpu_gate".to_string()]));
        cpu_runner.spawn(1);

        let io_runner = WorkerRunner::new(
            queue.clone(),
            registry.clone(),
            pool.clone(),
            Duration::from_millis(10),
            10,
        )
        .with_claim_filter(JobTypeFilter::Except(vec!["cpu_gate".to_string()]));
        io_runner.spawn(2);

        // The CPU job is claimed and blocked on the gate.
        let mut cpu_running = false;
        for _ in 0..500 {
            if queue.stats().await.unwrap().running == 1 {
                cpu_running = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(cpu_running, "CPU job was never claimed");

        // All IO jobs must finish while the CPU job is still held.
        let mut io_done = false;
        for _ in 0..500 {
            if queue.stats().await.unwrap().completed >= 3 {
                io_done = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(io_done, "IO jobs were starved by the blocked CPU job");
        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.running, 1, "CPU job should still be running");

        gate.notify_one();
        let mut all_done = false;
        for _ in 0..500 {
            if queue.stats().await.unwrap().completed >= 4 {
                all_done = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(all_done, "CPU job did not complete after release");
    }

    #[tokio::test]
    async fn shutdown_stops_claiming() {
        let (queue, registry, pool) = setup().await;
        queue
            .enqueue(NewJob::from(Job::GenerateSitemap))
            .await
            .unwrap();

        let (_tx, rx) = tokio::sync::watch::channel(true);
        let runner = WorkerRunner::new(queue.clone(), registry, pool, Duration::from_millis(20), 5)
            .with_shutdown(rx);
        runner.spawn(1);
        tokio::time::sleep(Duration::from_millis(120)).await;

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.running, 0);
    }

    #[tokio::test]
    async fn execute_completes_on_handler_success() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            5,
        );

        queue
            .enqueue(NewJob::from(Job::GenerateSitemap))
            .await
            .unwrap();
        let jobs = queue.dequeue(10).await.unwrap();
        assert_eq!(jobs.len(), 1);

        let result = runner.execute(&jobs[0]).await;
        assert!(result.is_ok());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.running, 0);
    }

    #[tokio::test]
    async fn execute_fails_and_retries() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            5,
        );

        queue
            .enqueue(NewJob {
                job: Job::SendWelcomeEmail {
                    user_id: SnowflakeId(1),
                    email: "a@b.com".into(),
                    username: "alice".into(),
                },
                max_attempts: Some(3),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: None,
                dedup_key: None,
            })
            .await
            .unwrap();

        let jobs = queue.dequeue(10).await.unwrap();
        assert_eq!(jobs[0].attempts, 1);

        let result = runner.execute(&jobs[0]).await;
        assert!(result.is_ok());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 1);
    }

    #[tokio::test]
    async fn execute_marks_dead_at_max_attempts() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            5,
        );

        queue
            .enqueue(NewJob {
                job: Job::SendWelcomeEmail {
                    user_id: SnowflakeId(1),
                    email: "a@b.com".into(),
                    username: "alice".into(),
                },
                max_attempts: Some(1),
                run_after: None,
                cron_schedule_id: None,
                cron_log_id: None,
                priority: 0,
                timeout_secs: None,
                dedup_key: None,
            })
            .await
            .unwrap();

        let jobs = queue.dequeue(10).await.unwrap();
        assert_eq!(jobs[0].attempts, 1);
        assert_eq!(jobs[0].max_attempts, 1);

        let result = runner.execute(&jobs[0]).await;
        assert!(result.is_ok());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.dead, 1);
        assert_eq!(stats.pending, 0);
    }

    #[tokio::test]
    async fn dequeue_empty_no_error() {
        let (queue, registry, pool) = setup().await;
        let _runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            5,
        );

        let jobs = queue.dequeue(10).await.unwrap();
        assert!(jobs.is_empty());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.pending, 0);
    }

    #[tokio::test]
    async fn spawn_processes_pending_jobs() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(50),
            5,
        );

        queue
            .enqueue(NewJob::from(Job::GenerateSitemap))
            .await
            .unwrap();

        runner.spawn(1);

        tokio::time::sleep(Duration::from_millis(300)).await;

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 1);
    }

    #[tokio::test]
    async fn unhandled_job_without_plugin_marks_dead() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            5,
        );

        queue
            .enqueue(NewJob::from(Job::Custom {
                job_type: "unknown_task".into(),
                payload: serde_json::json!({"x": 1}),
            }))
            .await
            .unwrap();

        let jobs = queue.dequeue(10).await.unwrap();
        assert_eq!(jobs.len(), 1);

        let result = runner.execute(&jobs[0]).await;
        assert!(result.is_ok());

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.dead, 1);
        assert_eq!(stats.completed, 0);
    }

    #[tokio::test]
    async fn coalesces_multiple_search_index_jobs() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            20,
        );

        queue
            .enqueue(NewJob::from(Job::RebuildSearchIndex {
                post_ids: vec![1, 2],
            }))
            .await
            .unwrap();
        queue
            .enqueue(NewJob::from(Job::RebuildSearchIndex {
                post_ids: vec![2, 3],
            }))
            .await
            .unwrap();
        queue
            .enqueue(NewJob::from(Job::RebuildSearchIndex { post_ids: vec![4] }))
            .await
            .unwrap();

        let jobs = queue.dequeue(20).await.unwrap();
        assert_eq!(jobs.len(), 3);

        runner.execute_batch(&jobs, 0).await;

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 3);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.running, 0);
        assert_eq!(stats.dead, 0);
    }

    #[tokio::test]
    async fn coalesces_search_index_with_mixed_jobs() {
        let (queue, registry, pool) = setup().await;
        let runner = WorkerRunner::new(
            queue.clone(),
            registry,
            pool.clone(),
            Duration::from_millis(100),
            20,
        );

        queue
            .enqueue(NewJob::from(Job::GenerateSitemap))
            .await
            .unwrap();
        queue
            .enqueue(NewJob::from(Job::RebuildSearchIndex { post_ids: vec![10] }))
            .await
            .unwrap();
        queue
            .enqueue(NewJob::from(Job::RebuildSearchIndex { post_ids: vec![20] }))
            .await
            .unwrap();

        let jobs = queue.dequeue(20).await.unwrap();
        assert_eq!(jobs.len(), 3);

        runner.execute_batch(&jobs, 0).await;

        let stats = queue.stats().await.unwrap();
        assert_eq!(stats.completed, 3);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.dead, 0);
    }
}
