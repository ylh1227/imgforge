# ImgForge Remote Deployment

ImgForge remote mode is a production server stack: clients upload assets, submit jobs, workers process them, and results are stored as remote artifacts. The primary deployment uses:

- Postgres for job metadata, assets, review batches, extract results, idempotency, and audit logs.
- Redis Streams for the reliable job queue.
- S3-compatible object storage, such as MinIO, for uploads and artifacts.
- `imgforge-server` with the `server` feature enabled.

SQLite, local disk object storage, and the in-memory queue are fallback backends for tests and single-machine development only. Do not use those fallbacks as the product architecture for shared remote processing.

## Docker Compose

The repository includes a starter stack:

```bash
docker compose -f deploy/docker-compose.yml up --build
```

It starts Postgres, Redis, MinIO, creates the `imgforge` bucket, and runs `imgforge-server`. Replace the default credentials and token before exposing the stack.

## Required Environment

`imgforge-server` reads configuration from environment variables:

- `IMGFORGE_SERVER_BIND`: listen address, for example `0.0.0.0:8787`.
- `IMGFORGE_PUBLIC_BASE`: externally reachable API base URL.
- `IMGFORGE_SERVER_TOKEN`: Bearer token required by clients.
- `IMGFORGE_DEFAULT_WORKSPACE`: default workspace when the client does not send `X-ImgForge-Workspace`.
- `IMGFORGE_RATE_LIMIT_PER_MINUTE`: per token/IP request limit, default `120`.
- `IMGFORGE_DATABASE_URL` or `DATABASE_URL`: Postgres connection string.
- `IMGFORGE_REDIS_URL` or `REDIS_URL`: Redis connection string.
- `IMGFORGE_S3_ENDPOINT`, `IMGFORGE_S3_REGION`, `IMGFORGE_S3_BUCKET`: S3/MinIO target.
- `IMGFORGE_S3_ACCESS_KEY` / `IMGFORGE_S3_SECRET_KEY`: object-store credentials.
- `IMGFORGE_S3_PATH_STYLE=true`: usually required for MinIO.
- `IMGFORGE_INLINE_WORKER`: keep `true` for small deployments; use separate worker processes when the queue grows.

Clients should set `IMGFORGE_REMOTE_ENABLED=true`, `IMGFORGE_REMOTE_BASE_URL`, `IMGFORGE_REMOTE_AUTH_MODE=env_bearer`, and `IMGFORGE_REMOTE_TOKEN`.

## 客户端远程数据源

When `remote.enabled=true` and `remote.base_url` is configured, GUI modules prefer the remote catalog on startup: review/video batches and data-extract results are loaded from the server by default.

The module sidebars expose a `远程` / `本地` data-source toggle. If the server is unreachable or catalog loading fails, the UI falls back to local data and keeps existing local batches visible.

Remote assets downloaded from catalogs, such as thumbnails and data-extract CSV reports, are cached under `~/.imgforge/remote_cache/assets` unless `remote.cache_path` is customized.

## Worker Dependencies

Workers use the same `imgforge-server` binary path today. The container image installs:

- `ffmpeg` / `ffprobe` for video review cover extraction and metadata probing.
- `tesseract-ocr` for OCR-capable data extraction builds.
- CA certificates for object-store and client TLS.

If `ffmpeg` is unavailable or cannot parse an uploaded video, video review still records a metadata placeholder artifact so remote workflows can finish in constrained test environments.

## Workspaces, Auth, Rate Limits, Audit

Mutating routes authorize Bearer tokens, derive the workspace from `X-ImgForge-Workspace` or `IMGFORGE_DEFAULT_WORKSPACE`, and reject mismatched request workspaces. Job submission, upload initialization, and batch creation append audit records when the backing store supports audit persistence.

The server includes a lightweight in-memory token bucket keyed by Bearer token, falling back to forwarded IP headers or anonymous traffic. This protects small deployments and tests; front it with an ingress or API gateway for distributed production rate limiting.

## Fallback Backends

When Postgres, Redis, or S3 variables are omitted, `imgforge-server` falls back to SQLite, an in-memory queue, and disk object storage. This is useful for feature-gated E2E tests and local development, but it is not the recommended remote stack.
