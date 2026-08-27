-- ============================================================================
-- Operations: customers, carriers, sites, vehicles, shipments, legs, events,
-- documents, work orders, inventory
-- ============================================================================

create table customers (
  id                 uuid primary key default gen_random_uuid(),
  code               citext not null unique,
  name               text not null,
  contact_name       text,
  contact_email      citext,
  phone              text,
  billing_address    jsonb not null default '{}'::jsonb,
  credit_limit       numeric(14,2) not null default 0 check (credit_limit >= 0),
  currency           char(3) not null default 'USD',
  status             text not null default 'active' check (status in ('active','on_hold','closed')),
  account_manager_id uuid references employees(id) on delete set null,
  created_at         timestamptz not null default now(),
  updated_at         timestamptz not null default now()
);
create trigger customers_updated before update on customers for each row execute function set_updated_at();

create table carriers (
  id        uuid primary key default gen_random_uuid(),
  code      citext not null unique,
  name      text not null,
  mode      text not null check (mode in ('sea','air','road','rail')),
  scac      text,
  contact   jsonb not null default '{}'::jsonb,
  on_time_rate numeric(5,4) check (on_time_rate between 0 and 1),
  active    boolean not null default true
);

create table sites (
  id         uuid primary key default gen_random_uuid(),
  code       citext not null unique,
  name       text not null,
  kind       text not null check (kind in ('office','warehouse','port','airport','depot')),
  address    jsonb not null default '{}'::jsonb,
  manager_id uuid references employees(id) on delete set null
);

create table vehicles (
  id          uuid primary key default gen_random_uuid(),
  plate       citext not null unique,
  kind        text not null check (kind in ('truck','van','trailer','forklift')),
  capacity_kg numeric(10,2),
  status      text not null default 'available'
              check (status in ('available','in_use','maintenance','retired')),
  home_site_id uuid references sites(id) on delete set null
);

create sequence shipment_ref_seq;

create table shipments (
  id               uuid primary key default gen_random_uuid(),
  reference        text not null unique,                 -- BWL-YYYY-NNNNNN
  customer_id      uuid not null references customers(id) on delete restrict,
  mode             text not null check (mode in ('sea','air','road','rail')),
  incoterm         text check (incoterm in ('EXW','FCA','FOB','CIF','DAP','DDP')),
  origin           jsonb not null,                        -- {city, country, port}
  destination      jsonb not null,
  cargo_description text not null,
  pieces           integer not null default 1 check (pieces > 0),
  weight_kg        numeric(12,2) not null check (weight_kg >= 0),
  volume_cbm       numeric(12,3),
  hazardous        boolean not null default false,
  declared_value   numeric(14,2) not null default 0,
  currency         char(3) not null default 'USD',
  status           text not null default 'draft'
                   check (status in ('draft','booked','picked_up','in_transit','customs',
                                     'out_for_delivery','delivered','cancelled','exception')),
  previous_status  text,                                  -- for exception -> resume
  etd              date,
  eta              date,
  delivered_at     timestamptz,
  delay_risk       numeric(5,4) check (delay_risk between 0 and 1),
  owner_id         uuid references employees(id) on delete set null,
  created_by       uuid references employees(id) on delete set null,
  created_at       timestamptz not null default now(),
  updated_at       timestamptz not null default now()
);
create index shipments_customer_idx on shipments (customer_id);
create index shipments_status_idx on shipments (status);
create index shipments_owner_idx on shipments (owner_id);
create trigger shipments_updated before update on shipments for each row execute function set_updated_at();

create table shipment_legs (
  id                uuid primary key default gen_random_uuid(),
  shipment_id       uuid not null references shipments(id) on delete cascade,
  seq               smallint not null check (seq > 0),
  mode              text not null check (mode in ('sea','air','road','rail')),
  carrier_id        uuid references carriers(id) on delete set null,
  vehicle_id        uuid references vehicles(id) on delete set null,
  driver_id         uuid references employees(id) on delete set null,
  from_location     jsonb not null,
  to_location       jsonb not null,
  planned_departure timestamptz,
  planned_arrival   timestamptz,
  actual_departure  timestamptz,
  actual_arrival    timestamptz,
  status            text not null default 'planned'
                    check (status in ('planned','in_progress','completed','cancelled')),
  unique (shipment_id, seq)
);

create table shipment_events (
  id          uuid primary key default gen_random_uuid(),
  shipment_id uuid not null references shipments(id) on delete cascade,
  leg_id      uuid references shipment_legs(id) on delete set null,
  event_type  text not null check (event_type in ('created','booked','picked_up','departed','arrived',
                                                  'customs_hold','customs_cleared','out_for_delivery',
                                                  'delivered','exception','resumed','cancelled','note')),
  occurred_at timestamptz not null default now(),
  location    text,
  note        text,
  recorded_by uuid references employees(id) on delete set null,
  created_at  timestamptz not null default now()
);
create index shipment_events_shipment_idx on shipment_events (shipment_id, occurred_at);

create table shipment_documents (
  id          uuid primary key default gen_random_uuid(),
  shipment_id uuid not null references shipments(id) on delete cascade,
  kind        text not null check (kind in ('bill_of_lading','air_waybill','commercial_invoice',
                                            'packing_list','customs','proof_of_delivery','other')),
  title       text not null,
  s3_key      text not null unique,
  mime_type   text not null,
  size_bytes  bigint not null check (size_bytes >= 0),
  uploaded_by uuid references employees(id) on delete set null,
  created_at  timestamptz not null default now()
);

create table work_orders (
  id           uuid primary key default gen_random_uuid(),
  shipment_id  uuid references shipments(id) on delete cascade,
  site_id      uuid references sites(id) on delete set null,
  kind         text not null check (kind in ('loading','unloading','pickup','delivery','inspection','inventory')),
  title        text not null,
  instructions text,
  assigned_to  uuid references employees(id) on delete set null,
  assigned_by  uuid references employees(id) on delete set null,
  status       text not null default 'open' check (status in ('open','in_progress','done','blocked','cancelled')),
  due_at       timestamptz,
  started_at   timestamptz,
  completed_at timestamptz,
  notes        text,
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now()
);
create index work_orders_assignee_idx on work_orders (assigned_to, status);
create index work_orders_shipment_idx on work_orders (shipment_id);
create trigger work_orders_updated before update on work_orders for each row execute function set_updated_at();

create table inventory_items (
  id          uuid primary key default gen_random_uuid(),
  site_id     uuid not null references sites(id) on delete restrict,
  shipment_id uuid references shipments(id) on delete set null,
  description text not null,
  quantity    integer not null check (quantity >= 0),
  bin         text,
  received_at timestamptz not null default now(),
  released_at timestamptz
);
create index inventory_items_site_idx on inventory_items (site_id) where released_at is null;
