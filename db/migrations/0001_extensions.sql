-- Extensions the platform relies on.
--   ltree   : materialised reporting path (chain of command) with GiST search
--   citext  : case-insensitive emails and codes
--   pgcrypto: gen_random_bytes for reference numbers and tokens
create extension if not exists ltree;
create extension if not exists citext;
create extension if not exists pgcrypto;

-- Shared trigger: keep updated_at current on every row change.
create or replace function set_updated_at() returns trigger
language plpgsql as $$
begin
  new.updated_at := now();
  return new;
end $$;
