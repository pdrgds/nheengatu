# Monitoring

## What's in place

| Signal | How |
|---|---|
| Structured logs | `tracing` crate, emits to stdout. Captured automatically on Fly/Railway/Render. On a VPS, pipe via `journald` or forward to Loki. |
| Health check | `GET /health` → `{"status":"ok"}`. Use for uptime monitoring and load balancer probes. |
| Active jobs | `AppState.active_jobs` (AtomicUsize) tracks in-flight jobs in memory. Not yet exposed via an endpoint. |

## Recommended additions (in priority order)

### 1. Error tracking — Sentry

Highest ROI. Captures panics, unhandled errors, and failed jobs with full context.
Free tier is sufficient for a small product.

```toml
# web/Cargo.toml
sentry = "0.34"
```

```rust
// web/src/main.rs
let _guard = sentry::init(std::env::var("SENTRY_DSN").unwrap());
```

Add `SENTRY_DSN` to `.env`.

### 2. Uptime monitoring

Point an external service at `GET /health`. Pages you if the server goes down.

- [UptimeRobot](https://uptimerobot.com) — free, 5-minute intervals
- [Better Uptime](https://betterstack.com/uptime) — free tier, nicer UI
- [Cronitor](https://cronitor.io) — also monitors cron jobs

### 3. Business metrics from the DB

No extra infra needed — query Supabase directly:

```sql
-- Jobs completed today
select count(*) from jobs where status = 'completed' and completed_at > now() - interval '1 day';

-- Failure rate (last 7 days)
select status, count(*) from jobs
where created_at > now() - interval '7 days'
group by status;

-- Average processing time
select avg(extract(epoch from (completed_at - started_at))) as avg_seconds
from jobs where status = 'completed';
```

### 4. Prometheus + Grafana

Overkill until you have real load. Add when debugging performance at scale.

## What to alert on

| Condition | Signal |
|---|---|
| Server down | `/health` stops responding |
| Job stuck | `jobs` row with `status = 'processing'` and `started_at < now() - interval '30 minutes'` (the startup recovery handles this on restart) |
| High failure rate | Sentry spike, or `status = 'failed'` count in DB |
| Groq rate limit | `error_message` LIKE '%rate limit%' in failed jobs |
