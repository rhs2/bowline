-- ============================================================================
-- Platform: append-only audit log and the transactional notification outbox
-- ============================================================================

create table audit_log (
  id                bigserial primary key,
  at                timestamptz not null default now(),
  actor_user_id     uuid,
  actor_employee_id uuid,
  action            text not null,          -- e.g. employee.update, invoice.issue
  entity_type       text not null,
  entity_id         uuid,
  before            jsonb,
  after             jsonb,
  ip                inet,
  request_id        text
);
create index audit_log_entity_idx on audit_log (entity_type, entity_id, at desc);
create index audit_log_actor_idx on audit_log (actor_user_id, at desc);
create index audit_log_at_idx on audit_log (at desc);

create or replace function audit_log_immutable() returns trigger
language plpgsql as $$
begin
  raise exception 'audit_log is append-only' using errcode = 'check_violation';
end $$;
create trigger audit_log_no_change before update or delete on audit_log
  for each row execute function audit_log_immutable();

create table notifications (
  id            uuid primary key default gen_random_uuid(),
  recipient_id  uuid not null references employees(id) on delete cascade,
  channel       text not null default 'email' check (channel in ('email','in_app')),
  to_address    citext not null,
  subject       text not null,
  body_text     text not null,
  status        text not null default 'pending' check (status in ('pending','sending','sent','failed')),
  attempts      integer not null default 0,
  next_attempt_at timestamptz not null default now(),
  last_error    text,
  created_at    timestamptz not null default now(),
  sent_at       timestamptz
);
create index notifications_pending_idx on notifications (next_attempt_at) where status in ('pending','sending');
create index notifications_recipient_idx on notifications (recipient_id, created_at desc);

-- Least-privilege grants, applied only when the roles exist (they do under
-- docker compose and Terraform; a bare test database may not have them).
do $$
begin
  if exists (select 1 from pg_roles where rolname = 'bowline_ro') then
    grant usage on schema public to bowline_ro;
    grant select on all tables in schema public to bowline_ro;
  end if;
  if exists (select 1 from pg_roles where rolname = 'bowline_notify') then
    grant usage on schema public to bowline_notify;
    grant select, update on notifications to bowline_notify;
  end if;
end $$;
