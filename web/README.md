# gunnlod-web

The web server for Gunnlod — upload an EPUB, pick a language and CEFR level, get back a graded-reader version.

## Stack

- **Axum 0.8** — HTTP server
- **Supabase** — auth, Postgres, file storage
- **Stripe** — payments
- **Groq** — LLM translation via `gunnlod-core`
- **htmx** — dashboard polling without a JS framework

## Prerequisites

- Rust (stable)
- A [Supabase](https://supabase.com) project (free tier works)
- A [Stripe](https://stripe.com) account (test mode is fine)
- A [Groq](https://console.groq.com) API key
- [Stripe CLI](https://stripe.com/docs/stripe-cli) for local webhook forwarding

## 1. Supabase setup

### Database

Run `docs/schema.sql` in **Supabase → SQL Editor**. This creates:
- `jobs` table with RLS (users see only their own jobs)
- `vouchers` table

### Storage

In **Supabase → Storage**, create a bucket named `uploads` (private).

### Auth

No extra configuration needed — email/password is enabled by default.

## 2. Environment variables

Copy the example and fill in your values:

```bash
cp .env.example .env
```

| Variable | Where to find it |
|---|---|
| `SUPABASE_URL` | Project Settings → API → Project URL |
| `SUPABASE_ANON_KEY` | Project Settings → API → anon key |
| `SUPABASE_SERVICE_ROLE_KEY` | Project Settings → API → service_role key |
| `SUPABASE_JWT_SECRET` | Project Settings → API → JWT Secret |
| `STRIPE_SECRET_KEY` | Stripe → Developers → API keys |
| `STRIPE_WEBHOOK_SECRET` | see step 4 below |
| `GROQ_API_KEY` | console.groq.com |

## 3. Run the server

```bash
cargo run -p gunnlod-web
```

Server starts on `http://localhost:3000`.

## 4. Stripe webhook (local)

Stripe needs to deliver webhook events to your local server. Use the Stripe CLI:

```bash
stripe listen --forward-to localhost:3000/api/stripe/webhook
```

It will print a webhook signing secret (`whsec_...`) — set that as `STRIPE_WEBHOOK_SECRET` in your `.env`.

## 5. Try it

1. Open `http://localhost:3000`
2. Sign up and confirm your email
3. Log in → Upload an EPUB
4. Wait for the preview (first chapter, processed immediately)
5. Click **Pay & Process** → completes via Stripe test checkout or a voucher code

### Test cards (Stripe)

| Card | Result |
|---|---|
| `4242 4242 4242 4242` | Success |
| `4000 0000 0000 9995` | Decline |

Use any future expiry, any CVC.

## 6. Create a voucher (bypass payment)

Insert a row directly in Supabase → Table Editor → `vouchers`:

```sql
insert into vouchers (code, max_pages) values ('TESTCODE', 999);
```

Then on the pay page, append `?voucher=TESTCODE` to the URL.

## Frontend

No JavaScript framework, no build step. The UI is server-rendered [Askama](https://github.com/djc/askama) templates (Jinja2-style) with [htmx 2](https://htmx.org) loaded from CDN.

htmx is used in two places:

| Feature | How |
|---|---|
| Dashboard polling | Each in-progress job row has `hx-get="/api/jobs/:id/status" hx-trigger="every 5s" hx-swap="outerHTML"` — htmx replaces the row with fresh HTML every 5 seconds until the job reaches a terminal state |
| Upload form | `hx-post="/api/upload" hx-encoding="multipart/form-data"` — htmx submits the form as multipart; the server responds with a redirect to `/dashboard` which htmx follows as a full page navigation |

The upload form also uses `hx-indicator="#spinner"` to show a loading message while the file is uploading.

Auth pages (login, signup) use the Supabase JS SDK directly (vanilla `<script>`) to call Supabase Auth from the browser and store the JWT in a cookie.

## File upload flow

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
   |                           | (uploads/{user}/{id}.epub) |
   |                           |                            |
   |                           | create job row (status: uploaded)
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

The 50 MB request body limit is enforced by `RequestBodyLimitLayer` in the router.

## Routes

| Method | Path | Description |
|---|---|---|
| `GET` | `/` | Landing page |
| `GET` | `/login` | Login |
| `GET` | `/signup` | Sign up |
| `GET` | `/upload` | Upload form (auth required) |
| `POST` | `/api/upload` | Submit EPUB (auth required) |
| `GET` | `/dashboard` | Job list (auth required) |
| `GET` | `/api/jobs/:id/status` | htmx polling endpoint |
| `GET` | `/api/jobs/:id/download` | Download completed EPUB |
| `GET` | `/api/jobs/:id/preview-download` | Download preview EPUB |
| `GET` | `/api/jobs/:id/pay` | Stripe checkout or voucher |
| `POST` | `/api/stripe/webhook` | Stripe event receiver |
| `GET` | `/health` | `{"status":"ok"}` |
