-- Multi-agent review debates (dev-docs/agent/multi-agent.md §13): ai_debates
-- table + ai_sessions.parent_id for debate child sessions.
-- Baseline drift fix for databases created before the change; idempotent —
-- fresh installs from the updated baseline already carry both and are
-- unaffected (matches 0001's ADD COLUMN IF NOT EXISTS convention).
ALTER TABLE ai_sessions ADD COLUMN IF NOT EXISTS parent_id BIGINT;

CREATE TABLE IF NOT EXISTS ai_debates (
    id BIGINT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    user_id BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    agent_a_id BIGINT NOT NULL,
    agent_b_id BIGINT NOT NULL,
    origin_session_id BIGINT,
    requirement TEXT NOT NULL,
    params JSONB,
    ledger JSONB NOT NULL,
    rounds_done INTEGER NOT NULL DEFAULT 0,
    report TEXT,
    usage_total JSONB,
    error TEXT,
    heartbeat_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ(0) NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_ai_debates_tenant_status ON ai_debates(tenant_id, status);
CREATE INDEX IF NOT EXISTS idx_ai_debates_agent_a ON ai_debates(agent_a_id);
CREATE INDEX IF NOT EXISTS idx_ai_debates_agent_b ON ai_debates(agent_b_id);
