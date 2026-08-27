-- ============================================================================
-- Finance: chart of accounts, fiscal periods, double-entry ledger, invoices,
-- payments, vendors and bills, expenses, payroll. Integrity lives here:
--   * journal entries must balance (deferred constraint trigger)
--   * journal lines are immutable once written
--   * nothing posts into a closed period
-- ============================================================================

create table accounts (
  id        uuid primary key default gen_random_uuid(),
  code      text not null unique,
  name      text not null,
  type      text not null check (type in ('asset','liability','equity','revenue','expense')),
  parent_id uuid references accounts(id) on delete restrict,
  active    boolean not null default true
);

create table fiscal_periods (
  id        uuid primary key default gen_random_uuid(),
  year      smallint not null,
  month     smallint not null check (month between 1 and 12),
  starts_on date not null,
  ends_on   date not null check (ends_on >= starts_on),
  status    text not null default 'open' check (status in ('open','closed')),
  closed_by uuid references employees(id) on delete set null,
  closed_at timestamptz,
  unique (year, month)
);

create sequence journal_entry_seq;

create table journal_entries (
  id                   uuid primary key default gen_random_uuid(),
  entry_no             bigint not null unique default nextval('journal_entry_seq'),
  period_id            uuid not null references fiscal_periods(id) on delete restrict,
  entry_date           date not null,
  memo                 text not null,
  source_type          text not null check (source_type in ('invoice','payment','expense','payroll','bill','manual','reversal')),
  source_id            uuid,
  posted_by            uuid references employees(id) on delete set null,
  posted_at            timestamptz not null default now(),
  reverses_entry_id    uuid references journal_entries(id) on delete restrict,
  reversed_by_entry_id uuid references journal_entries(id) on delete restrict
);
create index journal_entries_period_idx on journal_entries (period_id, entry_date);
create index journal_entries_source_idx on journal_entries (source_type, source_id);

create table journal_lines (
  id          uuid primary key default gen_random_uuid(),
  entry_id    uuid not null references journal_entries(id) on delete cascade,
  account_id  uuid not null references accounts(id) on delete restrict,
  debit       numeric(14,2) not null default 0 check (debit >= 0),
  credit      numeric(14,2) not null default 0 check (credit >= 0),
  description text,
  check (not (debit > 0 and credit > 0)),
  check (debit > 0 or credit > 0)
);
create index journal_lines_entry_idx on journal_lines (entry_id);
create index journal_lines_account_idx on journal_lines (account_id);

-- Balance check: a deferred constraint trigger, so it runs at COMMIT after every
-- line of the entry has been written. Callers must insert an entry and its lines
-- inside one transaction (the API always does).
create or replace function journal_entry_balanced() returns trigger
language plpgsql as $$
declare
  d numeric; c numeric; n integer; no bigint;
begin
  select coalesce(sum(debit),0), coalesce(sum(credit),0), count(*)
    into d, c, n from journal_lines where entry_id = new.entry_id;
  select entry_no into no from journal_entries where id = new.entry_id;
  if d <> c or n < 2 then
    raise exception 'journal entry % is not balanced (debit %, credit %, % lines)', no, d, c, n
      using errcode = 'check_violation';
  end if;
  return null;
end $$;
create constraint trigger journal_lines_balanced
  after insert on journal_lines deferrable initially deferred
  for each row execute function journal_entry_balanced();

-- Lines are immutable; corrections are reversing entries.
create or replace function journal_lines_immutable() returns trigger
language plpgsql as $$
begin
  raise exception 'journal lines are immutable; post a reversing entry instead'
    using errcode = 'check_violation';
end $$;
create trigger journal_lines_no_update before update or delete on journal_lines
  for each row execute function journal_lines_immutable();

-- Nothing posts into a closed period. Entries themselves are immutable except for
-- the reversed_by link.
create or replace function journal_entry_guard() returns trigger
language plpgsql as $$
declare
  p_status text;
begin
  if tg_op = 'DELETE' then
    raise exception 'journal entries cannot be deleted' using errcode = 'check_violation';
  end if;
  if tg_op = 'UPDATE' then
    if new.period_id <> old.period_id or new.entry_date <> old.entry_date or new.memo <> old.memo
       or new.source_type <> old.source_type or new.source_id is distinct from old.source_id then
      raise exception 'journal entries are immutable; post a reversing entry instead'
        using errcode = 'check_violation';
    end if;
    return new;
  end if;
  select status into p_status from fiscal_periods where id = new.period_id;
  if p_status is distinct from 'open' then
    raise exception 'fiscal period is closed' using errcode = 'check_violation';
  end if;
  if new.entry_date not between (select starts_on from fiscal_periods where id = new.period_id)
                          and (select ends_on   from fiscal_periods where id = new.period_id) then
    raise exception 'entry date is outside its fiscal period' using errcode = 'check_violation';
  end if;
  return new;
end $$;
create trigger journal_entries_guard before insert or update or delete on journal_entries
  for each row execute function journal_entry_guard();

-- ---------------------------------------------------------------------------
-- Receivables
-- ---------------------------------------------------------------------------
create sequence invoice_no_seq;

create table invoices (
  id               uuid primary key default gen_random_uuid(),
  invoice_no       text not null unique,                   -- INV-YYYY-NNNNNN
  customer_id      uuid not null references customers(id) on delete restrict,
  shipment_id      uuid references shipments(id) on delete set null,
  status           text not null default 'draft'
                   check (status in ('draft','pending_approval','approved','issued','partially_paid','paid','void')),
  issue_date       date,
  due_date         date,
  currency         char(3) not null default 'USD',
  subtotal         numeric(14,2) not null default 0 check (subtotal >= 0),
  tax              numeric(14,2) not null default 0 check (tax >= 0),
  total            numeric(14,2) not null default 0 check (total >= 0),
  amount_paid      numeric(14,2) not null default 0 check (amount_paid >= 0 and amount_paid <= total),
  notes            text,
  pdf_s3_key       text,
  created_by       uuid references employees(id) on delete set null,
  approved_by      uuid references employees(id) on delete set null,
  issued_by        uuid references employees(id) on delete set null,
  journal_entry_id uuid references journal_entries(id) on delete restrict,
  created_at       timestamptz not null default now(),
  updated_at       timestamptz not null default now(),
  check ((status in ('draft','pending_approval','approved')) or (issue_date is not null and due_date is not null))
);
create index invoices_customer_idx on invoices (customer_id, status);
create index invoices_due_idx on invoices (due_date) where status in ('issued','partially_paid');
create trigger invoices_updated before update on invoices for each row execute function set_updated_at();

create table invoice_lines (
  id          uuid primary key default gen_random_uuid(),
  invoice_id  uuid not null references invoices(id) on delete cascade,
  seq         smallint not null,
  description text not null,
  quantity    numeric(12,3) not null check (quantity > 0),
  unit_price  numeric(14,2) not null check (unit_price >= 0),
  tax_rate    numeric(5,4) not null default 0 check (tax_rate between 0 and 1),
  amount      numeric(14,2) not null check (amount >= 0),
  unique (invoice_id, seq)
);

create table payments (
  id               uuid primary key default gen_random_uuid(),
  invoice_id       uuid not null references invoices(id) on delete restrict,
  received_on      date not null,
  amount           numeric(14,2) not null check (amount > 0),
  method           text not null check (method in ('bank_transfer','card','cash','cheque')),
  reference        text,
  recorded_by      uuid references employees(id) on delete set null,
  journal_entry_id uuid references journal_entries(id) on delete restrict,
  created_at       timestamptz not null default now()
);
create index payments_invoice_idx on payments (invoice_id);

-- ---------------------------------------------------------------------------
-- Payables
-- ---------------------------------------------------------------------------
create table vendors (
  id      uuid primary key default gen_random_uuid(),
  code    citext not null unique,
  name    text not null,
  contact jsonb not null default '{}'::jsonb,
  active  boolean not null default true
);

create table vendor_bills (
  id               uuid primary key default gen_random_uuid(),
  vendor_id        uuid not null references vendors(id) on delete restrict,
  bill_no          text not null,
  expense_account_id uuid not null references accounts(id) on delete restrict,
  amount           numeric(14,2) not null check (amount > 0),
  currency         char(3) not null default 'USD',
  received_on      date not null,
  due_on           date not null,
  status           text not null default 'received' check (status in ('received','approved','paid','void')),
  approved_by      uuid references employees(id) on delete set null,
  paid_on          date,
  journal_entry_id uuid references journal_entries(id) on delete restrict,
  payment_entry_id uuid references journal_entries(id) on delete restrict,
  created_at       timestamptz not null default now(),
  unique (vendor_id, bill_no)
);

-- ---------------------------------------------------------------------------
-- Expense claims (two-step approval: manager, then finance)
-- ---------------------------------------------------------------------------
create table expenses (
  id                  uuid primary key default gen_random_uuid(),
  employee_id         uuid not null references employees(id) on delete cascade,
  department_id       uuid not null references departments(id) on delete restrict,
  category            text not null check (category in ('travel','fuel','meals','supplies','equipment','other')),
  expense_account_id  uuid not null references accounts(id) on delete restrict,
  amount              numeric(14,2) not null check (amount > 0),
  currency            char(3) not null default 'USD',
  incurred_on         date not null,
  description         text not null,
  receipt_s3_key      text,
  status              text not null default 'submitted'
                      check (status in ('submitted','manager_approved','finance_approved','rejected','paid')),
  manager_approved_by uuid references employees(id) on delete set null,
  finance_approved_by uuid references employees(id) on delete set null,
  rejected_by         uuid references employees(id) on delete set null,
  rejection_note      text,
  journal_entry_id    uuid references journal_entries(id) on delete restrict,
  created_at          timestamptz not null default now(),
  updated_at          timestamptz not null default now()
);
create index expenses_employee_idx on expenses (employee_id, status);
create trigger expenses_updated before update on expenses for each row execute function set_updated_at();

-- ---------------------------------------------------------------------------
-- Payroll
-- ---------------------------------------------------------------------------
create table payroll_runs (
  id               uuid primary key default gen_random_uuid(),
  period_id        uuid not null unique references fiscal_periods(id) on delete restrict,
  status           text not null default 'draft' check (status in ('draft','approved','posted')),
  total_gross      numeric(14,2) not null default 0,
  total_deductions numeric(14,2) not null default 0,
  total_net        numeric(14,2) not null default 0,
  created_by       uuid references employees(id) on delete set null,
  approved_by      uuid references employees(id) on delete set null,
  approved_at      timestamptz,
  posted_at        timestamptz,
  journal_entry_id uuid references journal_entries(id) on delete restrict,
  created_at       timestamptz not null default now()
);

create table payroll_items (
  id          uuid primary key default gen_random_uuid(),
  run_id      uuid not null references payroll_runs(id) on delete cascade,
  employee_id uuid not null references employees(id) on delete restrict,
  gross       numeric(12,2) not null check (gross >= 0),
  deductions  numeric(12,2) not null default 0 check (deductions >= 0),
  net         numeric(12,2) not null check (net >= 0),
  unique (run_id, employee_id)
);

-- ---------------------------------------------------------------------------
-- Report views
-- ---------------------------------------------------------------------------
create view trial_balance as
  select a.code, a.name, a.type,
         coalesce(sum(l.debit),0)  as debit,
         coalesce(sum(l.credit),0) as credit,
         coalesce(sum(l.debit),0) - coalesce(sum(l.credit),0) as balance
    from accounts a
    left join journal_lines l on l.account_id = a.id
   group by a.id, a.code, a.name, a.type
   order by a.code;

create view ar_aging as
  select i.id as invoice_id, i.invoice_no, i.customer_id, c.name as customer_name,
         i.due_date, i.total - i.amount_paid as outstanding,
         greatest(current_date - i.due_date, 0) as days_overdue,
         case when current_date <= i.due_date then 'current'
              when current_date - i.due_date <= 30 then '1-30'
              when current_date - i.due_date <= 60 then '31-60'
              when current_date - i.due_date <= 90 then '61-90'
              else '90+' end as bucket
    from invoices i
    join customers c on c.id = i.customer_id
   where i.status in ('issued','partially_paid');

create view profit_and_loss as
  select p.year, p.month, a.type, a.code, a.name,
         case when a.type = 'revenue' then coalesce(sum(l.credit),0) - coalesce(sum(l.debit),0)
              else coalesce(sum(l.debit),0) - coalesce(sum(l.credit),0) end as amount
    from journal_entries e
    join fiscal_periods p on p.id = e.period_id
    join journal_lines l on l.entry_id = e.id
    join accounts a on a.id = l.account_id
   where a.type in ('revenue','expense')
   group by p.year, p.month, a.type, a.code, a.name;
