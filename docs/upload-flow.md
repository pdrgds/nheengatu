# File upload flow

## Overview

```
Browser                     Server                      Supabase
   |                           |                            |
   | POST /api/upload           |                            |
   | (multipart/form-data)      |                            |
   |-------------------------->|                            |
   |                           | validate magic bytes       |
   |                           | (PK\x03\x04 = ZIP/EPUB)   |
   |                           |                            |
   |                           | parse EPUB metadata        |
   |                           | (title, word count, etc.)  |
   |                           |                            |
   |                           | upload raw bytes --------->|
   |                           | uploads/{user}/{id}.epub   |
   |                           |                            |
   |                           | create job row             |
   |                           | (status: uploaded)         |
   |                           |                            |
   |                           | spawn process_preview ─────── background:
   |                           |   translate chapter 1          translate via Groq
   |                           |   upload preview EPUB -------->|
   |                           |   update job (preview_ready)   |
   |                           |                            |
   | 303 → /dashboard          |                            |
   |<--------------------------|                            |
   |                           |                            |
   | GET /dashboard             |                            |
   | (shows job as processing) |                            |
   |                           |                            |
   | [every 5s htmx poll]       |                            |
   | GET /api/jobs/:id/status  |                            |
   |-------------------------->|                            |
   | <tr>…preview_ready…</tr>  |                            |
   |<--------------------------|                            |
```

## Step by step

### 1. Validation

`web/src/routes/upload.rs` receives the multipart body and checks:

- File must start with `PK\x03\x04` (the ZIP magic bytes that all EPUBs begin with)
- Body size capped at 50 MB via `RequestBodyLimitLayer` in the router

Invalid files get a `422 Unprocessable Entity` before any storage write happens.

### 2. EPUB parsing

The raw bytes are written to a temp file and passed to `nheengatu_core::epub_parser::parse_epub`. This extracts:

- Title
- Word count (used for pricing)
- Chapter count
- Source language (if present in EPUB metadata)

### 3. Storage

The raw EPUB is uploaded to Supabase Storage under `uploads/{user_id}/{job_id}.epub`. The bucket is private — files are only accessible via the service role key.

### 4. Job creation

A row is inserted into the `jobs` table with `status: uploaded`. Key fields set at this point:

| Field | Value |
|---|---|
| `source_file_path` | `uploads/{user_id}/{job_id}.epub` |
| `word_count` | from EPUB metadata |
| `price_cents` | calculated from word count |
| `status` | `uploaded` |

### 5. Preview processing (background)

`process_preview` is spawned as a Tokio task immediately after the job is created. It:

1. Sets `status: preview_processing`
2. Downloads the source EPUB from storage
3. Runs `run_pipeline` with `chapters: vec![1]` (first chapter only)
4. Uploads the result to `previews/{user_id}/{job_id}-preview.epub`
5. Sets `status: preview_ready` and records `preview_file_path`

If it fails, `status` is set to `preview_failed`. The user can still pay and process the full book.

### 6. Dashboard polling

The dashboard row for the job has `hx-trigger="every 5s"` while the job is in a processing state. Once `preview_ready` (or any terminal state) is returned, the row no longer carries the polling attributes and htmx stops.

## Job status transitions

```
uploaded
  └─► preview_processing
        ├─► preview_ready      (user can pay)
        └─► preview_failed     (user can still pay)

paid
  └─► processing
        ├─► completed          (download available)
        └─► failed             (after 2 retries)
              └─► paid         (re-enqueued for retry via job queue)
```

## Size limits and pricing

| Word count | Price |
|---|---|
| ≤ 30 000 | €3.00 |
| ≤ 100 000 | €5.00 |
| > 100 000 | €7.00+ |

Defined in `web/src/services/pricing.rs`.
