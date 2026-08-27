-- ============================================================================
-- HR: leave, balances, shifts, attendance, employee documents
-- ============================================================================

create table leave_types (
  key               citext primary key,
  name              text not null,
  paid              boolean not null default true,
  annual_quota_days numeric(5,1) not null default 0
);

create table leave_balances (
  employee_id uuid   not null references employees(id) on delete cascade,
  year        smallint not null,
  type_key    citext not null references leave_types(key),
  allocated   numeric(5,1) not null default 0,
  used        numeric(5,1) not null default 0 check (used >= 0),
  primary key (employee_id, year, type_key),
  check (used <= allocated or allocated = 0)   -- unpaid leave has no quota
);

create table leave_requests (
  id                  uuid primary key default gen_random_uuid(),
  employee_id         uuid   not null references employees(id) on delete cascade,
  type_key            citext not null references leave_types(key),
  start_date          date   not null,
  end_date            date   not null check (end_date >= start_date),
  days                numeric(5,1) not null check (days > 0),
  reason              text,
  status              text not null default 'pending'
                      check (status in ('pending','approved','rejected','cancelled')),
  current_approver_id uuid references employees(id) on delete set null,
  decided_by          uuid references employees(id) on delete set null,
  decided_at          timestamptz,
  decision_note       text,
  created_at          timestamptz not null default now(),
  updated_at          timestamptz not null default now()
);
create index leave_requests_employee_idx on leave_requests (employee_id, start_date);
create index leave_requests_approver_idx on leave_requests (current_approver_id) where status = 'pending';
create trigger leave_requests_updated before update on leave_requests for each row execute function set_updated_at();

-- No two approved/pending requests for the same person may overlap.
create extension if not exists btree_gist;
alter table leave_requests add constraint leave_requests_no_overlap
  exclude using gist (employee_id with =, daterange(start_date, end_date, '[]') with &&)
  where (status in ('pending','approved'));

create table shifts (
  id           uuid primary key default gen_random_uuid(),
  employee_id  uuid not null references employees(id) on delete cascade,
  site         text not null,
  starts_at    timestamptz not null,
  ends_at      timestamptz not null check (ends_at > starts_at),
  role_on_shift text,
  status       text not null default 'scheduled'
               check (status in ('scheduled','completed','missed','cancelled')),
  created_by   uuid references employees(id) on delete set null,
  created_at   timestamptz not null default now()
);
create index shifts_employee_idx on shifts (employee_id, starts_at);

create table attendance (
  id          uuid primary key default gen_random_uuid(),
  employee_id uuid not null references employees(id) on delete cascade,
  shift_id    uuid references shifts(id) on delete set null,
  clock_in    timestamptz not null,
  clock_out   timestamptz check (clock_out is null or clock_out > clock_in),
  late        boolean not null default false,
  source      text not null default 'web' check (source in ('web','mobile','kiosk','import')),
  created_at  timestamptz not null default now()
);
create index attendance_employee_idx on attendance (employee_id, clock_in);

create table employee_documents (
  id          uuid primary key default gen_random_uuid(),
  employee_id uuid not null references employees(id) on delete cascade,
  kind        text not null check (kind in ('contract','id','certificate','payslip','other')),
  title       text not null,
  s3_key      text not null unique,
  mime_type   text not null,
  size_bytes  bigint not null check (size_bytes >= 0),
  uploaded_by uuid references employees(id) on delete set null,
  created_at  timestamptz not null default now()
);
create index employee_documents_employee_idx on employee_documents (employee_id);
