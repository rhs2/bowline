-- ============================================================================
-- Communications: threads, participants, messages, support tickets
-- ============================================================================

create table threads (
  id         uuid primary key default gen_random_uuid(),
  kind       text not null check (kind in ('direct','announcement','ticket')),
  subject    text not null,
  created_by uuid references employees(id) on delete set null,
  audience   jsonb,              -- announcements: {"scope":"company"|"department"|"subtree","ref":uuid}
  created_at timestamptz not null default now(),
  last_message_at timestamptz not null default now()
);
create index threads_last_message_idx on threads (last_message_at desc);

create table thread_participants (
  thread_id    uuid not null references threads(id) on delete cascade,
  employee_id  uuid not null references employees(id) on delete cascade,
  role         text not null default 'recipient' check (role in ('sender','recipient','cc','agent')),
  last_read_at timestamptz,
  archived     boolean not null default false,
  primary key (thread_id, employee_id)
);
create index thread_participants_employee_idx on thread_participants (employee_id, archived);

create table messages (
  id         uuid primary key default gen_random_uuid(),
  thread_id  uuid not null references threads(id) on delete cascade,
  sender_id  uuid references employees(id) on delete set null,
  body       text not null check (length(body) between 1 and 20000),
  importance text not null default 'normal' check (importance in ('low','normal','high')),
  sent_at    timestamptz not null default now()
);
create index messages_thread_idx on messages (thread_id, sent_at);

create sequence ticket_no_seq;

create table support_tickets (
  id            uuid primary key default gen_random_uuid(),
  ticket_no     text not null unique,               -- TKT-NNNNNN
  thread_id     uuid not null unique references threads(id) on delete cascade,
  requester_id  uuid not null references employees(id) on delete cascade,
  category      text not null check (category in ('it','hr','payroll','operations','facilities','other')),
  priority      text not null default 'normal' check (priority in ('low','normal','high','urgent')),
  status        text not null default 'open'
                check (status in ('open','triaged','in_progress','waiting_on_requester','resolved','closed')),
  assignee_id   uuid references employees(id) on delete set null,
  sla_due_at    timestamptz not null,
  first_response_at timestamptz,
  resolved_at   timestamptz,
  closed_at     timestamptz,
  satisfaction  smallint check (satisfaction between 1 and 5),
  created_at    timestamptz not null default now(),
  updated_at    timestamptz not null default now()
);
create index support_tickets_status_idx on support_tickets (status, priority);
create index support_tickets_assignee_idx on support_tickets (assignee_id) where status not in ('resolved','closed');
create index support_tickets_requester_idx on support_tickets (requester_id);
create trigger support_tickets_updated before update on support_tickets for each row execute function set_updated_at();
