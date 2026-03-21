-- Run this in Supabase > SQL Editor

create extension if not exists "uuid-ossp";

-- Jobs table
create table if not exists jobs (
    id uuid primary key default uuid_generate_v4(),
    user_id uuid not null references auth.users(id) on delete cascade,
    status text not null default 'uploaded',
        -- uploaded | preview_processing | preview_ready | preview_failed
        -- paid | processing | completed | failed
    title text,
    source_lang text,
    target_lang text not null,
    cefr_level text not null,
    word_count int,
    chapter_count int,
    price_cents int,
    pipeline text default 'single',

    source_file_path text,
    output_file_path text,
    preview_file_path text,

    stripe_payment_id text unique,
    voucher_code text,
    voucher_id uuid,

    retry_count int not null default 0,
    error_message text,

    created_at timestamptz not null default now(),
    started_at timestamptz,
    completed_at timestamptz
);

-- Users can only see their own jobs
alter table jobs enable row level security;
create policy "users see own jobs" on jobs
    for all using (auth.uid() = user_id);

-- Service role bypasses RLS (used by the server)

-- Vouchers table
create table if not exists vouchers (
    id uuid primary key default uuid_generate_v4(),
    code text not null unique,
    max_pages int not null default 200,
    used_by uuid references auth.users(id),
    used_at timestamptz,
    created_at timestamptz not null default now()
);

alter table vouchers enable row level security;
-- Only service role can read/write vouchers

-- Storage bucket for uploads and outputs
-- Create in Supabase Dashboard > Storage:
--   Bucket name: uploads
--   Private (not public)
