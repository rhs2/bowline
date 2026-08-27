//! Loads the Bowline Logistics demo company into the database.
//!
//! ```text
//! cargo run --bin seed            # seed an empty database, no-op if already seeded
//! cargo run --bin seed -- --reset # wipe the business tables first, then seed
//! ```
//!
//! The whole company is built in memory first (deterministically, from
//! `SEED_RANDOM_SEED`) and then written in batched inserts inside a single
//! transaction, so seeding either lands completely or not at all. Nothing goes
//! through the HTTP API: the seeder speaks to the schema directly and respects the
//! same integrity rules the API does.
//!
//! Rules worth remembering while reading this file:
//!
//! * `employees.path` is maintained by a trigger, so it is never inserted. Employees
//!   are written one level at a time (CEO first) so every manager already exists.
//! * Exactly one employee may have no manager.
//! * A journal entry and all of its lines must be written in one transaction: the
//!   balance check is a deferred constraint trigger that runs at commit.
//! * Journal lines are immutable and the audit log is append-only.
//! * Leave requests for the same employee may not overlap while pending or approved.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, Months, NaiveDate, Utc, Weekday};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use bowline_api::auth::password;
use bowline_api::{db, telemetry, Config};

/// The document backfill, compiled into the seeder as well as into its own binary, so
/// that seeding and `cargo run --bin backfill-documents` run exactly the same code.
/// Its `main` is dead here; the seeder only calls [`backfill::run`].
#[path = "backfill_documents.rs"]
#[allow(dead_code)]
mod backfill;

const USAGE: &str = "\
bowline seed: load the demo company (departments, 260 employees, customers,
shipments, ledger, tickets) into DATABASE_URL.

Usage:
  seed [--reset] [--help]

Options:
  --reset   truncate every business table (reference data is kept) before seeding
  --help    print this text

Environment:
  DATABASE_URL, SEED_PASSWORD, SEED_SKIP_PASSWORD_CHANGE, SEED_RANDOM_SEED";

const EMAIL_DOMAIN: &str = "bowline.example";
/// Rows per INSERT statement. Every table here stays well inside the 65535 bind
/// parameter limit of the extended query protocol at this width.
const CHUNK: usize = 500;

// ---------------------------------------------------------------------------
// The company on paper
// ---------------------------------------------------------------------------

/// Divisions: code, name, C-suite title, position code, well-known login.
const DIVISIONS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "OPS",
        "Operations",
        "Chief Operating Officer",
        "EXE-COO",
        "coo",
    ),
    (
        "FIN",
        "Finance",
        "Chief Financial Officer",
        "EXE-CFO",
        "cfo",
    ),
    (
        "PPL",
        "People",
        "Chief Human Resources Officer",
        "EXE-CHRO",
        "chro",
    ),
    (
        "TECH",
        "Technology",
        "Chief Technology Officer",
        "EXE-CTO",
        "cto",
    ),
    (
        "COMM",
        "Commercial",
        "Chief Commercial Officer",
        "EXE-CCO",
        "cco",
    ),
];

/// A leaf department and the shape of its team.
struct Unit {
    code: &'static str,
    name: &'static str,
    division: &'static str,
    site: &'static str,
    headcount: usize,
    managers: usize,
    supervisors: usize,
    /// Share of the rank and file that is level 7 ground staff.
    ground_share: f64,
    /// Titles for levels 3, 4, 5, 6 and 7 ("" when the unit has no ground staff).
    titles: [&'static str; 5],
    /// Well-known login local part for the first holder of each rank ("" for none).
    logins: [&'static str; 5],
}

const UNITS: &[Unit] = &[
    Unit {
        code: "SEA",
        name: "Sea Freight",
        division: "OPS",
        site: "PORT",
        headcount: 22,
        managers: 2,
        supervisors: 4,
        ground_share: 0.40,
        titles: [
            "Director of Sea Freight",
            "Sea Freight Manager",
            "Sea Freight Supervisor",
            "Freight Coordinator",
            "Cargo Handler",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "AIR",
        name: "Air Freight",
        division: "OPS",
        site: "AIRP",
        headcount: 18,
        managers: 2,
        supervisors: 3,
        ground_share: 0.40,
        titles: [
            "Director of Air Freight",
            "Air Freight Manager",
            "Air Export Supervisor",
            "Air Freight Coordinator",
            "Cargo Handler",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "ROAD",
        name: "Road and Last Mile",
        division: "OPS",
        site: "HUB",
        headcount: 28,
        managers: 2,
        supervisors: 4,
        ground_share: 0.45,
        titles: [
            "Director of Road and Last Mile",
            "Road Operations Manager",
            "Dispatch Supervisor",
            "Dispatch Coordinator",
            "Delivery Driver",
        ],
        logins: ["", "", "", "dispatcher", ""],
    },
    Unit {
        code: "WH",
        name: "Warehousing",
        division: "OPS",
        site: "BOND",
        headcount: 40,
        managers: 2,
        supervisors: 5,
        ground_share: 0.70,
        titles: [
            "Director of Warehousing",
            "Warehouse Manager",
            "Dock Supervisor",
            "Inventory Controller",
            "Dock Worker",
        ],
        logins: [
            "",
            "manager.warehouse",
            "supervisor.dock",
            "",
            "dock.worker",
        ],
    },
    Unit {
        code: "FLEET",
        name: "Fleet and Drivers",
        division: "OPS",
        site: "HUB",
        headcount: 36,
        managers: 2,
        supervisors: 4,
        ground_share: 0.80,
        titles: [
            "Director of Fleet",
            "Fleet Manager",
            "Transport Supervisor",
            "Fleet Coordinator",
            "Driver",
        ],
        logins: ["", "", "", "", "driver"],
    },
    Unit {
        code: "CUST",
        name: "Customs and Compliance",
        division: "OPS",
        site: "PORT",
        headcount: 12,
        managers: 1,
        supervisors: 2,
        ground_share: 0.0,
        titles: [
            "Director of Customs and Compliance",
            "Customs Manager",
            "Customs Supervisor",
            "Customs Broker",
            "",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "ACC",
        name: "Accounting",
        division: "FIN",
        site: "HQ",
        headcount: 10,
        managers: 1,
        supervisors: 1,
        ground_share: 0.0,
        titles: [
            "Director of Finance",
            "Accounting Manager",
            "Accounting Supervisor",
            "Accountant",
            "",
        ],
        logins: ["director.finance", "", "", "accountant", ""],
    },
    Unit {
        code: "AR",
        name: "Billing and AR",
        division: "FIN",
        site: "HQ",
        headcount: 10,
        managers: 1,
        supervisors: 2,
        ground_share: 0.0,
        titles: [
            "Head of Revenue Operations",
            "Billing Manager",
            "Billing Supervisor",
            "Billing Specialist",
            "",
        ],
        logins: ["", "manager.billing", "", "", ""],
    },
    Unit {
        code: "PAY",
        name: "Payroll",
        division: "FIN",
        site: "HQ",
        headcount: 6,
        managers: 1,
        supervisors: 1,
        ground_share: 0.0,
        titles: [
            "Head of Payroll",
            "Payroll Manager",
            "Payroll Supervisor",
            "Payroll Specialist",
            "",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "AP",
        name: "Procurement and AP",
        division: "FIN",
        site: "HQ",
        headcount: 6,
        managers: 1,
        supervisors: 1,
        ground_share: 0.0,
        titles: [
            "Head of Procurement",
            "Procurement Manager",
            "Accounts Payable Supervisor",
            "Procurement Specialist",
            "",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "TA",
        name: "Talent Acquisition",
        division: "PPL",
        site: "HQ",
        headcount: 6,
        managers: 1,
        supervisors: 1,
        ground_share: 0.0,
        titles: [
            "Head of Talent Acquisition",
            "Talent Acquisition Manager",
            "Recruiting Team Lead",
            "Recruiter",
            "",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "PO",
        name: "People Operations",
        division: "PPL",
        site: "HQ",
        headcount: 8,
        managers: 1,
        supervisors: 1,
        ground_share: 0.0,
        titles: [
            "Director of People Operations",
            "HR Manager",
            "HR Team Lead",
            "People Operations Specialist",
            "",
        ],
        logins: ["", "", "", "hr.admin", ""],
    },
    Unit {
        code: "PLAT",
        name: "Platform Engineering",
        division: "TECH",
        site: "HQ",
        headcount: 11,
        managers: 1,
        supervisors: 2,
        ground_share: 0.0,
        titles: [
            "Director of Engineering",
            "Platform Engineering Manager",
            "Engineering Team Lead",
            "Platform Engineer",
            "",
        ],
        logins: ["", "it.admin", "", "", ""],
    },
    Unit {
        code: "SD",
        name: "Service Desk",
        division: "TECH",
        site: "HQ",
        headcount: 9,
        managers: 1,
        supervisors: 1,
        ground_share: 0.0,
        titles: [
            "Director of IT Services",
            "Service Desk Manager",
            "Support Team Lead",
            "Support Agent",
            "",
        ],
        logins: ["", "", "", "support.agent", ""],
    },
    Unit {
        code: "SALES",
        name: "Sales",
        division: "COMM",
        site: "HQ",
        headcount: 18,
        managers: 2,
        supervisors: 3,
        ground_share: 0.0,
        titles: [
            "Director of Sales",
            "Sales Manager",
            "Sales Team Lead",
            "Account Executive",
            "",
        ],
        logins: ["", "", "", "", ""],
    },
    Unit {
        code: "CS",
        name: "Customer Service",
        division: "COMM",
        site: "HQ",
        headcount: 14,
        managers: 1,
        supervisors: 2,
        ground_share: 0.0,
        titles: [
            "Director of Customer Service",
            "Customer Service Manager",
            "Customer Service Team Lead",
            "Customer Service Representative",
            "",
        ],
        logins: ["", "", "", "", ""],
    },
];

/// Sites: code, name, kind, city, country.
const SITES: &[(&str, &str, &str, &str, &str)] = &[
    ("HQ", "Bowline House", "office", "Rotterdam", "NL"),
    ("PORT", "Harbour Terminal Depot", "port", "Rotterdam", "NL"),
    ("AIRP", "Airport Cargo Centre", "airport", "Amsterdam", "NL"),
    ("HUB", "Inland Road Hub", "depot", "Utrecht", "NL"),
    ("BOND", "Bonded Warehouse", "warehouse", "Rotterdam", "NL"),
];

/// Carriers: code, name, mode, scac, on-time rate in ten-thousandths.
const CARRIERS: &[(&str, &str, &str, &str, i64)] = &[
    ("MRDN", "Meridian Container Line", "sea", "MRDU", 9120),
    ("BLUW", "Blue Wave Shipping", "sea", "BLWV", 8840),
    ("SKYF", "Skyfreight Cargo", "air", "SKYF", 9410),
    ("AERL", "Aerolink Air Cargo", "air", "AELK", 9230),
    ("NRTH", "Northgate Haulage", "road", "NGHL", 8760),
    ("CRFX", "Continental Rail Freight", "rail", "CRFX", 9050),
];

const FIRST_NAMES: &[&str] = &[
    "Adam", "Adele", "Ahmed", "Aisha", "Alan", "Alice", "Amara", "Andre", "Anika", "Anton",
    "Beatrix", "Bram", "Carla", "Carlos", "Cato", "Chloe", "Daan", "Dalia", "Damian", "Diana",
    "Edwin", "Elena", "Elias", "Elise", "Emeka", "Emma", "Erik", "Esther", "Fatima", "Felix",
    "Fiona", "Frank", "Gerda", "Gijs", "Grace", "Hanna", "Hugo", "Ibrahim", "Ines", "Iris",
    "Jasper", "Joana", "Jonas", "Julia", "Kaito", "Karim", "Katja", "Kwame", "Lars", "Laura",
    "Leon", "Lidia", "Linh", "Lucas", "Maja", "Marco", "Marta", "Mateo", "Mila", "Nadia", "Niels",
    "Nina", "Noor", "Olivier", "Omar", "Paula", "Pieter", "Priya", "Rafael", "Rania", "Rebecca",
    "Ruben", "Sanne", "Sofia", "Stefan", "Tessa", "Thomas", "Tobias", "Vera", "Victor", "Wei",
    "Yara", "Yusuf", "Zara", "Zoltan",
];

const LAST_NAMES: &[&str] = &[
    "Abbas",
    "Adeyemi",
    "Albers",
    "Almeida",
    "Andersen",
    "Bakker",
    "Barros",
    "Bauer",
    "Beckman",
    "Berger",
    "Blom",
    "Bosman",
    "Brandt",
    "Bruins",
    "Caldeira",
    "Cardoso",
    "Chen",
    "Costa",
    "Dekker",
    "Delgado",
    "Doyle",
    "Draper",
    "Duarte",
    "Egan",
    "Engel",
    "Faber",
    "Ferreira",
    "Fischer",
    "Fonseca",
    "Garcia",
    "Gerritsen",
    "Gomes",
    "Haddad",
    "Hansen",
    "Hartman",
    "Heikkinen",
    "Hoffmann",
    "Ibrahim",
    "Ivanov",
    "Jansen",
    "Jimenez",
    "Kaminski",
    "Keller",
    "Khan",
    "Kimura",
    "Kovac",
    "Kruger",
    "Laurent",
    "Lehmann",
    "Lindqvist",
    "Maas",
    "Marchetti",
    "Mbeki",
    "Meijer",
    "Mendes",
    "Moreau",
    "Mueller",
    "Nakamura",
    "Navarro",
    "Nguyen",
    "Nowak",
    "Okafor",
    "Olsen",
    "Pereira",
    "Petrov",
    "Pires",
    "Quinn",
    "Rahman",
    "Reinders",
    "Ribeiro",
    "Rossi",
    "Sadiq",
    "Santos",
    "Schneider",
    "Silva",
    "Smit",
    "Sorensen",
    "Tanaka",
    "Torres",
    "Ubelhart",
    "Vandenberg",
    "Varga",
    "Verhoeven",
    "Vermeulen",
    "Visser",
    "Wagner",
    "Walsh",
    "Weber",
    "Yilmaz",
    "Zieliński",
    "Zwart",
];

const COMPANY_HEADS: &[&str] = &[
    "Northwind",
    "Harbourline",
    "Vantage",
    "Kestrel",
    "Meridian",
    "Blue Ridge",
    "Delta Grove",
    "Ironwood",
    "Silverpine",
    "Clearwater",
    "Redstone",
    "Amberfield",
    "Lakeshore",
    "Granite",
    "Fairmount",
    "Copperbeech",
    "Highgate",
    "Sable",
    "Windrose",
    "Oakhaven",
];

const COMPANY_TAILS: &[&str] = &[
    "Industries",
    "Trading",
    "Foods",
    "Components",
    "Textiles",
    "Chemicals",
    "Electronics",
    "Pharma",
    "Machinery",
    "Agri",
    "Retail Group",
    "Beverages",
];

const VENDOR_NAMES: &[&str] = &[
    "Portside Fuel Supply",
    "Delta Container Repair",
    "Harbour Crane Services",
    "Northline Tyre and Fleet",
    "Bonded Storage Partners",
    "Clearview Customs Agency",
    "Rotterdam Office Supplies",
    "Skyline Facility Care",
    "Vantage IT Hardware",
    "Meridian Insurance Brokers",
    "Trainline Rail Logistics",
    "Coastal Packaging Works",
];

/// Sea ports: city, country, UN/LOCODE.
const SEA_PORTS: &[(&str, &str, &str)] = &[
    ("Rotterdam", "NL", "NLRTM"),
    ("Singapore", "SG", "SGSIN"),
    ("Shanghai", "CN", "CNSHA"),
    ("Hamburg", "DE", "DEHAM"),
    ("Los Angeles", "US", "USLAX"),
    ("Santos", "BR", "BRSSZ"),
    ("Durban", "ZA", "ZADUR"),
    ("Felixstowe", "GB", "GBFXT"),
    ("Busan", "KR", "KRPUS"),
    ("Jebel Ali", "AE", "AEJEA"),
];

const AIRPORTS: &[(&str, &str, &str)] = &[
    ("Amsterdam", "NL", "AMS"),
    ("Frankfurt", "DE", "FRA"),
    ("Hong Kong", "HK", "HKG"),
    ("Chicago", "US", "ORD"),
    ("Dubai", "AE", "DXB"),
    ("Istanbul", "TR", "IST"),
    ("Sao Paulo", "BR", "GRU"),
    ("Nairobi", "KE", "NBO"),
];

const INLAND: &[(&str, &str, &str)] = &[
    ("Utrecht", "NL", "UTR"),
    ("Antwerp", "BE", "ANR"),
    ("Lille", "FR", "LIL"),
    ("Munich", "DE", "MUC"),
    ("Milan", "IT", "MIL"),
    ("Manchester", "GB", "MAN"),
    ("Lyon", "FR", "LYS"),
    ("Basel", "CH", "BSL"),
];

const CARGO: &[&str] = &[
    "Palletised consumer electronics",
    "Refrigerated fruit",
    "Automotive spare parts",
    "Industrial pumps",
    "Textile rolls",
    "Packaged foodstuffs",
    "Laboratory equipment",
    "Furniture, flat packed",
    "Steel fittings",
    "Bagged polymer granulate",
    "Paint and coatings",
    "Bottled beverages",
];

/// Shipment status mix; the sum is the shipment count.
const SHIPMENT_MIX: &[(&str, usize)] = &[
    ("delivered", 150),
    ("in_transit", 38),
    ("booked", 26),
    ("picked_up", 18),
    ("customs", 14),
    ("out_for_delivery", 14),
    ("draft", 16),
    ("exception", 14),
    ("cancelled", 10),
];

const INVOICE_MIX: &[(&str, usize)] = &[
    ("paid", 22),
    ("issued", 30),
    ("partially_paid", 12),
    ("draft", 10),
    ("pending_approval", 6),
    ("approved", 6),
    ("void", 4),
];

const EXPENSE_MIX: &[(&str, usize)] = &[
    ("paid", 25),
    ("submitted", 15),
    ("manager_approved", 12),
    ("finance_approved", 10),
    ("rejected", 8),
];

const BILL_MIX: &[(&str, usize)] = &[
    ("paid", 12),
    ("approved", 12),
    ("received", 10),
    ("void", 1),
];

const TICKET_MIX: &[(&str, usize)] = &[
    ("resolved", 14),
    ("closed", 12),
    ("in_progress", 10),
    ("open", 8),
    ("triaged", 8),
    ("waiting_on_requester", 8),
];

const WORK_ORDER_MIX: &[(&str, usize)] = &[
    ("done", 200),
    ("open", 60),
    ("in_progress", 40),
    ("blocked", 30),
    ("cancelled", 30),
];

/// Ticket seeds: category, subject, opening message.
const TICKET_SEEDS: &[(&str, &str, &str)] = &[
    (
        "it",
        "Laptop will not connect to the depot wifi",
        "Since this morning my laptop drops off the depot network every few minutes. Rebooting does not help.",
    ),
    (
        "it",
        "Password reset for the scanning app",
        "The handheld scanner app rejects my password after the last update. Could you reset it?",
    ),
    (
        "hr",
        "Question about my remaining annual leave",
        "My balance shows fewer days than I expected after the summer break. Could someone check it?",
    ),
    (
        "hr",
        "Reference letter for a mortgage application",
        "My bank asks for a letter confirming my contract and salary. Where do I request one?",
    ),
    (
        "payroll",
        "Overtime hours missing from last payslip",
        "I worked two late shifts in the last period and they do not appear on my payslip.",
    ),
    (
        "payroll",
        "Change of bank account details",
        "I moved to a new bank and need to update the account my salary is paid into.",
    ),
    (
        "operations",
        "Dock door 4 barrier stuck open",
        "The barrier at dock door 4 does not close. We are routing traffic to door 5 for now.",
    ),
    (
        "operations",
        "Scanner not reading damaged labels",
        "A pallet arrived with smudged labels and the scanner refuses them. Manual entry is slow.",
    ),
    (
        "facilities",
        "Heating out in the bonded warehouse office",
        "The office next to the bonded area has had no heating since the weekend.",
    ),
    (
        "facilities",
        "Broken chair in the dispatch room",
        "One of the chairs in the dispatch room has a cracked base and is not safe to sit on.",
    ),
    (
        "other",
        "Access badge for the new starter",
        "Our new coordinator starts on Monday and needs a badge for the port gate.",
    ),
    (
        "other",
        "Request for a second monitor",
        "Working two systems side by side on one screen is slowing the team down.",
    ),
];

const AGENT_REPLIES: &[&str] = &[
    "Thanks for the report. I have picked this up and will come back to you today.",
    "I can reproduce this. A fix is on the way, I will keep this ticket updated.",
    "Could you confirm which site and shift this happened on so I can narrow it down?",
    "This is now with the vendor. I will chase them tomorrow morning.",
    "Sorted on our side. Please confirm it works for you and I will close the ticket.",
];

const REQUESTER_REPLIES: &[&str] = &[
    "Thank you, that is much appreciated.",
    "Still happening this morning, though less often than yesterday.",
    "That worked, thanks for the quick turnaround.",
    "Confirmed, it was the depot network. All good now.",
];

const DIRECT_SUBJECTS: &[(&str, &str)] = &[
    (
        "Shift cover for next week",
        "Could you cover the late shift on Thursday? I can swap you for the Monday early if that helps.",
    ),
    (
        "Customer escalation on the Hamburg lane",
        "The customer called about transit times again. Can we talk through the plan before I reply?",
    ),
    (
        "Quarterly objectives",
        "I have drafted the objectives for the quarter. Have a read and let me know what is missing.",
    ),
    (
        "Fuel card for the new van",
        "The new van is on the road from Monday. Please arrange a fuel card before then.",
    ),
    (
        "Training day for the new handlers",
        "Two new handlers start next month. Can we book the forklift refresher for the same week?",
    ),
    (
        "Invoice query from the customer",
        "They dispute the storage line on last month's invoice. Could you check the dates for me?",
    ),
    (
        "Holiday plan",
        "I am planning two weeks in August. Sending the request through the system today.",
    ),
];

const MANAGER_REPLIES: &[&str] = &[
    "Thanks, that works. Go ahead and I will update the roster.",
    "Good summary. Let us cover the rest in the team meeting.",
    "Approved from my side. Flag it to me again if anything slips.",
    "Noted. I will pick this up with the department head this week.",
];

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

struct DeptRow {
    id: Uuid,
    code: &'static str,
    name: &'static str,
    parent_id: Option<Uuid>,
    cost_center: String,
}

struct PositionRow {
    id: Uuid,
    code: String,
    title: &'static str,
    level: i16,
    department_id: Uuid,
    people_manager: bool,
}

struct Emp {
    id: Uuid,
    employee_no: String,
    first_name: String,
    last_name: String,
    email: String,
    phone: String,
    position_id: Uuid,
    department_id: Uuid,
    manager_id: Option<Uuid>,
    status: &'static str,
    employment_type: &'static str,
    hire_date: NaiveDate,
    termination_date: Option<NaiveDate>,
    site: &'static str,
    pay_grade: String,
    base_salary: Decimal,
    level: i16,
    title: &'static str,
    dept_code: &'static str,
    roles: Vec<&'static str>,
    well_known: bool,
}

impl Emp {
    fn active(&self) -> bool {
        self.status != "terminated"
    }
}

struct UserRow {
    id: Uuid,
    employee_id: Uuid,
    email: String,
    status: &'static str,
    must_change_password: bool,
    last_login_at: Option<DateTime<Utc>>,
}

struct SiteRow {
    id: Uuid,
    code: &'static str,
    name: &'static str,
    kind: &'static str,
    address: serde_json::Value,
    manager_id: Option<Uuid>,
}

struct CarrierRow {
    id: Uuid,
    code: &'static str,
    name: &'static str,
    mode: &'static str,
    scac: &'static str,
    contact: serde_json::Value,
    on_time_rate: Decimal,
}

struct VehicleRow {
    id: Uuid,
    plate: String,
    kind: &'static str,
    capacity_kg: Decimal,
    status: &'static str,
    home_site_id: Uuid,
}

struct CustomerRow {
    id: Uuid,
    code: String,
    name: String,
    contact_name: String,
    contact_email: String,
    phone: String,
    billing_address: serde_json::Value,
    credit_limit: Decimal,
    status: &'static str,
    account_manager_id: Option<Uuid>,
}

struct VendorRow {
    id: Uuid,
    code: String,
    name: &'static str,
    contact: serde_json::Value,
}

struct ShipmentRow {
    id: Uuid,
    reference: String,
    customer_id: Uuid,
    mode: &'static str,
    incoterm: &'static str,
    origin: serde_json::Value,
    destination: serde_json::Value,
    cargo_description: &'static str,
    pieces: i32,
    weight_kg: Decimal,
    volume_cbm: Decimal,
    hazardous: bool,
    declared_value: Decimal,
    status: &'static str,
    previous_status: Option<&'static str>,
    etd: NaiveDate,
    eta: NaiveDate,
    delivered_at: Option<DateTime<Utc>>,
    delay_risk: Decimal,
    owner_id: Uuid,
}

struct LegRow {
    id: Uuid,
    shipment_id: Uuid,
    seq: i16,
    mode: &'static str,
    carrier_id: Option<Uuid>,
    vehicle_id: Option<Uuid>,
    driver_id: Option<Uuid>,
    from_location: serde_json::Value,
    to_location: serde_json::Value,
    planned_departure: DateTime<Utc>,
    planned_arrival: DateTime<Utc>,
    actual_departure: Option<DateTime<Utc>>,
    actual_arrival: Option<DateTime<Utc>>,
    status: &'static str,
}

struct EventRow {
    id: Uuid,
    shipment_id: Uuid,
    event_type: &'static str,
    occurred_at: DateTime<Utc>,
    location: String,
    note: Option<String>,
    recorded_by: Uuid,
}

struct DocRow {
    id: Uuid,
    parent_id: Uuid,
    kind: &'static str,
    title: String,
    s3_key: String,
    mime_type: &'static str,
    size_bytes: i64,
    uploaded_by: Uuid,
}

struct WorkOrderRow {
    id: Uuid,
    shipment_id: Option<Uuid>,
    site_id: Uuid,
    kind: &'static str,
    title: String,
    instructions: String,
    assigned_to: Uuid,
    assigned_by: Uuid,
    status: &'static str,
    due_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    notes: Option<&'static str>,
}

struct InventoryRow {
    id: Uuid,
    site_id: Uuid,
    shipment_id: Option<Uuid>,
    description: String,
    quantity: i32,
    bin: String,
    received_at: DateTime<Utc>,
    released_at: Option<DateTime<Utc>>,
}

struct LeaveBalanceRow {
    employee_id: Uuid,
    year: i16,
    type_key: &'static str,
    allocated: Decimal,
    used: Decimal,
}

struct LeaveRequestRow {
    id: Uuid,
    employee_id: Uuid,
    type_key: &'static str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    days: Decimal,
    reason: &'static str,
    status: &'static str,
    current_approver_id: Option<Uuid>,
    decided_by: Option<Uuid>,
    decided_at: Option<DateTime<Utc>>,
    decision_note: Option<&'static str>,
}

struct ShiftRow {
    id: Uuid,
    employee_id: Uuid,
    site: &'static str,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    role_on_shift: &'static str,
    status: &'static str,
    created_by: Uuid,
}

struct AttendanceRow {
    id: Uuid,
    employee_id: Uuid,
    shift_id: Uuid,
    clock_in: DateTime<Utc>,
    clock_out: Option<DateTime<Utc>>,
    late: bool,
    source: &'static str,
}

struct EntryRow {
    id: Uuid,
    period_id: Uuid,
    entry_date: NaiveDate,
    memo: String,
    source_type: &'static str,
    source_id: Option<Uuid>,
    posted_by: Uuid,
    posted_at: DateTime<Utc>,
    reverses_entry_id: Option<Uuid>,
}

struct LineRow {
    id: Uuid,
    entry_id: Uuid,
    account_id: Uuid,
    debit: Decimal,
    credit: Decimal,
    description: String,
}

struct InvoiceRow {
    id: Uuid,
    invoice_no: String,
    customer_id: Uuid,
    shipment_id: Option<Uuid>,
    status: &'static str,
    issue_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    subtotal: Decimal,
    tax: Decimal,
    total: Decimal,
    amount_paid: Decimal,
    notes: Option<String>,
    pdf_s3_key: Option<String>,
    created_by: Uuid,
    approved_by: Option<Uuid>,
    issued_by: Option<Uuid>,
    journal_entry_id: Option<Uuid>,
}

struct InvoiceLineRow {
    id: Uuid,
    invoice_id: Uuid,
    seq: i16,
    description: &'static str,
    quantity: Decimal,
    unit_price: Decimal,
    tax_rate: Decimal,
    amount: Decimal,
}

struct PaymentRow {
    id: Uuid,
    invoice_id: Uuid,
    received_on: NaiveDate,
    amount: Decimal,
    method: &'static str,
    reference: String,
    recorded_by: Uuid,
    journal_entry_id: Uuid,
}

struct BillRow {
    id: Uuid,
    vendor_id: Uuid,
    bill_no: String,
    expense_account_id: Uuid,
    amount: Decimal,
    received_on: NaiveDate,
    due_on: NaiveDate,
    status: &'static str,
    approved_by: Option<Uuid>,
    paid_on: Option<NaiveDate>,
    journal_entry_id: Option<Uuid>,
    payment_entry_id: Option<Uuid>,
}

struct ExpenseRow {
    id: Uuid,
    employee_id: Uuid,
    department_id: Uuid,
    category: &'static str,
    expense_account_id: Uuid,
    amount: Decimal,
    incurred_on: NaiveDate,
    description: String,
    receipt_s3_key: Option<String>,
    status: &'static str,
    manager_approved_by: Option<Uuid>,
    finance_approved_by: Option<Uuid>,
    rejected_by: Option<Uuid>,
    rejection_note: Option<&'static str>,
    journal_entry_id: Option<Uuid>,
}

struct PayrollItemRow {
    id: Uuid,
    run_id: Uuid,
    employee_id: Uuid,
    gross: Decimal,
    deductions: Decimal,
    net: Decimal,
}

struct ThreadRow {
    id: Uuid,
    kind: &'static str,
    subject: String,
    created_by: Uuid,
    audience: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    last_message_at: DateTime<Utc>,
}

struct ParticipantRow {
    thread_id: Uuid,
    employee_id: Uuid,
    role: &'static str,
    last_read_at: Option<DateTime<Utc>>,
}

struct MessageRow {
    id: Uuid,
    thread_id: Uuid,
    sender_id: Uuid,
    body: String,
    importance: &'static str,
    sent_at: DateTime<Utc>,
}

struct TicketRow {
    id: Uuid,
    ticket_no: String,
    thread_id: Uuid,
    requester_id: Uuid,
    category: &'static str,
    priority: &'static str,
    status: &'static str,
    assignee_id: Option<Uuid>,
    sla_due_at: DateTime<Utc>,
    first_response_at: Option<DateTime<Utc>>,
    resolved_at: Option<DateTime<Utc>>,
    closed_at: Option<DateTime<Utc>>,
    satisfaction: Option<i16>,
    created_at: DateTime<Utc>,
}

struct NotificationRow {
    id: Uuid,
    recipient_id: Uuid,
    to_address: String,
    subject: String,
    body_text: String,
    status: &'static str,
    attempts: i32,
    created_at: DateTime<Utc>,
    sent_at: Option<DateTime<Utc>>,
}

struct AuditRow {
    actor_user_id: Option<Uuid>,
    actor_employee_id: Option<Uuid>,
    action: &'static str,
    entity_type: &'static str,
    entity_id: Uuid,
    after: serde_json::Value,
    ip: String,
    at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let mut reset = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--reset" => reset = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown argument {other}; try --help"),
        }
    }

    load_dotenv();
    // The seeder never signs a token, so it does not insist on a real JWT secret;
    // it shares the service Config to read DATABASE_URL and the SEED_* values.
    let signing_key_set = std::env::var("JWT_SECRET").is_ok_and(|v| !v.trim().is_empty());
    if !signing_key_set {
        std::env::set_var("JWT_SECRET", "seed-binary-placeholder-not-used-for-signing");
    }
    let config = Config::from_env().context("loading configuration")?;
    telemetry::init(config.log_format);

    let pool = db::connect(&config.database_url, 4)
        .await
        .context("connecting to postgres")?;
    let outcome = seed(&pool, &config, reset).await?;
    fill_object_storage(&pool, &config).await;
    pool.close().await;

    match outcome {
        Outcome::AlreadySeeded => {
            println!(
                "Bowline demo data is already present (ceo@{EMAIL_DOMAIN} exists). \
                 Nothing was written. Run with --reset to rebuild it."
            );
        }
        Outcome::Seeded(report) => report.print(&config),
    }
    Ok(())
}

/// Rows in `employee_documents` and a `pdf_s3_key` on an invoice are only half of a
/// document: without the bytes behind them every Download button answers 404. The
/// backfill binary owns that job and the seeder runs the same code path, so a fresh
/// `make seed` leaves a demo with nothing broken to click.
///
/// Billing does the rendering, so this needs billing to be up. When it is not, the
/// seeded data is still perfectly good, and this says what to run once it is.
async fn fill_object_storage(pool: &PgPool, config: &Config) {
    match backfill::run(pool, config, backfill::Options::default()).await {
        Ok(summary) if summary.failed == 0 => {}
        Ok(summary) => {
            println!(
                "\n  {} document(s) could not be rendered; the errors are above. \
                 Fix them and run: cd api && cargo run --bin backfill-documents",
                summary.failed
            );
        }
        Err(e) => {
            println!("\n  Object storage was not filled: {e:#}");
            println!(
                "  The demo data is loaded and correct, but document downloads answer 404 \n  \
                 until the objects exist. Once billing is up, run:\n\n      \
                 cd api && cargo run --bin backfill-documents\n"
            );
        }
    }
}

fn load_dotenv() {
    for candidate in [".env", "../.env"] {
        if Path::new(candidate).is_file() {
            let _ = dotenvy::from_filename(candidate);
            return;
        }
    }
}

enum Outcome {
    AlreadySeeded,
    Seeded(Box<Report>),
}

/// One pass: reset if asked, bail out if the company is already there, build it in
/// memory, then write it. Everything after the reset happens in one transaction.
async fn seed(pool: &PgPool, config: &Config, reset: bool) -> Result<Outcome> {
    let mut tx = pool.begin().await.context("beginning transaction")?;
    if reset {
        reset_business_tables(&mut tx).await?;
    }
    let already: bool = sqlx::query_scalar(
        "select exists (select 1 from users where email = 'ceo@bowline.example')",
    )
    .fetch_one(&mut *tx)
    .await
    .context("checking for existing seed data")?;
    if already {
        tx.rollback().await?;
        return Ok(Outcome::AlreadySeeded);
    }

    let reference = Reference::load(&mut tx).await?;
    let mut rng = StdRng::seed_from_u64(config.seed.random_seed);
    let today = Utc::now().date_naive();
    let now = Utc::now();

    tracing::info!(seed = config.seed.random_seed, "building the demo company");
    let mut org = build_org(&mut rng, today, now);
    for user in &mut org.users {
        user.must_change_password = !config.seed.skip_password_change;
    }
    let ops = build_ops(&mut rng, &org, today, now);
    let hr = build_hr(&mut rng, &org, today, now);
    let mut audit = Vec::new();
    let finance = build_finance(&mut rng, &org, &ops, &reference, today, now, &mut audit);
    let mut notifications = Vec::new();
    let comms = build_comms(&mut rng, &org, now, &mut notifications, &mut audit);

    // One shared Argon2id hash: every demo account uses SEED_PASSWORD, and 260
    // separate hashes at m=64MiB would dominate the run time. Real accounts always
    // get their own salt through the same helper the login path verifies against.
    let password_hash = password::hash(&config.seed.password)
        .map_err(anyhow::Error::from)
        .context("hashing SEED_PASSWORD")?;

    write_org(&mut tx, &org, &password_hash, &reference).await?;
    write_ops(&mut tx, &ops).await?;
    write_hr(&mut tx, &hr).await?;
    write_finance(&mut tx, &finance).await?;
    write_comms(&mut tx, &comms).await?;
    write_platform(&mut tx, &notifications, &audit).await?;
    advance_sequences(&mut tx, &ops, &finance, &comms).await?;

    tx.commit()
        .await
        .context("committing the seed transaction")?;
    let report = Report::load(pool, &org).await?;
    Ok(Outcome::Seeded(Box::new(report)))
}

/// Business tables only. Permissions, roles, role permissions, leave types and the
/// chart of accounts survive. `fiscal_periods` references `employees.closed_by`, so
/// the cascade reaches it as well and the periods are rebuilt from the same rule as
/// migration 0008.
async fn reset_business_tables(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    tracing::warn!("--reset: truncating every business table");
    sqlx::query(
        "truncate table
           attendance, shifts, leave_requests, leave_balances, employee_documents,
           thread_participants, messages, support_tickets, threads,
           notifications, audit_log,
           payroll_items, payroll_runs, expenses, vendor_bills, vendors,
           payments, invoice_lines, invoices, journal_lines, journal_entries,
           inventory_items, work_orders, shipment_documents, shipment_events,
           shipment_legs, shipments, vehicles, sites, carriers, customers,
           user_roles, refresh_tokens, users, employees, positions, departments
         restart identity cascade",
    )
    .execute(&mut **tx)
    .await
    .context("truncating business tables")?;

    sqlx::query(
        "insert into fiscal_periods (year, month, starts_on, ends_on)
         select extract(year from d)::smallint, extract(month from d)::smallint,
                d::date, (d + interval '1 month - 1 day')::date
           from generate_series(date_trunc('year', now() - interval '1 year'),
                                date_trunc('year', now() + interval '1 year') + interval '11 months',
                                interval '1 month') d
         on conflict (year, month) do nothing",
    )
    .execute(&mut **tx)
    .await
    .context("restoring fiscal periods")?;

    for sequence in [
        "shipment_ref_seq",
        "invoice_no_seq",
        "ticket_no_seq",
        "journal_entry_seq",
    ] {
        sqlx::query(&format!("alter sequence {sequence} restart with 1"))
            .execute(&mut **tx)
            .await
            .with_context(|| format!("restarting {sequence}"))?;
    }
    Ok(())
}

/// References the reference data by the keys the seeder needs.
struct Reference {
    roles: HashMap<String, i16>,
    accounts: HashMap<String, Uuid>,
    periods: Vec<Period>,
}

struct Period {
    id: Uuid,
    year: i32,
    month: u32,
}

impl Reference {
    async fn load(tx: &mut Transaction<'_, Postgres>) -> Result<Reference> {
        let roles: Vec<(i16, String)> = sqlx::query_as("select id, key::text from roles")
            .fetch_all(&mut **tx)
            .await
            .context("loading roles")?;
        let accounts: Vec<(Uuid, String)> = sqlx::query_as("select id, code from accounts")
            .fetch_all(&mut **tx)
            .await
            .context("loading the chart of accounts")?;
        let periods: Vec<(Uuid, i16, i16)> =
            sqlx::query_as("select id, year, month from fiscal_periods where status = 'open'")
                .fetch_all(&mut **tx)
                .await
                .context("loading fiscal periods")?;
        anyhow::ensure!(!roles.is_empty(), "roles are missing; run the migrations");
        anyhow::ensure!(
            !accounts.is_empty(),
            "the chart of accounts is missing; run the migrations"
        );
        Ok(Reference {
            roles: roles.into_iter().map(|(id, key)| (key, id)).collect(),
            accounts: accounts.into_iter().map(|(id, code)| (code, id)).collect(),
            periods: periods
                .into_iter()
                .map(|(id, year, month)| Period {
                    id,
                    year: year as i32,
                    month: month as u32,
                })
                .collect(),
        })
    }

    fn role(&self, key: &str) -> i16 {
        *self.roles.get(key).expect("seeded role exists")
    }

    fn account(&self, code: &str) -> Uuid {
        *self.accounts.get(code).expect("seeded account exists")
    }

    fn period(&self, date: NaiveDate) -> Option<Uuid> {
        self.periods
            .iter()
            .find(|p| p.year == date.year() && p.month == date.month())
            .map(|p| p.id)
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// A v4 uuid drawn from the seeded generator, so the same seed rebuilds the same
/// company down to the primary keys.
fn uuid(rng: &mut StdRng) -> Uuid {
    uuid::Builder::from_random_bytes(rng.gen()).into_uuid()
}

fn pick<'a, T>(rng: &mut StdRng, values: &'a [T]) -> &'a T {
    values.choose(rng).expect("a non-empty table")
}

fn chance(rng: &mut StdRng, percent: u32) -> bool {
    rng.gen_range(0..100) < percent
}

fn money(units: i64) -> Decimal {
    Decimal::new(units * 100, 2)
}

fn cents(value: i64) -> Decimal {
    Decimal::new(value, 2)
}

fn at(date: NaiveDate, hour: u32, minute: u32) -> DateTime<Utc> {
    date.and_hms_opt(hour, minute, 0)
        .expect("valid time of day")
        .and_utc()
}

/// Caps a timestamp at the moment the seeder runs.
///
/// The company is generated by adding random offsets to real dates, so a column that
/// records something which has *already happened* (a tracking event, a decision, a
/// posting, a reply, a completed task) can otherwise be pushed past now by its own
/// offset and read as broken: a delivery that departs three weeks from today, an audit
/// entry dated tomorrow.
///
/// Planned timestamps are deliberately not passed through this. An ETD, an ETA, a
/// planned leg, a rostered shift, an SLA deadline, a due date and a leave request that
/// starts next month all belong ahead of now, and clamping them would flatten the
/// demo into a company with no future.
fn happened(when: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    when.min(now - Duration::minutes(1))
}

/// Expands a mix table such as `[("delivered", 150), ...]` into one entry per row.
fn expand(mix: &[(&'static str, usize)]) -> Vec<&'static str> {
    mix.iter()
        .flat_map(|(value, count)| std::iter::repeat_n(*value, *count))
        .collect()
}

fn location(place: &(&str, &str, &str)) -> serde_json::Value {
    json!({ "city": place.0, "country": place.1, "code": place.2 })
}

fn slug(value: &str) -> String {
    value
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Organisation: departments, positions, employees, users
// ---------------------------------------------------------------------------

/// Indexes into `Org::employees` for one unit, by rank.
struct UnitSlots {
    director: usize,
    managers: Vec<usize>,
    supervisors: Vec<usize>,
    specialists: Vec<usize>,
    ground: Vec<usize>,
}

struct Org {
    departments: Vec<DeptRow>,
    positions: Vec<PositionRow>,
    employees: Vec<Emp>,
    users: Vec<UserRow>,
    slots: Vec<UnitSlots>,
    ceo: usize,
    division_heads: HashMap<&'static str, usize>,
    by_login: HashMap<String, usize>,
}

impl Org {
    fn dept(&self, code: &str) -> Uuid {
        self.departments
            .iter()
            .find(|d| d.code == code)
            .map(|d| d.id)
            .expect("seeded department exists")
    }

    fn login(&self, local_part: &str) -> &Emp {
        &self.employees[*self.by_login.get(local_part).expect("well-known login")]
    }

    fn user_id_for(&self, employee_id: Uuid) -> Uuid {
        self.users
            .iter()
            .find(|u| u.employee_id == employee_id)
            .map(|u| u.id)
            .expect("every employee has a user")
    }

    /// Active employees at a given rank inside a unit.
    fn rank(&self, unit: usize, level: i16) -> Vec<usize> {
        let slots = &self.slots[unit];
        let pool = match level {
            3 => vec![slots.director],
            4 => slots.managers.clone(),
            5 => slots.supervisors.clone(),
            6 => slots.specialists.clone(),
            _ => slots.ground.clone(),
        };
        pool.into_iter()
            .filter(|i| self.employees[*i].active())
            .collect()
    }

    fn unit_index(code: &str) -> usize {
        UNITS
            .iter()
            .position(|u| u.code == code)
            .expect("known unit code")
    }

    /// One active employee of a rank inside a unit, chosen deterministically.
    fn one(&self, rng: &mut StdRng, unit_code: &str, level: i16) -> usize {
        let unit = Org::unit_index(unit_code);
        let candidates = self.rank(unit, level);
        *pick(rng, &candidates)
    }
}

struct EmpSpec {
    position_id: Uuid,
    department_id: Uuid,
    manager_id: Option<Uuid>,
    level: i16,
    title: &'static str,
    dept_code: &'static str,
    site: &'static str,
    login: &'static str,
}

/// Annual base salary by level, before jitter.
const SALARY: [i64; 7] = [340_000, 245_000, 168_000, 122_000, 92_000, 71_000, 52_000];
/// How many years back a level is typically hired.
const HIRE_YEARS: [(i64, i64); 7] = [(9, 14), (6, 12), (4, 10), (3, 9), (2, 8), (0, 7), (0, 5)];

struct EmpFactory<'a> {
    rng: &'a mut StdRng,
    today: NaiveDate,
    employees: Vec<Emp>,
    used_names: HashSet<String>,
    used_emails: HashSet<String>,
}

impl EmpFactory<'_> {
    fn name(&mut self) -> (String, String) {
        loop {
            let first = pick(self.rng, FIRST_NAMES).to_string();
            let last = pick(self.rng, LAST_NAMES).to_string();
            let full = format!("{first} {last}");
            if self.used_names.insert(full) {
                return (first, last);
            }
        }
    }

    fn add(&mut self, spec: EmpSpec) -> usize {
        let (first, last) = self.name();
        let email = if spec.login.is_empty() {
            let base = format!("{}.{}", slug(&first), slug(&last));
            let mut candidate = format!("{base}@{EMAIL_DOMAIN}");
            let mut suffix = 2;
            while self.used_emails.contains(&candidate) {
                candidate = format!("{base}{suffix}@{EMAIL_DOMAIN}");
                suffix += 1;
            }
            candidate
        } else {
            format!("{}@{EMAIL_DOMAIN}", spec.login)
        };
        self.used_emails.insert(email.clone());

        let level_idx = (spec.level - 1) as usize;
        let jitter = self.rng.gen_range(92..=108);
        let salary = (SALARY[level_idx] * jitter / 100 + 250) / 500 * 500;
        let (from, to) = HIRE_YEARS[level_idx];
        let hire_date = self.today - Duration::days(self.rng.gen_range(from * 365..=to * 365));
        let employment_type = if spec.level >= 6 && chance(self.rng, 12) {
            if chance(self.rng, 60) {
                "part_time"
            } else {
                "contract"
            }
        } else {
            "full_time"
        };
        let grade = ['A', 'B', 'C'][self.rng.gen_range(0..3)];
        let index = self.employees.len();
        self.employees.push(Emp {
            id: uuid(self.rng),
            employee_no: format!("EMP-{:06}", index + 1),
            first_name: first,
            last_name: last,
            email,
            phone: format!(
                "+31 10 {:03} {:04}",
                self.rng.gen_range(100..999),
                self.rng.gen_range(1000..9999)
            ),
            position_id: spec.position_id,
            department_id: spec.department_id,
            manager_id: spec.manager_id,
            status: "active",
            employment_type,
            hire_date,
            termination_date: None,
            site: spec.site,
            pay_grade: format!("G{}{}", spec.level, grade),
            base_salary: money(salary),
            level: spec.level,
            title: spec.title,
            dept_code: spec.dept_code,
            roles: roles_for(spec.dept_code, spec.level),
            well_known: !spec.login.is_empty(),
        });
        index
    }
}

fn roles_for(dept: &str, level: i16) -> Vec<&'static str> {
    let mut roles = vec!["baseline"];
    roles.push(match level {
        1 | 2 => "executive",
        3 => "director",
        4 => "manager",
        5 => "supervisor",
        6 => "staff",
        _ => "field_worker",
    });
    match (dept, level) {
        ("FIN", 2) | ("ACC", 3) | ("AR", 4) => roles.push("finance_admin"),
        ("ACC" | "AR" | "PAY" | "AP", 5 | 6) => roles.push("accountant"),
        ("PO", 4..=6) => roles.push("hr_admin"),
        ("ROAD" | "FLEET", 5 | 6) => roles.push("dispatcher"),
        ("SD", 4..=6) => roles.push("support_agent"),
        ("PLAT", 3..=5) => roles.push("it_admin"),
        _ => {}
    }
    roles
}

fn build_org(rng: &mut StdRng, today: NaiveDate, now: DateTime<Utc>) -> Org {
    // Departments: the executive office, one per division, one per unit.
    let mut departments = Vec::new();
    let exec_id = uuid(rng);
    departments.push(DeptRow {
        id: exec_id,
        code: "EXEC",
        name: "Executive Office",
        parent_id: None,
        cost_center: "CC-100".to_string(),
    });
    let mut division_ids: HashMap<&'static str, Uuid> = HashMap::new();
    for (index, &(code, name, _, _, _)) in DIVISIONS.iter().enumerate() {
        let id = uuid(rng);
        division_ids.insert(code, id);
        departments.push(DeptRow {
            id,
            code,
            name,
            parent_id: Some(exec_id),
            cost_center: format!("CC-{}", 200 + index * 100),
        });
    }
    for (index, unit) in UNITS.iter().enumerate() {
        departments.push(DeptRow {
            id: uuid(rng),
            code: unit.code,
            name: unit.name,
            parent_id: Some(division_ids[unit.division]),
            cost_center: format!("CC-{}", 300 + index * 10),
        });
    }
    let dept_id = |code: &str| -> Uuid {
        departments
            .iter()
            .find(|d| d.code == code)
            .map(|d| d.id)
            .expect("department was just built")
    };

    // Positions: one per rank per unit, plus the executive positions.
    let mut positions = Vec::new();
    let ceo_position = PositionRow {
        id: uuid(rng),
        code: "EXE-CEO".to_string(),
        title: "Chief Executive Officer",
        level: 1,
        department_id: exec_id,
        people_manager: true,
    };
    let ceo_position_id = ceo_position.id;
    positions.push(ceo_position);
    let mut division_positions: HashMap<&'static str, Uuid> = HashMap::new();
    for &(code, _, title, position_code, _) in DIVISIONS {
        let id = uuid(rng);
        division_positions.insert(code, id);
        positions.push(PositionRow {
            id,
            code: position_code.to_string(),
            title,
            level: 2,
            department_id: division_ids[code],
            people_manager: true,
        });
    }
    let mut unit_positions: Vec<[Option<Uuid>; 5]> = Vec::new();
    for unit in UNITS {
        let mut ids = [None; 5];
        for (rank, title) in unit.titles.iter().enumerate() {
            if title.is_empty() {
                continue;
            }
            let id = uuid(rng);
            ids[rank] = Some(id);
            positions.push(PositionRow {
                id,
                code: format!("{}-{}", unit.code, ["D", "M", "S", "C", "G"][rank]),
                title,
                level: (rank + 3) as i16,
                department_id: dept_id(unit.code),
                people_manager: rank <= 2,
            });
        }
        unit_positions.push(ids);
    }

    // Employees, level by level.
    let mut factory = EmpFactory {
        rng,
        today,
        employees: Vec::new(),
        used_names: HashSet::new(),
        used_emails: HashSet::new(),
    };
    let ceo = factory.add(EmpSpec {
        position_id: ceo_position_id,
        department_id: exec_id,
        manager_id: None,
        level: 1,
        title: "Chief Executive Officer",
        dept_code: "EXEC",
        site: "HQ",
        login: "ceo",
    });
    let ceo_id = factory.employees[ceo].id;

    let mut division_heads: HashMap<&'static str, usize> = HashMap::new();
    for &(code, _, title, _, login) in DIVISIONS {
        let index = factory.add(EmpSpec {
            position_id: division_positions[code],
            department_id: division_ids[code],
            manager_id: Some(ceo_id),
            level: 2,
            title,
            dept_code: code,
            site: "HQ",
            login,
        });
        division_heads.insert(code, index);
    }

    let mut slots: Vec<UnitSlots> = Vec::new();
    for (unit_index, unit) in UNITS.iter().enumerate() {
        let head = factory.employees[division_heads[unit.division]].id;
        let director = factory.add(EmpSpec {
            position_id: unit_positions[unit_index][0].expect("every unit has a director"),
            department_id: dept_id(unit.code),
            manager_id: Some(head),
            level: 3,
            title: unit.titles[0],
            dept_code: unit.code,
            site: unit.site,
            login: unit.logins[0],
        });
        slots.push(UnitSlots {
            director,
            managers: Vec::new(),
            supervisors: Vec::new(),
            specialists: Vec::new(),
            ground: Vec::new(),
        });
    }
    for (unit_index, unit) in UNITS.iter().enumerate() {
        let director_id = factory.employees[slots[unit_index].director].id;
        for slot in 0..unit.managers {
            let index = factory.add(EmpSpec {
                position_id: unit_positions[unit_index][1].expect("manager position"),
                department_id: dept_id(unit.code),
                manager_id: Some(director_id),
                level: 4,
                title: unit.titles[1],
                dept_code: unit.code,
                site: unit.site,
                login: if slot == 0 { unit.logins[1] } else { "" },
            });
            slots[unit_index].managers.push(index);
        }
    }
    for (unit_index, unit) in UNITS.iter().enumerate() {
        let managers: Vec<Uuid> = slots[unit_index]
            .managers
            .iter()
            .map(|i| factory.employees[*i].id)
            .collect();
        for slot in 0..unit.supervisors {
            let index = factory.add(EmpSpec {
                position_id: unit_positions[unit_index][2].expect("supervisor position"),
                department_id: dept_id(unit.code),
                manager_id: Some(managers[slot % managers.len()]),
                level: 5,
                title: unit.titles[2],
                dept_code: unit.code,
                site: unit.site,
                login: if slot == 0 { unit.logins[2] } else { "" },
            });
            slots[unit_index].supervisors.push(index);
        }
    }
    for rank in [3usize, 4] {
        for (unit_index, unit) in UNITS.iter().enumerate() {
            let Some(position_id) = unit_positions[unit_index][rank] else {
                continue;
            };
            let leads: Vec<Uuid> = slots[unit_index]
                .supervisors
                .iter()
                .map(|i| factory.employees[*i].id)
                .collect();
            let rank_and_file = unit.headcount - 1 - unit.managers - unit.supervisors;
            let ground = (rank_and_file as f64 * unit.ground_share).round() as usize;
            let count = if rank == 3 {
                rank_and_file - ground
            } else {
                ground
            };
            for slot in 0..count {
                let index = factory.add(EmpSpec {
                    position_id,
                    department_id: dept_id(unit.code),
                    manager_id: Some(leads[slot % leads.len()]),
                    level: (rank + 3) as i16,
                    title: unit.titles[rank],
                    dept_code: unit.code,
                    site: unit.site,
                    login: if slot == 0 { unit.logins[rank] } else { "" },
                });
                if rank == 3 {
                    slots[unit_index].specialists.push(index);
                } else {
                    slots[unit_index].ground.push(index);
                }
            }
        }
    }

    let mut employees = factory.employees;

    // One internal auditor, so the read-only role has a holder.
    if let Some(index) = employees
        .iter()
        .rposition(|e| e.dept_code == "ACC" && e.level == 6)
    {
        employees[index].roles.push("auditor");
    }

    // A realistic spread of lifecycle states, never touching a well-known login or
    // anyone with reports.
    let mut candidates: Vec<usize> = employees
        .iter()
        .enumerate()
        .filter(|(_, e)| e.level >= 6 && !e.well_known)
        .map(|(i, _)| i)
        .collect();
    candidates.shuffle(rng);
    let mut candidates = candidates.into_iter();
    for index in candidates.by_ref().take(6) {
        let hired = employees[index].hire_date;
        let left = today - Duration::days(rng.gen_range(20..300));
        employees[index].status = "terminated";
        employees[index].termination_date = Some(left.max(hired + Duration::days(90)).min(today));
    }
    for index in candidates.by_ref().take(8) {
        employees[index].status = "on_leave";
    }
    for index in candidates.by_ref().take(2) {
        employees[index].status = "suspended";
    }

    let users = employees
        .iter()
        .map(|e| UserRow {
            id: uuid(rng),
            employee_id: e.id,
            email: e.email.clone(),
            status: match e.status {
                "terminated" => "disabled",
                "suspended" => "locked",
                _ => "active",
            },
            must_change_password: false,
            last_login_at: if e.status == "terminated" {
                None
            } else {
                Some(now - Duration::hours(rng.gen_range(1..600)))
            },
        })
        .collect();

    let by_login = employees
        .iter()
        .enumerate()
        .filter(|(_, e)| e.well_known)
        .map(|(i, e)| {
            let local = e.email.split('@').next().expect("an email local part");
            (local.to_string(), i)
        })
        .collect();

    Org {
        departments,
        positions,
        employees,
        users,
        slots,
        ceo,
        division_heads,
        by_login,
    }
}

// ---------------------------------------------------------------------------
// Operations: sites, carriers, vehicles, customers, vendors, shipments
// ---------------------------------------------------------------------------

struct Ops {
    sites: Vec<SiteRow>,
    carriers: Vec<CarrierRow>,
    vehicles: Vec<VehicleRow>,
    customers: Vec<CustomerRow>,
    vendors: Vec<VendorRow>,
    shipments: Vec<ShipmentRow>,
    legs: Vec<LegRow>,
    events: Vec<EventRow>,
    documents: Vec<DocRow>,
    work_orders: Vec<WorkOrderRow>,
    inventory: Vec<InventoryRow>,
}

/// Vehicles: plate prefix, kind, capacity in kg, how many, home site.
const FLEET_PLAN: &[(&str, &str, i64, usize, &str)] = &[
    ("BW-TR", "truck", 24_000, 10, "HUB"),
    ("BW-VN", "van", 3_500, 6, "HUB"),
    ("BW-TL", "trailer", 30_000, 5, "BOND"),
    ("BW-FL", "forklift", 2_500, 4, "BOND"),
];

/// Mode mix, one entry per twentieth of the shipment book.
const MODE_MIX: &[(&str, usize)] = &[("sea", 8), ("air", 5), ("road", 6), ("rail", 1)];

const INCOTERMS: &[&str] = &["EXW", "FCA", "FOB", "CIF", "DAP", "DDP"];

const CLOCK_SOURCES: &[&str] = &["kiosk", "mobile", "web"];

const PAYMENT_METHODS: &[&str] = &["bank_transfer", "bank_transfer", "card", "cheque"];

const TICKET_PRIORITIES: &[&str] = &["low", "normal", "normal", "high", "urgent"];

fn transit_days(rng: &mut StdRng, mode: &str) -> i64 {
    match mode {
        "sea" => rng.gen_range(18..36),
        "air" => rng.gen_range(2..6),
        "rail" => rng.gen_range(5..10),
        _ => rng.gen_range(1..5),
    }
}

/// How far back the departure sits for each status.
fn etd_offset(rng: &mut StdRng, status: &str) -> i64 {
    match status {
        "draft" => -rng.gen_range(5..25),
        "booked" => -rng.gen_range(2..18),
        "picked_up" => rng.gen_range(0..4),
        "in_transit" => rng.gen_range(3..20),
        "customs" => rng.gen_range(10..30),
        "out_for_delivery" => rng.gen_range(12..35),
        "delivered" => rng.gen_range(20..180),
        "exception" => rng.gen_range(5..40),
        _ => rng.gen_range(5..60),
    }
}

fn timeline(status: &str, previous: Option<&'static str>) -> Vec<&'static str> {
    match status {
        "draft" => vec!["created"],
        "booked" => vec!["created", "booked"],
        "picked_up" => vec!["created", "booked", "picked_up"],
        "in_transit" => vec!["created", "booked", "picked_up", "departed"],
        "customs" => vec![
            "created",
            "booked",
            "picked_up",
            "departed",
            "arrived",
            "customs_hold",
        ],
        "out_for_delivery" => vec![
            "created",
            "booked",
            "picked_up",
            "departed",
            "arrived",
            "customs_cleared",
            "out_for_delivery",
        ],
        "delivered" => vec![
            "created",
            "booked",
            "picked_up",
            "departed",
            "arrived",
            "customs_cleared",
            "out_for_delivery",
            "delivered",
        ],
        "cancelled" => vec!["created", "booked", "cancelled"],
        "exception" => {
            let mut events = timeline(previous.unwrap_or("in_transit"), None);
            events.push("exception");
            events
        }
        _ => vec!["created"],
    }
}

fn leg_status(shipment_status: &str, seq: usize, legs: usize) -> &'static str {
    match shipment_status {
        "draft" | "booked" => "planned",
        "cancelled" => "cancelled",
        "delivered" => "completed",
        "picked_up" => {
            if seq == 0 {
                "in_progress"
            } else {
                "planned"
            }
        }
        "in_transit" | "exception" => match seq {
            0 => "completed",
            1 => "in_progress",
            _ => "planned",
        },
        "customs" => {
            if seq + 1 < legs {
                "completed"
            } else {
                "planned"
            }
        }
        "out_for_delivery" => {
            if seq + 1 < legs {
                "completed"
            } else {
                "in_progress"
            }
        }
        _ => "planned",
    }
}

fn build_ops(rng: &mut StdRng, org: &Org, today: NaiveDate, now: DateTime<Utc>) -> Ops {
    // Sites, each run by the director of the department that lives there.
    let mut sites = Vec::new();
    for &(code, name, kind, city, country) in SITES {
        let manager = match code {
            "HQ" => org.division_heads["OPS"],
            "PORT" => org.slots[Org::unit_index("SEA")].director,
            "AIRP" => org.slots[Org::unit_index("AIR")].director,
            "HUB" => org.slots[Org::unit_index("ROAD")].director,
            _ => org.slots[Org::unit_index("WH")].director,
        };
        sites.push(SiteRow {
            id: uuid(rng),
            code,
            name,
            kind,
            address: json!({
                "line1": format!("{} Harbour Way", rng.gen_range(1..240)),
                "city": city,
                "country": country,
                "postcode": format!("{} AB", rng.gen_range(1000..9999)),
            }),
            manager_id: Some(org.employees[manager].id),
        });
    }
    let site_id = |code: &str| -> Uuid {
        sites
            .iter()
            .find(|s| s.code == code)
            .map(|s| s.id)
            .expect("seeded site")
    };

    let carriers: Vec<CarrierRow> = CARRIERS
        .iter()
        .map(|&(code, name, mode, scac, on_time)| CarrierRow {
            id: uuid(rng),
            code,
            name,
            mode,
            scac,
            contact: json!({
                "email": format!("ops@{}.example", slug(name)),
                "phone": "+31 10 555 0100",
            }),
            on_time_rate: Decimal::new(on_time, 4),
        })
        .collect();

    let mut vehicles = Vec::new();
    for &(prefix, kind, capacity, count, home) in FLEET_PLAN {
        for n in 1..=count {
            let status = match (n, kind) {
                (1, "truck") => "maintenance",
                (2, "truck") | (1, "van") => "in_use",
                (5, "trailer") => "retired",
                _ => "available",
            };
            vehicles.push(VehicleRow {
                id: uuid(rng),
                plate: format!("{prefix}-{n:02}"),
                kind,
                capacity_kg: money(capacity),
                status,
                home_site_id: site_id(home),
            });
        }
    }

    // Customers, each with an account executive from Sales.
    let sales_unit = Org::unit_index("SALES");
    let account_managers = org.rank(sales_unit, 6);
    let mut customers = Vec::new();
    for index in 0..40 {
        let name = format!("{} {}", pick(rng, COMPANY_HEADS), pick(rng, COMPANY_TAILS));
        let status = match index {
            7 | 19 | 31 => "on_hold",
            37 => "closed",
            _ => "active",
        };
        let contact = format!("{} {}", pick(rng, FIRST_NAMES), pick(rng, LAST_NAMES));
        customers.push(CustomerRow {
            id: uuid(rng),
            code: format!("CUST-{:04}", index + 1),
            contact_email: format!("{}@{}.example", slug(&contact), slug(&name)),
            contact_name: contact,
            phone: format!(
                "+31 20 {:03} {:04}",
                rng.gen_range(100..999),
                rng.gen_range(1000..9999)
            ),
            billing_address: json!({
                "line1": format!("{} Trade Park", rng.gen_range(1..180)),
                "city": pick(rng, INLAND).0,
                "country": "NL",
                "postcode": format!("{} XX", rng.gen_range(1000..9999)),
            }),
            credit_limit: money(rng.gen_range(1..21) * 25_000),
            status,
            account_manager_id: Some(
                org.employees[account_managers[index % account_managers.len()]].id,
            ),
            name,
        });
    }
    let billable: Vec<usize> = (0..customers.len())
        .filter(|i| customers[*i].status != "closed")
        .collect();

    let vendors: Vec<VendorRow> = VENDOR_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| VendorRow {
            id: uuid(rng),
            code: format!("VEND-{:03}", index + 1),
            name,
            contact: json!({
                "email": format!("accounts@{}.example", slug(name)),
                "terms": "net 30",
            }),
        })
        .collect();

    // Shipments and their legs, events and documents.
    let mut statuses = expand(SHIPMENT_MIX);
    statuses.shuffle(rng);
    let modes = expand(MODE_MIX);
    let coordinators: HashMap<&str, Vec<usize>> = [
        ("sea", org.rank(Org::unit_index("SEA"), 6)),
        ("air", org.rank(Org::unit_index("AIR"), 6)),
        ("road", org.rank(Org::unit_index("ROAD"), 6)),
    ]
    .into_iter()
    .collect();
    let drivers = org.rank(Org::unit_index("FLEET"), 7);
    let road_vehicles: Vec<usize> = (0..vehicles.len())
        .filter(|i| matches!(vehicles[*i].kind, "truck" | "van"))
        .collect();
    let carriers_by_mode: HashMap<&'static str, Vec<Uuid>> = ["sea", "air", "road", "rail"]
        .into_iter()
        .map(|mode| {
            (
                mode,
                carriers
                    .iter()
                    .filter(|c| c.mode == mode)
                    .map(|c| c.id)
                    .collect(),
            )
        })
        .collect();

    let year = today.year();
    let mut shipments = Vec::new();
    let mut legs = Vec::new();
    let mut events = Vec::new();
    let mut documents = Vec::new();

    for (index, status) in statuses.into_iter().enumerate() {
        let mode = *pick(rng, &modes);
        let (places, main_mode) = match mode {
            "sea" => (SEA_PORTS, "sea"),
            "air" => (AIRPORTS, "air"),
            "rail" => (INLAND, "rail"),
            _ => (INLAND, "road"),
        };
        let origin = pick(rng, places);
        let mut destination = pick(rng, places);
        while destination.2 == origin.2 {
            destination = pick(rng, places);
        }
        let owner = *pick(
            rng,
            coordinators
                .get(if mode == "rail" { "road" } else { mode })
                .expect("a coordinator pool"),
        );
        let owner_id = org.employees[owner].id;
        let etd = today - Duration::days(etd_offset(rng, status));
        let transit = transit_days(rng, mode);
        let eta = etd + Duration::days(transit);
        let previous_status = if status == "exception" {
            Some(*pick(
                rng,
                &["booked", "picked_up", "in_transit", "customs"],
            ))
        } else {
            None
        };
        let delivered_at = if status == "delivered" {
            Some(at(eta, 14, 30).min(now - Duration::hours(2)))
        } else {
            None
        };
        let weight = rng.gen_range(120..28_000);
        let shipment = ShipmentRow {
            id: uuid(rng),
            reference: format!("BWL-{year}-{:06}", index + 1),
            customer_id: customers[*pick(rng, &billable)].id,
            mode,
            incoterm: pick(rng, INCOTERMS),
            origin: location(origin),
            destination: location(destination),
            cargo_description: pick(rng, CARGO),
            pieces: rng.gen_range(1..420),
            weight_kg: money(weight),
            volume_cbm: Decimal::new(weight * 1000 / 260, 3),
            hazardous: chance(rng, 8),
            declared_value: money(rng.gen_range(5..250) * 1_000),
            status,
            previous_status,
            etd,
            eta,
            delivered_at,
            delay_risk: Decimal::new(rng.gen_range(150..8_600), 4),
            owner_id,
        };

        // Legs: pre-carriage, main leg, on-carriage for sea and air.
        let plan: Vec<(&'static str, serde_json::Value, serde_json::Value)> = match mode {
            "sea" | "air" => vec![
                ("road", location(pick(rng, INLAND)), location(origin)),
                (main_mode, location(origin), location(destination)),
                ("road", location(destination), location(pick(rng, INLAND))),
            ],
            "rail" => vec![
                ("rail", location(origin), location(destination)),
                ("road", location(destination), location(pick(rng, INLAND))),
            ],
            _ => vec![("road", location(origin), location(destination))],
        };
        let total_legs = plan.len();
        for (seq, (leg_mode, from, to)) in plan.into_iter().enumerate() {
            let share = transit as f64 / total_legs as f64;
            let departure = at(etd, 8, 0) + Duration::hours((seq as f64 * share * 24.0) as i64);
            let arrival = departure + Duration::hours((share * 24.0) as i64 + 4);
            let state = leg_status(status, seq, total_legs);
            let carrier = carriers_by_mode[leg_mode].choose(rng).copied();
            let is_road = leg_mode == "road";
            legs.push(LegRow {
                id: uuid(rng),
                shipment_id: shipment.id,
                seq: (seq + 1) as i16,
                mode: leg_mode,
                carrier_id: carrier,
                vehicle_id: if is_road {
                    Some(vehicles[*pick(rng, &road_vehicles)].id)
                } else {
                    None
                },
                driver_id: if is_road {
                    Some(org.employees[*pick(rng, &drivers)].id)
                } else {
                    None
                },
                from_location: from,
                to_location: to,
                planned_departure: departure,
                planned_arrival: arrival,
                // The planned pair may sit in the future; the actual pair records a
                // movement that has already been made.
                actual_departure: match state {
                    "completed" | "in_progress" => Some(happened(
                        departure + Duration::minutes(rng.gen_range(-90..240)),
                        now,
                    )),
                    _ => None,
                },
                actual_arrival: match state {
                    "completed" => Some(happened(
                        arrival + Duration::minutes(rng.gen_range(-60..420)),
                        now,
                    )),
                    _ => None,
                },
                status: state,
            });
        }

        // Events: one timeline consistent with the status. A tracking event is a record
        // of something that happened, so the whole timeline ends at or before now, even
        // for a shipment whose ETD is still ahead of us. The window is at least six
        // hours wide so the events keep their order.
        let types = timeline(status, previous_status);
        let last = happened(at(eta, 17, 0), now);
        let first = (at(etd, 9, 0) - Duration::days(2)).min(last - Duration::hours(6));
        let span = (last - first).num_minutes().max(60);
        let count = types.len();
        for (position, event_type) in types.into_iter().enumerate() {
            let fraction = if count > 1 {
                position as f64 / (count - 1) as f64
            } else {
                0.0
            };
            events.push(EventRow {
                id: uuid(rng),
                shipment_id: shipment.id,
                event_type,
                occurred_at: first + Duration::minutes((span as f64 * fraction) as i64),
                location: if position == 0 {
                    origin.0.to_string()
                } else {
                    destination.0.to_string()
                },
                note: match event_type {
                    "customs_hold" => {
                        Some("Held for document check by the customs office".to_string())
                    }
                    "exception" => Some("Carrier reported a delay, customer informed".to_string()),
                    "cancelled" => Some("Cancelled at the customer's request".to_string()),
                    _ => None,
                },
                recorded_by: owner_id,
            });
        }

        // Paperwork for anything that has actually moved.
        if matches!(
            status,
            "in_transit" | "customs" | "out_for_delivery" | "delivered"
        ) && chance(rng, 55)
        {
            let main_doc = match mode {
                "air" => ("air_waybill", "Air waybill"),
                "sea" => ("bill_of_lading", "Bill of lading"),
                _ => ("commercial_invoice", "Commercial invoice"),
            };
            let mut kinds = vec![main_doc, ("packing_list", "Packing list")];
            if status == "delivered" {
                kinds.push(("proof_of_delivery", "Proof of delivery"));
            }
            for (kind, title) in kinds {
                documents.push(DocRow {
                    id: uuid(rng),
                    parent_id: shipment.id,
                    kind,
                    title: format!("{title} {}", shipment.reference),
                    s3_key: format!("shipments/{}/{kind}.pdf", shipment.id),
                    mime_type: "application/pdf",
                    size_bytes: rng.gen_range(40_000..900_000),
                    uploaded_by: owner_id,
                });
            }
        }

        shipments.push(shipment);
    }

    // Work orders for the ground crews.
    let workers: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.active() && (e.level == 7 || (e.level == 6 && matches!(e.dept_code, "WH" | "SEA")))
        })
        .map(|(index, _)| index)
        .collect();
    let movable: Vec<usize> = (0..shipments.len())
        .filter(|i| !matches!(shipments[*i].status, "draft" | "cancelled"))
        .collect();
    let mut work_statuses = expand(WORK_ORDER_MIX);
    work_statuses.shuffle(rng);
    let mut work_orders = Vec::new();
    for status in work_statuses {
        let worker = &org.employees[*pick(rng, &workers)];
        let kind = *pick(
            rng,
            match worker.site {
                "BOND" => &["loading", "unloading", "inventory"][..],
                "HUB" => &["pickup", "delivery"][..],
                _ => &["loading", "unloading", "inspection"][..],
            },
        );
        let shipment = if kind == "inventory" {
            None
        } else {
            Some(shipments[*pick(rng, &movable)].id)
        };
        // A work order may fall due in the next few days; starting and finishing it are
        // things that have already happened.
        let due_at = now - Duration::hours(rng.gen_range(-96..480));
        let started_at = match status {
            "done" | "in_progress" => {
                Some(happened(due_at - Duration::hours(rng.gen_range(1..6)), now))
            }
            _ => None,
        };
        work_orders.push(WorkOrderRow {
            id: uuid(rng),
            shipment_id: shipment,
            site_id: site_id(worker.site),
            kind,
            title: match kind {
                "loading" => "Load outbound trailer".to_string(),
                "unloading" => "Strip inbound container".to_string(),
                "pickup" => "Collect from consignor".to_string(),
                "delivery" => "Deliver to consignee".to_string(),
                "inspection" => "Seal and condition check".to_string(),
                _ => "Cycle count, bonded aisle".to_string(),
            },
            instructions: "Scan every pallet, photograph any damage before it moves.".to_string(),
            assigned_to: worker.id,
            assigned_by: worker.manager_id.expect("ground staff have a manager"),
            status,
            due_at,
            started_at,
            completed_at: if status == "done" {
                started_at.map(|s| happened(s + Duration::minutes(rng.gen_range(25..300)), now))
            } else {
                None
            },
            notes: match status {
                "blocked" => Some("Waiting on customs release"),
                "cancelled" => Some("Shipment re-routed to the road hub"),
                _ => None,
            },
        });
    }

    // Inventory held at the warehouses.
    let mut inventory = Vec::new();
    for _ in 0..140 {
        let shipment = &shipments[*pick(rng, &movable)];
        let received = now - Duration::days(rng.gen_range(1..60));
        inventory.push(InventoryRow {
            id: uuid(rng),
            site_id: site_id(pick(rng, &["BOND", "PORT", "AIRP"])),
            shipment_id: Some(shipment.id),
            description: shipment.cargo_description.to_string(),
            quantity: rng.gen_range(1..400),
            bin: format!(
                "{}-{:02}-{:02}",
                ['A', 'B', 'C', 'D'][rng.gen_range(0..4)],
                rng.gen_range(1..24),
                rng.gen_range(1..12)
            ),
            received_at: received,
            released_at: if shipment.status == "delivered" {
                Some(happened(
                    received + Duration::days(rng.gen_range(1..12)),
                    now,
                ))
            } else {
                None
            },
        });
    }

    Ops {
        sites,
        carriers,
        vehicles,
        customers,
        vendors,
        shipments,
        legs,
        events,
        documents,
        work_orders,
        inventory,
    }
}

// ---------------------------------------------------------------------------
// HR: leave, shifts, attendance, documents
// ---------------------------------------------------------------------------

struct Hr {
    balances: Vec<LeaveBalanceRow>,
    requests: Vec<LeaveRequestRow>,
    shifts: Vec<ShiftRow>,
    attendance: Vec<AttendanceRow>,
    documents: Vec<DocRow>,
}

const LEAVE_REASONS: &[&str] = &[
    "Family holiday",
    "Long weekend away",
    "Medical appointment",
    "Moving house",
    "Wedding in the family",
    "Recovering after an operation",
    "School holidays",
];

/// Every status appears, and the ring keeps the mix stable across runs.
const LEAVE_STATES: &[&str] = &[
    "approved",
    "pending",
    "approved",
    "rejected",
    "approved",
    "cancelled",
];

const LEAVE_TYPES: &[(&str, i64, i64)] = &[
    ("annual", 1, 5),
    ("annual", 2, 5),
    ("sick", 1, 3),
    ("unpaid", 1, 2),
    ("parental", 5, 10),
];

fn business_days(start: NaiveDate, end: NaiveDate) -> i64 {
    let mut day = start;
    let mut count = 0;
    while day <= end {
        if !matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            count += 1;
        }
        day = day.succ_opt().expect("a valid next day");
    }
    count
}

fn build_hr(rng: &mut StdRng, org: &Org, today: NaiveDate, now: DateTime<Utc>) -> Hr {
    let year = today.year();
    let month_start = NaiveDate::from_ymd_opt(year, today.month(), 1).expect("valid month");
    let base = month_start
        .checked_sub_months(Months::new(6))
        .expect("six months back");

    // Leave requests first: the balances then reflect what was actually approved.
    let mut applicants: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| e.active())
        .map(|(index, _)| index)
        .collect();
    applicants.shuffle(rng);
    applicants.truncate(130);

    let mut requests = Vec::new();
    let mut ring = 0usize;
    for employee_index in applicants {
        let employee = &org.employees[employee_index];
        let mut offsets: Vec<u32> = (0..9).collect();
        offsets.shuffle(rng);
        for offset in offsets.into_iter().take(rng.gen_range(1..=3)) {
            let month = base
                .checked_add_months(Months::new(offset))
                .expect("a month inside the seeded periods");
            let (type_key, low, high) = *pick(rng, LEAVE_TYPES);
            let start = month + Duration::days(rng.gen_range(0..16));
            let end = start + Duration::days(rng.gen_range(low..=high) - 1);
            let days = business_days(start, end);
            if days == 0 {
                continue;
            }
            let status = LEAVE_STATES[ring % LEAVE_STATES.len()];
            ring += 1;
            let decided = matches!(status, "approved" | "rejected");
            requests.push(LeaveRequestRow {
                id: uuid(rng),
                employee_id: employee.id,
                type_key,
                start_date: start,
                end_date: end,
                days: Decimal::new(days * 10, 1),
                reason: pick(rng, LEAVE_REASONS),
                status,
                current_approver_id: if status == "pending" {
                    employee.manager_id
                } else {
                    None
                },
                decided_by: if decided {
                    employee.manager_id
                } else if status == "cancelled" {
                    Some(employee.id)
                } else {
                    None
                },
                // Leave itself may start next month, but the decision on it was taken
                // before now.
                decided_at: if status == "pending" {
                    None
                } else {
                    Some(happened(
                        at(start, 9, 0) - Duration::days(rng.gen_range(3..20)),
                        now,
                    ))
                },
                decision_note: match status {
                    "rejected" => Some("Two people are already off that week"),
                    "cancelled" => Some("Withdrawn by the employee"),
                    _ => None,
                },
            });
        }
    }

    // Balances for the current year, with `used` taken from approved requests.
    let mut used: HashMap<(Uuid, &'static str), i64> = HashMap::new();
    for request in &requests {
        if request.status == "approved" && request.start_date.year() == year {
            *used
                .entry((request.employee_id, request.type_key))
                .or_default() += business_days(request.start_date, request.end_date);
        }
    }
    let mut balances = Vec::new();
    for employee in &org.employees {
        for (type_key, allocated) in [("annual", 20), ("sick", 10), ("unpaid", 0)] {
            let taken = used
                .get(&(employee.id, type_key))
                .copied()
                .unwrap_or_default();
            balances.push(LeaveBalanceRow {
                employee_id: employee.id,
                year: year as i16,
                type_key,
                allocated: Decimal::new(allocated * 10, 1),
                // Unpaid leave has no quota, and the schema only allows `used` to
                // exceed `allocated` there; every other type is capped.
                used: Decimal::new(
                    if allocated == 0 {
                        taken
                    } else {
                        taken.min(allocated)
                    } * 10,
                    1,
                ),
            });
        }
        if let Some(taken) = used.get(&(employee.id, "parental")) {
            balances.push(LeaveBalanceRow {
                employee_id: employee.id,
                year: year as i16,
                type_key: "parental",
                allocated: Decimal::new(900, 1),
                used: Decimal::new(taken * 10, 1),
            });
        }
    }

    // Two weeks of rosters for the operational crews.
    let crew: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.active()
                && matches!(e.dept_code, "SEA" | "AIR" | "ROAD" | "WH" | "FLEET")
                && matches!(e.level, 5 | 7)
        })
        .map(|(index, _)| index)
        .collect();
    let mut shifts = Vec::new();
    let mut attendance = Vec::new();
    for day in -13i64..=2 {
        let date = today + Duration::days(day);
        if date.weekday() == Weekday::Sun {
            continue;
        }
        for (position, employee_index) in crew.iter().enumerate() {
            if chance(rng, 12) {
                continue;
            }
            let employee = &org.employees[*employee_index];
            let early = (position + (day + 13) as usize).is_multiple_of(2);
            let starts_at = at(date, if early { 6 } else { 14 }, 0);
            let ends_at = starts_at + Duration::hours(8);
            let past = date < today;
            let status = if !past {
                "scheduled"
            } else if chance(rng, 4) {
                "missed"
            } else if chance(rng, 3) {
                "cancelled"
            } else {
                "completed"
            };
            let shift = ShiftRow {
                id: uuid(rng),
                employee_id: employee.id,
                site: employee.site,
                starts_at,
                ends_at,
                role_on_shift: match employee.level {
                    5 => "shift lead",
                    _ => match employee.dept_code {
                        "WH" => "dock",
                        "FLEET" | "ROAD" => "driver",
                        _ => "yard",
                    },
                },
                status,
                created_by: employee.manager_id.expect("crews have a manager"),
            };
            if status == "completed" {
                let clock_in = starts_at + Duration::minutes(rng.gen_range(-8..26));
                attendance.push(AttendanceRow {
                    id: uuid(rng),
                    employee_id: employee.id,
                    shift_id: shift.id,
                    clock_in,
                    clock_out: Some(ends_at + Duration::minutes(rng.gen_range(-10..31))),
                    late: clock_in > starts_at + Duration::minutes(10),
                    source: pick(rng, CLOCK_SOURCES),
                });
            }
            shifts.push(shift);
        }
    }

    // Personnel files.
    let hr_admin = org.login("hr.admin").id;
    let mut documents = Vec::new();
    for employee in &org.employees {
        documents.push(DocRow {
            id: uuid(rng),
            parent_id: employee.id,
            kind: "contract",
            title: format!("Employment contract, {}", employee.employee_no),
            s3_key: format!("employees/{}/contract.pdf", employee.id),
            mime_type: "application/pdf",
            size_bytes: rng.gen_range(60_000..240_000),
            uploaded_by: hr_admin,
        });
        if chance(rng, 40) {
            documents.push(DocRow {
                id: uuid(rng),
                parent_id: employee.id,
                kind: "id",
                title: "Identity document".to_string(),
                s3_key: format!("employees/{}/id.pdf", employee.id),
                mime_type: "application/pdf",
                size_bytes: rng.gen_range(40_000..180_000),
                uploaded_by: hr_admin,
            });
        }
        if chance(rng, 15) {
            documents.push(DocRow {
                id: uuid(rng),
                parent_id: employee.id,
                kind: "certificate",
                title: "Forklift and dangerous goods certificate".to_string(),
                s3_key: format!("employees/{}/certificate.pdf", employee.id),
                mime_type: "application/pdf",
                size_bytes: rng.gen_range(30_000..120_000),
                uploaded_by: hr_admin,
            });
        }
        if employee.active() && chance(rng, 30) {
            let last_month = now - Duration::days(30);
            documents.push(DocRow {
                id: uuid(rng),
                parent_id: employee.id,
                kind: "payslip",
                title: format!("Payslip {}", last_month.format("%Y-%m")),
                s3_key: format!(
                    "employees/{}/payslip-{}.pdf",
                    employee.id,
                    last_month.format("%Y-%m")
                ),
                mime_type: "application/pdf",
                size_bytes: rng.gen_range(20_000..60_000),
                uploaded_by: hr_admin,
            });
        }
    }

    Hr {
        balances,
        requests,
        shifts,
        attendance,
        documents,
    }
}

// ---------------------------------------------------------------------------
// Finance: ledger, receivables, payables, expenses, payroll
// ---------------------------------------------------------------------------

struct PayrollRunRow {
    id: Uuid,
    period_id: Uuid,
    status: &'static str,
    total_gross: Decimal,
    total_deductions: Decimal,
    total_net: Decimal,
    created_by: Uuid,
    approved_by: Uuid,
    approved_at: DateTime<Utc>,
    posted_at: DateTime<Utc>,
    journal_entry_id: Uuid,
}

struct Finance {
    entries: Vec<EntryRow>,
    lines: Vec<LineRow>,
    reversals: Vec<(Uuid, Uuid)>,
    invoices: Vec<InvoiceRow>,
    invoice_lines: Vec<InvoiceLineRow>,
    payments: Vec<PaymentRow>,
    bills: Vec<BillRow>,
    expenses: Vec<ExpenseRow>,
    payroll_run: Option<PayrollRunRow>,
    payroll_items: Vec<PayrollItemRow>,
}

/// One journal entry with its lines: account, debit, credit, description.
struct Posting {
    date: NaiveDate,
    memo: String,
    source_type: &'static str,
    source_id: Option<Uuid>,
    posted_by: Uuid,
    reverses_entry_id: Option<Uuid>,
    lines: Vec<(Uuid, Decimal, Decimal, String)>,
}

struct Ledger {
    /// The moment the seeder runs: a posting is never stamped later than that.
    now: DateTime<Utc>,
    entries: Vec<EntryRow>,
    lines: Vec<LineRow>,
}

impl Ledger {
    fn new(now: DateTime<Utc>) -> Ledger {
        Ledger {
            now,
            entries: Vec::new(),
            lines: Vec::new(),
        }
    }

    /// Posts one entry, or nothing when the date falls outside the seeded periods.
    fn post(&mut self, rng: &mut StdRng, reference: &Reference, posting: Posting) -> Option<Uuid> {
        let period_id = reference.period(posting.date)?;
        let debit: Decimal = posting.lines.iter().map(|line| line.1).sum();
        let credit: Decimal = posting.lines.iter().map(|line| line.2).sum();
        assert_eq!(
            debit, credit,
            "seeded entry '{}' does not balance",
            posting.memo
        );
        let id = uuid(rng);
        self.entries.push(EntryRow {
            id,
            period_id,
            entry_date: posting.date,
            memo: posting.memo,
            source_type: posting.source_type,
            source_id: posting.source_id,
            posted_by: posting.posted_by,
            posted_at: happened(at(posting.date, 17, 0), self.now),
            reverses_entry_id: posting.reverses_entry_id,
        });
        for (account_id, debit, credit, description) in posting.lines {
            self.lines.push(LineRow {
                id: uuid(rng),
                entry_id: id,
                account_id,
                debit,
                credit,
                description,
            });
        }
        Some(id)
    }
}

/// Billable charges: description and the revenue account they land on.
const CHARGES: &[(&str, &str)] = &[
    ("Ocean freight, port to port", "4000"),
    ("Air freight, airport to airport", "4000"),
    ("Road haulage", "4000"),
    ("Fuel surcharge", "4000"),
    ("Warehouse storage, per pallet week", "4100"),
    ("Handling in and out", "4100"),
    ("Customs entry and clearance", "4200"),
    ("Documentation fee", "4200"),
];

/// Expense claims: category, account, description.
const EXPENSE_KINDS: &[(&str, &str, &str)] = &[
    ("travel", "5700", "Rail fare to a customer site"),
    ("fuel", "5200", "Diesel for the depot van"),
    ("meals", "5700", "Meals during the night shift"),
    ("supplies", "5400", "Labels and printer supplies"),
    ("equipment", "5400", "Replacement barcode scanner"),
    ("other", "5400", "Parking and road tolls"),
];

/// Vendor bills: account and what the vendor billed for.
const BILL_KINDS: &[(&str, &str)] = &[
    ("5000", "Carrier services"),
    ("5200", "Fuel deliveries"),
    ("5300", "Warehouse services"),
    ("5400", "Office and administration"),
];

fn build_finance(
    rng: &mut StdRng,
    org: &Org,
    ops: &Ops,
    reference: &Reference,
    today: NaiveDate,
    now: DateTime<Utc>,
    audit: &mut Vec<AuditRow>,
) -> Finance {
    let mut ledger = Ledger::new(now);
    let mut reversals = Vec::new();
    let year = today.year();

    let cfo = org.login("cfo");
    let director_finance = org.login("director.finance");
    let billing_manager = org.login("manager.billing");
    let accountant = org.login("accountant");
    let payroll_manager = &org.employees[org.one(rng, "PAY", 4)];
    let billing_clerks = org.rank(Org::unit_index("AR"), 6);

    let cash = reference.account("1000");
    let receivable = reference.account("1100");
    let equipment = reference.account("1500");
    let payable = reference.account("2000");
    let salaries_payable = reference.account("2100");
    let taxes_payable = reference.account("2200");
    let share_capital = reference.account("3000");
    let retained = reference.account("3100");
    let salaries = reference.account("5100");
    let depreciation = reference.account("5500");

    // Opening balances, so the trial balance reads like a going concern.
    let opening = NaiveDate::from_ymd_opt(year - 1, 1, 1).expect("first of last year");
    ledger.post(
        rng,
        reference,
        Posting {
            date: opening,
            memo: "Opening balances".to_string(),
            source_type: "manual",
            source_id: None,
            posted_by: cfo.id,
            reverses_entry_id: None,
            lines: vec![
                (cash, money(2_400_000), Decimal::ZERO, "Bank".to_string()),
                (
                    equipment,
                    money(900_000),
                    Decimal::ZERO,
                    "Vehicles and handling equipment".to_string(),
                ),
                (
                    share_capital,
                    Decimal::ZERO,
                    money(2_800_000),
                    "Share capital".to_string(),
                ),
                (
                    retained,
                    Decimal::ZERO,
                    money(500_000),
                    "Retained earnings brought forward".to_string(),
                ),
            ],
        },
    );

    // Monthly depreciation for the three complete months before this one.
    for back in 1..=3u32 {
        let month = NaiveDate::from_ymd_opt(year, today.month(), 1)
            .expect("valid month")
            .checked_sub_months(Months::new(back))
            .expect("a month inside the seeded periods");
        let last_day = month
            .checked_add_months(Months::new(1))
            .expect("next month")
            - Duration::days(1);
        ledger.post(
            rng,
            reference,
            Posting {
                date: last_day,
                memo: format!("Depreciation {}", month.format("%Y-%m")),
                source_type: "manual",
                source_id: None,
                posted_by: accountant.id,
                reverses_entry_id: None,
                lines: vec![
                    (
                        depreciation,
                        money(12_500),
                        Decimal::ZERO,
                        "Monthly depreciation".to_string(),
                    ),
                    (
                        equipment,
                        Decimal::ZERO,
                        money(12_500),
                        "Accumulated depreciation".to_string(),
                    ),
                ],
            },
        );
    }

    // Invoices, their lines, their ledger entries and their payments.
    let mut statuses = expand(INVOICE_MIX);
    statuses.shuffle(rng);
    let invoiceable: Vec<usize> = (0..ops.shipments.len())
        .filter(|i| {
            matches!(
                ops.shipments[*i].status,
                "delivered" | "in_transit" | "out_for_delivery" | "customs"
            )
        })
        .collect();
    let mut invoices = Vec::new();
    let mut invoice_lines = Vec::new();
    let mut payments = Vec::new();

    for (index, status) in statuses.into_iter().enumerate() {
        let shipment = &ops.shipments[*pick(rng, &invoiceable)];
        let issued = matches!(status, "issued" | "partially_paid" | "paid" | "void");
        let overdue = issued && chance(rng, 30);
        let issue_date = today
            - Duration::days(if overdue {
                rng.gen_range(45..130)
            } else {
                rng.gen_range(3..40)
            });
        let invoice_id = uuid(rng);
        let invoice_no = format!("INV-{year}-{:06}", index + 1);

        let mut subtotal = Decimal::ZERO;
        let mut tax = Decimal::ZERO;
        let mut revenue: Vec<(Uuid, Decimal)> = Vec::new();
        for seq in 1..=rng.gen_range(1..=4) {
            let (description, account_code) = *pick(rng, CHARGES);
            let quantity = Decimal::from(rng.gen_range(1..25i64));
            let unit_price = cents(rng.gen_range(15_000..420_000));
            let amount = (quantity * unit_price).round_dp(2);
            let rate = if chance(rng, 60) {
                Decimal::new(2100, 4)
            } else {
                Decimal::ZERO
            };
            subtotal += amount;
            tax += (amount * rate).round_dp(2);
            let account = reference.account(account_code);
            match revenue.iter_mut().find(|(id, _)| *id == account) {
                Some(entry) => entry.1 += amount,
                None => revenue.push((account, amount)),
            }
            invoice_lines.push(InvoiceLineRow {
                id: uuid(rng),
                invoice_id,
                seq,
                description,
                quantity,
                unit_price,
                tax_rate: rate,
                amount,
            });
        }
        let total = subtotal + tax;

        let mut entry_id = None;
        if issued {
            let mut lines = vec![(
                receivable,
                total,
                Decimal::ZERO,
                format!("{invoice_no} {}", shipment.reference),
            )];
            for (account, amount) in &revenue {
                lines.push((*account, Decimal::ZERO, *amount, "Revenue".to_string()));
            }
            if tax > Decimal::ZERO {
                lines.push((taxes_payable, Decimal::ZERO, tax, "VAT charged".to_string()));
            }
            entry_id = ledger.post(
                rng,
                reference,
                Posting {
                    date: issue_date,
                    memo: format!("Invoice {invoice_no}"),
                    source_type: "invoice",
                    source_id: Some(invoice_id),
                    posted_by: billing_manager.id,
                    reverses_entry_id: None,
                    lines,
                },
            );
            audit.push(AuditRow {
                actor_user_id: Some(org.user_id_for(billing_manager.id)),
                actor_employee_id: Some(billing_manager.id),
                action: "invoice.issue",
                entity_type: "invoice",
                entity_id: invoice_id,
                after: json!({ "invoice_no": invoice_no, "total": total.to_string(), "status": "issued" }),
                ip: "10.20.0.14".to_string(),
                at: happened(at(issue_date, 11, 12), now),
            });
        }

        // Void invoices carry a reversing entry, never an edited one.
        if status == "void" {
            if let Some(original) = entry_id {
                let mut lines = vec![(
                    receivable,
                    Decimal::ZERO,
                    total,
                    format!("Reversal of {invoice_no}"),
                )];
                for (account, amount) in &revenue {
                    lines.push((
                        *account,
                        *amount,
                        Decimal::ZERO,
                        "Revenue reversed".to_string(),
                    ));
                }
                if tax > Decimal::ZERO {
                    lines.push((
                        taxes_payable,
                        tax,
                        Decimal::ZERO,
                        "VAT reversed".to_string(),
                    ));
                }
                if let Some(reversal) = ledger.post(
                    rng,
                    reference,
                    Posting {
                        date: issue_date + Duration::days(2),
                        memo: format!("Void invoice {invoice_no}"),
                        source_type: "reversal",
                        source_id: Some(invoice_id),
                        posted_by: director_finance.id,
                        reverses_entry_id: Some(original),
                        lines,
                    },
                ) {
                    reversals.push((original, reversal));
                }
            }
        }

        // Payments against the ones that were settled.
        let mut amount_paid = Decimal::ZERO;
        if matches!(status, "paid" | "partially_paid") {
            let instalments = if status == "paid" && chance(rng, 35) {
                vec![
                    (total * Decimal::new(60, 2)).round_dp(2),
                    total - (total * Decimal::new(60, 2)).round_dp(2),
                ]
            } else if status == "paid" {
                vec![total]
            } else {
                vec![(total * Decimal::new(rng.gen_range(25..70), 2)).round_dp(2)]
            };
            for (instalment, amount) in instalments.into_iter().enumerate() {
                if amount <= Decimal::ZERO {
                    continue;
                }
                let received_on = (issue_date
                    + Duration::days(rng.gen_range(8..40) + instalment as i64 * 15))
                .min(today);
                let clerk = &org.employees[*pick(rng, &billing_clerks)];
                let payment_entry = ledger.post(
                    rng,
                    reference,
                    Posting {
                        date: received_on,
                        memo: format!("Payment for {invoice_no}"),
                        source_type: "payment",
                        source_id: Some(invoice_id),
                        posted_by: clerk.id,
                        reverses_entry_id: None,
                        lines: vec![
                            (cash, amount, Decimal::ZERO, "Bank receipt".to_string()),
                            (
                                receivable,
                                Decimal::ZERO,
                                amount,
                                format!("Settlement of {invoice_no}"),
                            ),
                        ],
                    },
                );
                let Some(payment_entry) = payment_entry else {
                    continue;
                };
                amount_paid += amount;
                payments.push(PaymentRow {
                    id: uuid(rng),
                    invoice_id,
                    received_on,
                    amount,
                    method: pick(rng, PAYMENT_METHODS),
                    reference: format!("REM-{}-{}", year, rng.gen_range(100_000..999_999)),
                    recorded_by: clerk.id,
                    journal_entry_id: payment_entry,
                });
            }
        }
        // A payment that could not be posted must not leave the invoice overpaid.
        let status = if amount_paid.is_zero() && status == "partially_paid" {
            "issued"
        } else {
            status
        };

        invoices.push(InvoiceRow {
            id: invoice_id,
            customer_id: shipment.customer_id,
            shipment_id: Some(shipment.id),
            status,
            issue_date: issued.then_some(issue_date),
            due_date: issued.then_some(issue_date + Duration::days(30)),
            subtotal,
            tax,
            total,
            amount_paid,
            notes: (status == "void")
                .then(|| "Cancelled, replaced by a corrected invoice".to_string()),
            pdf_s3_key: issued.then(|| format!("invoices/{invoice_no}.pdf")),
            created_by: accountant.id,
            approved_by: (total >= money(50_000)).then_some(director_finance.id),
            issued_by: issued.then_some(billing_manager.id),
            journal_entry_id: entry_id,
            invoice_no,
        });
    }

    // Expense claims.
    let mut expense_statuses = expand(EXPENSE_MIX);
    expense_statuses.shuffle(rng);
    let claimants: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| e.active() && e.manager_id.is_some())
        .map(|(index, _)| index)
        .collect();
    let mut expenses = Vec::new();
    for status in expense_statuses {
        let employee = &org.employees[*pick(rng, &claimants)];
        let (category, account_code, description) = *pick(rng, EXPENSE_KINDS);
        let amount = cents(rng.gen_range(2_500..120_000));
        let incurred_on = today - Duration::days(rng.gen_range(2..90));
        let expense_id = uuid(rng);
        let account = reference.account(account_code);
        let journal_entry_id = if status == "paid" {
            ledger.post(
                rng,
                reference,
                Posting {
                    // Reimbursed about a week after the claim, but never later than
                    // today: the ledger only holds postings that have been made.
                    date: (incurred_on + Duration::days(6)).min(today),
                    memo: format!(
                        "Expense claim, {} {}",
                        employee.first_name, employee.last_name
                    ),
                    source_type: "expense",
                    source_id: Some(expense_id),
                    posted_by: cfo.id,
                    reverses_entry_id: None,
                    lines: vec![
                        (account, amount, Decimal::ZERO, description.to_string()),
                        (cash, Decimal::ZERO, amount, "Reimbursed".to_string()),
                    ],
                },
            )
        } else {
            None
        };
        expenses.push(ExpenseRow {
            id: expense_id,
            employee_id: employee.id,
            department_id: employee.department_id,
            category,
            expense_account_id: account,
            amount,
            incurred_on,
            description: description.to_string(),
            receipt_s3_key: chance(rng, 80).then(|| format!("receipts/{expense_id}.jpg")),
            status,
            manager_approved_by: matches!(status, "manager_approved" | "finance_approved" | "paid")
                .then_some(employee.manager_id)
                .flatten(),
            finance_approved_by: matches!(status, "finance_approved" | "paid")
                .then_some(director_finance.id),
            rejected_by: (status == "rejected")
                .then_some(employee.manager_id)
                .flatten(),
            rejection_note: (status == "rejected").then_some("No receipt attached"),
            journal_entry_id,
        });
    }

    // Vendor bills.
    let mut bill_statuses = expand(BILL_MIX);
    bill_statuses.shuffle(rng);
    let procurement = &org.employees[org.one(rng, "AP", 6)];
    let mut bills = Vec::new();
    for (index, status) in bill_statuses.into_iter().enumerate() {
        let vendor = pick(rng, &ops.vendors);
        let (account_code, description) = *pick(rng, BILL_KINDS);
        let account = reference.account(account_code);
        let amount = cents(rng.gen_range(50_000..1_800_000));
        let received_on = today - Duration::days(rng.gen_range(5..120));
        let bill_id = uuid(rng);
        let bill_no = format!("{year}-{:04}", index + 1);
        let journal_entry_id = if matches!(status, "approved" | "paid") {
            ledger.post(
                rng,
                reference,
                Posting {
                    date: received_on,
                    memo: format!("Bill {bill_no}, {}", vendor.name),
                    source_type: "bill",
                    source_id: Some(bill_id),
                    posted_by: procurement.id,
                    reverses_entry_id: None,
                    lines: vec![
                        (account, amount, Decimal::ZERO, description.to_string()),
                        (payable, Decimal::ZERO, amount, vendor.name.to_string()),
                    ],
                },
            )
        } else {
            None
        };
        let paid_on = (status == "paid").then(|| (received_on + Duration::days(25)).min(today));
        let payment_entry_id = paid_on.and_then(|date| {
            ledger.post(
                rng,
                reference,
                Posting {
                    date,
                    memo: format!("Payment of bill {bill_no}"),
                    source_type: "payment",
                    source_id: Some(bill_id),
                    posted_by: cfo.id,
                    reverses_entry_id: None,
                    lines: vec![
                        (payable, amount, Decimal::ZERO, "Settled".to_string()),
                        (cash, Decimal::ZERO, amount, vendor.name.to_string()),
                    ],
                },
            )
        });
        bills.push(BillRow {
            id: bill_id,
            vendor_id: vendor.id,
            bill_no,
            expense_account_id: account,
            amount,
            received_on,
            due_on: received_on + Duration::days(30),
            status,
            approved_by: matches!(status, "approved" | "paid").then_some(cfo.id),
            paid_on: payment_entry_id.and(paid_on),
            journal_entry_id,
            payment_entry_id,
        });
    }

    // One posted payroll run for last month.
    let last_month = NaiveDate::from_ymd_opt(year, today.month(), 1)
        .expect("valid month")
        .checked_sub_months(Months::new(1))
        .expect("last month");
    let pay_date = last_month
        .checked_add_months(Months::new(1))
        .expect("this month")
        - Duration::days(1);
    let mut payroll_items = Vec::new();
    let mut total_gross = Decimal::ZERO;
    let mut total_deductions = Decimal::ZERO;
    let run_id = uuid(rng);
    for employee in org.employees.iter().filter(|e| e.active()) {
        let gross = (employee.base_salary / Decimal::from(12)).round_dp(2);
        let deductions = (gross * Decimal::new(28, 2)).round_dp(2);
        total_gross += gross;
        total_deductions += deductions;
        payroll_items.push(PayrollItemRow {
            id: uuid(rng),
            run_id,
            employee_id: employee.id,
            gross,
            deductions,
            net: gross - deductions,
        });
    }
    let total_net = total_gross - total_deductions;
    let payroll_entry = ledger.post(
        rng,
        reference,
        Posting {
            date: pay_date,
            memo: format!("Payroll {}", last_month.format("%Y-%m")),
            source_type: "payroll",
            source_id: Some(run_id),
            posted_by: cfo.id,
            reverses_entry_id: None,
            lines: vec![
                (
                    salaries,
                    total_gross,
                    Decimal::ZERO,
                    "Gross salaries".to_string(),
                ),
                (
                    salaries_payable,
                    Decimal::ZERO,
                    total_net,
                    "Net pay".to_string(),
                ),
                (
                    taxes_payable,
                    Decimal::ZERO,
                    total_deductions,
                    "Payroll taxes and contributions".to_string(),
                ),
            ],
        },
    );
    let payroll_run = payroll_entry.map(|journal_entry_id| PayrollRunRow {
        id: run_id,
        period_id: reference.period(pay_date).expect("last month is open"),
        status: "posted",
        total_gross,
        total_deductions,
        total_net,
        created_by: payroll_manager.id,
        approved_by: cfo.id,
        approved_at: at(pay_date, 9, 30),
        posted_at: at(pay_date, 16, 0),
        journal_entry_id,
    });
    if payroll_run.is_none() {
        payroll_items.clear();
    }

    Finance {
        entries: ledger.entries,
        lines: ledger.lines,
        reversals,
        invoices,
        invoice_lines,
        payments,
        bills,
        expenses,
        payroll_run,
        payroll_items,
    }
}

// ---------------------------------------------------------------------------
// Communications: announcements, direct threads, the support desk
// ---------------------------------------------------------------------------

struct Comms {
    threads: Vec<ThreadRow>,
    participants: Vec<ParticipantRow>,
    messages: Vec<MessageRow>,
    tickets: Vec<TicketRow>,
}

/// Announcements: subject and body, fanned out to an audience below.
const ANNOUNCEMENTS: &[(&str, &str)] = &[
    (
        "Quarter in review and what comes next",
        "Thank you all for a strong quarter. Volumes on the sea lanes grew by eleven percent \
         and on time delivery held above ninety percent. The board approved the new bonded \
         racking, which goes in over the next two months. Site leads will share the detail \
         with their teams this week.",
    ),
    (
        "Winter schedule for the depots",
        "From the first of next month the port and hub depots move to the winter schedule. \
         Early shifts start at six, late shifts finish at ten in the evening, and the yard \
         closes to visiting hauliers at nine. Supervisors will publish the rosters on Friday.",
    ),
    (
        "New scanning flow in the bonded warehouse",
        "The handheld scanners now ask for the bin location before the pallet is released. \
         It is one extra step and it removes most of the stock corrections we were making at \
         the end of the week. Ask your dock supervisor for a walkthrough if anything is unclear.",
    ),
];

fn subtree_of(org: &Org, root: usize) -> Vec<usize> {
    let mut inside: HashSet<Uuid> = HashSet::new();
    inside.insert(org.employees[root].id);
    let mut members = Vec::new();
    // Employees are built from the top down, so one pass reaches every descendant.
    for (index, employee) in org.employees.iter().enumerate() {
        if let Some(manager) = employee.manager_id {
            if inside.contains(&manager) {
                inside.insert(employee.id);
                members.push(index);
            }
        }
    }
    members
}

fn build_comms(
    rng: &mut StdRng,
    org: &Org,
    now: DateTime<Utc>,
    notifications: &mut Vec<NotificationRow>,
    audit: &mut Vec<AuditRow>,
) -> Comms {
    let mut threads = Vec::new();
    let mut participants = Vec::new();
    let mut messages = Vec::new();

    // One outbox row per recipient of every message, exactly as the API does.
    let contacts: HashMap<Uuid, (String, bool)> = org
        .employees
        .iter()
        .map(|e| (e.id, (e.email.clone(), e.active())))
        .collect();
    let mut fan_out =
        |rng: &mut StdRng, thread: &ThreadRow, sender: Uuid, body: &str, recipients: &[Uuid]| {
            for recipient in recipients {
                let Some((address, active)) = contacts.get(recipient) else {
                    continue;
                };
                if !active || *recipient == sender {
                    continue;
                }
                let age = now - thread.last_message_at;
                notifications.push(NotificationRow {
                    id: uuid(rng),
                    recipient_id: *recipient,
                    to_address: address.clone(),
                    subject: thread.subject.clone(),
                    body_text: body.chars().take(400).collect(),
                    status: if age > Duration::hours(6) {
                        "sent"
                    } else if chance(rng, 6) {
                        "failed"
                    } else {
                        "pending"
                    },
                    attempts: if age > Duration::hours(6) { 1 } else { 0 },
                    created_at: thread.last_message_at,
                    sent_at: (age > Duration::hours(6))
                        .then(|| thread.last_message_at + Duration::minutes(2)),
                });
            }
        };

    // Three announcements: the whole company, one division, one subtree.
    let everyone: Vec<usize> = (0..org.employees.len()).collect();
    let operations: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.dept_code == "OPS"
                || UNITS
                    .iter()
                    .any(|u| u.code == e.dept_code && u.division == "OPS")
        })
        .map(|(index, _)| index)
        .collect();
    let warehouse_manager = org.by_login["manager.warehouse"];
    let warehouse = subtree_of(org, warehouse_manager);

    let announcements: [(usize, serde_json::Value, Vec<usize>); 3] = [
        (org.ceo, json!({ "scope": "company" }), everyone),
        (
            org.division_heads["OPS"],
            json!({ "scope": "department", "ref": org.dept("OPS") }),
            operations,
        ),
        (
            warehouse_manager,
            json!({
                "scope": "subtree",
                "ref": org.employees[warehouse_manager].id,
            }),
            warehouse,
        ),
    ];

    for (position, (sender_index, audience_json, audience)) in announcements.into_iter().enumerate()
    {
        let (subject, body) = ANNOUNCEMENTS[position];
        let sender = &org.employees[sender_index];
        let sent_at = now - Duration::hours(rng.gen_range(2..240));
        let thread = ThreadRow {
            id: uuid(rng),
            kind: "announcement",
            subject: subject.to_string(),
            created_by: sender.id,
            audience: Some(audience_json),
            created_at: sent_at,
            last_message_at: sent_at,
        };
        participants.push(ParticipantRow {
            thread_id: thread.id,
            employee_id: sender.id,
            role: "sender",
            last_read_at: Some(sent_at),
        });
        let mut recipients = Vec::new();
        for index in &audience {
            let employee = &org.employees[*index];
            if !employee.active() || employee.id == sender.id {
                continue;
            }
            recipients.push(employee.id);
            participants.push(ParticipantRow {
                thread_id: thread.id,
                employee_id: employee.id,
                role: "recipient",
                // Most people have read it by now; nobody has read it later than now.
                last_read_at: chance(rng, 65)
                    .then(|| happened(sent_at + Duration::minutes(rng.gen_range(4..900)), now)),
            });
        }
        messages.push(MessageRow {
            id: uuid(rng),
            thread_id: thread.id,
            sender_id: sender.id,
            body: body.to_string(),
            importance: if position == 0 { "high" } else { "normal" },
            sent_at,
        });
        fan_out(rng, &thread, sender.id, body, &recipients);
        threads.push(thread);
    }

    // Direct threads up and down the chain of command.
    let chatty: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| e.active() && e.manager_id.is_some() && e.level >= 4)
        .map(|(index, _)| index)
        .collect();
    for (position, (subject, opening)) in DIRECT_SUBJECTS.iter().enumerate() {
        for round in 0..2 {
            let employee = &org.employees[chatty[(position * 3 + round) % chatty.len()]];
            let manager_id = employee.manager_id.expect("a manager");
            let started = now - Duration::hours(rng.gen_range(3..500));
            let reply_at = happened(started + Duration::minutes(rng.gen_range(20..600)), now);
            let thread = ThreadRow {
                id: uuid(rng),
                kind: "direct",
                subject: subject.to_string(),
                created_by: employee.id,
                audience: None,
                created_at: started,
                last_message_at: reply_at,
            };
            participants.push(ParticipantRow {
                thread_id: thread.id,
                employee_id: employee.id,
                role: "sender",
                last_read_at: Some(reply_at),
            });
            participants.push(ParticipantRow {
                thread_id: thread.id,
                employee_id: manager_id,
                role: "recipient",
                last_read_at: Some(reply_at),
            });
            messages.push(MessageRow {
                id: uuid(rng),
                thread_id: thread.id,
                sender_id: employee.id,
                body: opening.to_string(),
                importance: "normal",
                sent_at: started,
            });
            let reply = *pick(rng, MANAGER_REPLIES);
            messages.push(MessageRow {
                id: uuid(rng),
                thread_id: thread.id,
                sender_id: manager_id,
                body: reply.to_string(),
                importance: "normal",
                sent_at: reply_at,
            });
            fan_out(rng, &thread, employee.id, opening, &[manager_id]);
            threads.push(thread);
        }
    }

    // The support desk.
    let agents = org.rank(Org::unit_index("SD"), 6);
    let agent_ids: Vec<Uuid> = agents.iter().map(|i| org.employees[*i].id).collect();
    let requesters: Vec<usize> = org
        .employees
        .iter()
        .enumerate()
        .filter(|(_, e)| e.active() && e.dept_code != "SD")
        .map(|(index, _)| index)
        .collect();
    let mut ticket_statuses = expand(TICKET_MIX);
    ticket_statuses.shuffle(rng);
    let mut tickets = Vec::new();
    for (index, status) in ticket_statuses.into_iter().enumerate() {
        let (category, subject, opening) = TICKET_SEEDS[index % TICKET_SEEDS.len()];
        let requester = &org.employees[*pick(rng, &requesters)];
        let priority: &str = pick(rng, TICKET_PRIORITIES);
        let sla_hours = match priority {
            "urgent" => 1,
            "high" => 4,
            "normal" => 24,
            _ => 72,
        };
        let created_at = now - Duration::hours(rng.gen_range(2..720));
        let assigned = status != "open";
        let assignee = assigned.then(|| *pick(rng, &agent_ids));
        // The SLA deadline may still be ahead of us; answering, resolving and closing
        // the ticket are all things the desk has already done.
        let first_response_at =
            assigned.then(|| happened(created_at + Duration::minutes(rng.gen_range(8..400)), now));
        let resolved_at = matches!(status, "resolved" | "closed")
            .then(|| happened(created_at + Duration::hours(rng.gen_range(3..90)), now));
        let closed_at = (status == "closed")
            .then(|| resolved_at.map(|r| happened(r + Duration::hours(rng.gen_range(2..72)), now)))
            .flatten();
        let last_message_at = closed_at
            .or(resolved_at)
            .or(first_response_at)
            .unwrap_or(created_at)
            .min(now - Duration::minutes(5));

        let thread = ThreadRow {
            id: uuid(rng),
            kind: "ticket",
            subject: subject.to_string(),
            created_by: requester.id,
            audience: None,
            created_at,
            last_message_at,
        };
        participants.push(ParticipantRow {
            thread_id: thread.id,
            employee_id: requester.id,
            role: "sender",
            last_read_at: Some(last_message_at),
        });
        if let Some(agent) = assignee {
            participants.push(ParticipantRow {
                thread_id: thread.id,
                employee_id: agent,
                role: "agent",
                last_read_at: Some(last_message_at),
            });
        }
        messages.push(MessageRow {
            id: uuid(rng),
            thread_id: thread.id,
            sender_id: requester.id,
            body: opening.to_string(),
            importance: if priority == "urgent" {
                "high"
            } else {
                "normal"
            },
            sent_at: created_at,
        });
        if let (Some(agent), Some(replied_at)) = (assignee, first_response_at) {
            messages.push(MessageRow {
                id: uuid(rng),
                thread_id: thread.id,
                sender_id: agent,
                body: pick(rng, AGENT_REPLIES).to_string(),
                importance: "normal",
                sent_at: replied_at,
            });
            if matches!(status, "waiting_on_requester" | "resolved" | "closed") {
                messages.push(MessageRow {
                    id: uuid(rng),
                    thread_id: thread.id,
                    sender_id: requester.id,
                    body: pick(rng, REQUESTER_REPLIES).to_string(),
                    importance: "normal",
                    sent_at: last_message_at,
                });
            }
        }
        let desk: Vec<Uuid> = assignee.into_iter().chain([requester.id]).collect();
        fan_out(rng, &thread, requester.id, opening, &desk);

        let ticket_id = uuid(rng);
        if let (Some(agent), Some(when)) = (assignee, resolved_at) {
            audit.push(AuditRow {
                actor_user_id: Some(org.user_id_for(agent)),
                actor_employee_id: Some(agent),
                action: "ticket.resolve",
                entity_type: "support_ticket",
                entity_id: ticket_id,
                after: json!({ "status": "resolved", "category": category }),
                ip: "10.20.0.51".to_string(),
                at: when,
            });
        }
        tickets.push(TicketRow {
            id: ticket_id,
            ticket_no: format!("TKT-{:06}", index + 1),
            thread_id: thread.id,
            requester_id: requester.id,
            category,
            priority,
            status,
            assignee_id: assignee,
            sla_due_at: created_at + Duration::hours(sla_hours),
            first_response_at,
            resolved_at,
            closed_at,
            satisfaction: (status == "closed").then(|| rng.gen_range(3..=5i16)),
            created_at,
        });
        threads.push(thread);
    }

    Comms {
        threads,
        participants,
        messages,
        tickets,
    }
}

// ---------------------------------------------------------------------------
// Writers: batched inserts, in dependency order
// ---------------------------------------------------------------------------

async fn write_org(
    tx: &mut Transaction<'_, Postgres>,
    org: &Org,
    password_hash: &str,
    reference: &Reference,
) -> Result<()> {
    // Departments, parents before children.
    let divisions = 1 + DIVISIONS.len();
    for slice in [
        &org.departments[..1],
        &org.departments[1..divisions],
        &org.departments[divisions..],
    ] {
        for part in slice.chunks(CHUNK) {
            let mut qb = QueryBuilder::new(
                "insert into departments (id, code, name, parent_id, cost_center) ",
            );
            qb.push_values(part, |mut b, row| {
                b.push_bind(row.id)
                    .push_bind(row.code)
                    .push_bind(row.name)
                    .push_bind(row.parent_id)
                    .push_bind(row.cost_center.clone());
            });
            qb.build().execute(&mut **tx).await?;
        }
    }

    for part in org.positions.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into positions (id, code, title, level, department_id, is_people_manager) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.code.clone())
                .push_bind(row.title)
                .push_bind(row.level)
                .push_bind(row.department_id)
                .push_bind(row.people_manager);
        });
        qb.build().execute(&mut **tx).await?;
    }

    // Employees one level at a time: the path trigger reads the manager's path, so
    // every manager must already be in the table. `path` is never written here.
    for level in 1..=7i16 {
        let rows: Vec<&Emp> = org.employees.iter().filter(|e| e.level == level).collect();
        for part in rows.chunks(CHUNK) {
            let mut qb = QueryBuilder::new(
                "insert into employees (id, employee_no, first_name, last_name, email, phone,
                    position_id, department_id, manager_id, status, employment_type, hire_date,
                    termination_date, site, pay_grade, base_salary) ",
            );
            qb.push_values(part, |mut b, row| {
                b.push_bind(row.id)
                    .push_bind(row.employee_no.clone())
                    .push_bind(row.first_name.clone())
                    .push_bind(row.last_name.clone())
                    .push_bind(row.email.clone())
                    .push_bind(row.phone.clone())
                    .push_bind(row.position_id)
                    .push_bind(row.department_id)
                    .push_bind(row.manager_id)
                    .push_bind(row.status)
                    .push_bind(row.employment_type)
                    .push_bind(row.hire_date)
                    .push_bind(row.termination_date)
                    .push_bind(row.site)
                    .push_bind(row.pay_grade.clone())
                    .push_bind(row.base_salary);
            });
            qb.build().execute(&mut **tx).await?;
        }
        tracing::debug!(level, count = rows.len(), "employees written");
    }

    for part in org.users.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into users (id, employee_id, email, password_hash, status,
                must_change_password, last_login_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.employee_id)
                .push_bind(row.email.clone())
                .push_bind(password_hash.to_string())
                .push_bind(row.status)
                .push_bind(row.must_change_password)
                .push_bind(row.last_login_at);
        });
        qb.build().execute(&mut **tx).await?;
    }

    let mut grants: Vec<(Uuid, i16)> = Vec::new();
    for (user, employee) in org.users.iter().zip(org.employees.iter()) {
        for role in &employee.roles {
            grants.push((user.id, reference.role(role)));
        }
    }
    for part in grants.chunks(CHUNK) {
        let mut qb = QueryBuilder::new("insert into user_roles (user_id, role_id) ");
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.0).push_bind(row.1);
        });
        qb.build().execute(&mut **tx).await?;
    }
    Ok(())
}

async fn write_ops(tx: &mut Transaction<'_, Postgres>, ops: &Ops) -> Result<()> {
    for part in ops.sites.chunks(CHUNK) {
        let mut qb =
            QueryBuilder::new("insert into sites (id, code, name, kind, address, manager_id) ");
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.code)
                .push_bind(row.name)
                .push_bind(row.kind)
                .push_bind(row.address.clone())
                .push_bind(row.manager_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.carriers.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into carriers (id, code, name, mode, scac, contact, on_time_rate) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.code)
                .push_bind(row.name)
                .push_bind(row.mode)
                .push_bind(row.scac)
                .push_bind(row.contact.clone())
                .push_bind(row.on_time_rate);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.vehicles.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into vehicles (id, plate, kind, capacity_kg, status, home_site_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.plate.clone())
                .push_bind(row.kind)
                .push_bind(row.capacity_kg)
                .push_bind(row.status)
                .push_bind(row.home_site_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.customers.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into customers (id, code, name, contact_name, contact_email, phone,
                billing_address, credit_limit, status, account_manager_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.code.clone())
                .push_bind(row.name.clone())
                .push_bind(row.contact_name.clone())
                .push_bind(row.contact_email.clone())
                .push_bind(row.phone.clone())
                .push_bind(row.billing_address.clone())
                .push_bind(row.credit_limit)
                .push_bind(row.status)
                .push_bind(row.account_manager_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.vendors.chunks(CHUNK) {
        let mut qb = QueryBuilder::new("insert into vendors (id, code, name, contact) ");
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.code.clone())
                .push_bind(row.name)
                .push_bind(row.contact.clone());
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.shipments.chunks(200) {
        let mut qb = QueryBuilder::new(
            "insert into shipments (id, reference, customer_id, mode, incoterm, origin,
                destination, cargo_description, pieces, weight_kg, volume_cbm, hazardous,
                declared_value, status, previous_status, etd, eta, delivered_at, delay_risk,
                owner_id, created_by) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.reference.clone())
                .push_bind(row.customer_id)
                .push_bind(row.mode)
                .push_bind(row.incoterm)
                .push_bind(row.origin.clone())
                .push_bind(row.destination.clone())
                .push_bind(row.cargo_description)
                .push_bind(row.pieces)
                .push_bind(row.weight_kg)
                .push_bind(row.volume_cbm)
                .push_bind(row.hazardous)
                .push_bind(row.declared_value)
                .push_bind(row.status)
                .push_bind(row.previous_status)
                .push_bind(row.etd)
                .push_bind(row.eta)
                .push_bind(row.delivered_at)
                .push_bind(row.delay_risk)
                .push_bind(row.owner_id)
                .push_bind(row.owner_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.legs.chunks(200) {
        let mut qb = QueryBuilder::new(
            "insert into shipment_legs (id, shipment_id, seq, mode, carrier_id, vehicle_id,
                driver_id, from_location, to_location, planned_departure, planned_arrival,
                actual_departure, actual_arrival, status) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.shipment_id)
                .push_bind(row.seq)
                .push_bind(row.mode)
                .push_bind(row.carrier_id)
                .push_bind(row.vehicle_id)
                .push_bind(row.driver_id)
                .push_bind(row.from_location.clone())
                .push_bind(row.to_location.clone())
                .push_bind(row.planned_departure)
                .push_bind(row.planned_arrival)
                .push_bind(row.actual_departure)
                .push_bind(row.actual_arrival)
                .push_bind(row.status);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.events.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into shipment_events (id, shipment_id, event_type, occurred_at, location,
                note, recorded_by) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.shipment_id)
                .push_bind(row.event_type)
                .push_bind(row.occurred_at)
                .push_bind(row.location.clone())
                .push_bind(row.note.clone())
                .push_bind(row.recorded_by);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.documents.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into shipment_documents (id, shipment_id, kind, title, s3_key, mime_type,
                size_bytes, uploaded_by) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.parent_id)
                .push_bind(row.kind)
                .push_bind(row.title.clone())
                .push_bind(row.s3_key.clone())
                .push_bind(row.mime_type)
                .push_bind(row.size_bytes)
                .push_bind(row.uploaded_by);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.work_orders.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into work_orders (id, shipment_id, site_id, kind, title, instructions,
                assigned_to, assigned_by, status, due_at, started_at, completed_at, notes) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.shipment_id)
                .push_bind(row.site_id)
                .push_bind(row.kind)
                .push_bind(row.title.clone())
                .push_bind(row.instructions.clone())
                .push_bind(row.assigned_to)
                .push_bind(row.assigned_by)
                .push_bind(row.status)
                .push_bind(row.due_at)
                .push_bind(row.started_at)
                .push_bind(row.completed_at)
                .push_bind(row.notes);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in ops.inventory.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into inventory_items (id, site_id, shipment_id, description, quantity, bin,
                received_at, released_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.site_id)
                .push_bind(row.shipment_id)
                .push_bind(row.description.clone())
                .push_bind(row.quantity)
                .push_bind(row.bin.clone())
                .push_bind(row.received_at)
                .push_bind(row.released_at);
        });
        qb.build().execute(&mut **tx).await?;
    }
    Ok(())
}

async fn write_hr(tx: &mut Transaction<'_, Postgres>, hr: &Hr) -> Result<()> {
    for part in hr.balances.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into leave_balances (employee_id, year, type_key, allocated, used) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.employee_id)
                .push_bind(row.year)
                .push_bind(row.type_key)
                .push_bind(row.allocated)
                .push_bind(row.used);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in hr.requests.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into leave_requests (id, employee_id, type_key, start_date, end_date, days,
                reason, status, current_approver_id, decided_by, decided_at, decision_note) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.employee_id)
                .push_bind(row.type_key)
                .push_bind(row.start_date)
                .push_bind(row.end_date)
                .push_bind(row.days)
                .push_bind(row.reason)
                .push_bind(row.status)
                .push_bind(row.current_approver_id)
                .push_bind(row.decided_by)
                .push_bind(row.decided_at)
                .push_bind(row.decision_note);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in hr.shifts.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into shifts (id, employee_id, site, starts_at, ends_at, role_on_shift,
                status, created_by) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.employee_id)
                .push_bind(row.site)
                .push_bind(row.starts_at)
                .push_bind(row.ends_at)
                .push_bind(row.role_on_shift)
                .push_bind(row.status)
                .push_bind(row.created_by);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in hr.attendance.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into attendance (id, employee_id, shift_id, clock_in, clock_out, late, source) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.employee_id)
                .push_bind(row.shift_id)
                .push_bind(row.clock_in)
                .push_bind(row.clock_out)
                .push_bind(row.late)
                .push_bind(row.source);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in hr.documents.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into employee_documents (id, employee_id, kind, title, s3_key, mime_type,
                size_bytes, uploaded_by) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.parent_id)
                .push_bind(row.kind)
                .push_bind(row.title.clone())
                .push_bind(row.s3_key.clone())
                .push_bind(row.mime_type)
                .push_bind(row.size_bytes)
                .push_bind(row.uploaded_by);
        });
        qb.build().execute(&mut **tx).await?;
    }
    Ok(())
}

async fn write_finance(tx: &mut Transaction<'_, Postgres>, finance: &Finance) -> Result<()> {
    // Entries and their lines share this transaction: the balance trigger is deferred
    // and fires once, at commit.
    for part in finance.entries.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into journal_entries (id, period_id, entry_date, memo, source_type,
                source_id, posted_by, posted_at, reverses_entry_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.period_id)
                .push_bind(row.entry_date)
                .push_bind(row.memo.clone())
                .push_bind(row.source_type)
                .push_bind(row.source_id)
                .push_bind(row.posted_by)
                .push_bind(row.posted_at)
                .push_bind(row.reverses_entry_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in finance.lines.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into journal_lines (id, entry_id, account_id, debit, credit, description) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.entry_id)
                .push_bind(row.account_id)
                .push_bind(row.debit)
                .push_bind(row.credit)
                .push_bind(row.description.clone());
        });
        qb.build().execute(&mut **tx).await?;
    }

    // Only the reversal link may be updated on a posted entry.
    for (original, reversal) in &finance.reversals {
        sqlx::query("update journal_entries set reversed_by_entry_id = $1 where id = $2")
            .bind(reversal)
            .bind(original)
            .execute(&mut **tx)
            .await?;
    }

    for part in finance.invoices.chunks(200) {
        let mut qb = QueryBuilder::new(
            "insert into invoices (id, invoice_no, customer_id, shipment_id, status, issue_date,
                due_date, subtotal, tax, total, amount_paid, notes, pdf_s3_key, created_by,
                approved_by, issued_by, journal_entry_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.invoice_no.clone())
                .push_bind(row.customer_id)
                .push_bind(row.shipment_id)
                .push_bind(row.status)
                .push_bind(row.issue_date)
                .push_bind(row.due_date)
                .push_bind(row.subtotal)
                .push_bind(row.tax)
                .push_bind(row.total)
                .push_bind(row.amount_paid)
                .push_bind(row.notes.clone())
                .push_bind(row.pdf_s3_key.clone())
                .push_bind(row.created_by)
                .push_bind(row.approved_by)
                .push_bind(row.issued_by)
                .push_bind(row.journal_entry_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in finance.invoice_lines.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into invoice_lines (id, invoice_id, seq, description, quantity, unit_price,
                tax_rate, amount) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.invoice_id)
                .push_bind(row.seq)
                .push_bind(row.description)
                .push_bind(row.quantity)
                .push_bind(row.unit_price)
                .push_bind(row.tax_rate)
                .push_bind(row.amount);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in finance.payments.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into payments (id, invoice_id, received_on, amount, method, reference,
                recorded_by, journal_entry_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.invoice_id)
                .push_bind(row.received_on)
                .push_bind(row.amount)
                .push_bind(row.method)
                .push_bind(row.reference.clone())
                .push_bind(row.recorded_by)
                .push_bind(row.journal_entry_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in finance.bills.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into vendor_bills (id, vendor_id, bill_no, expense_account_id, amount,
                received_on, due_on, status, approved_by, paid_on, journal_entry_id,
                payment_entry_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.vendor_id)
                .push_bind(row.bill_no.clone())
                .push_bind(row.expense_account_id)
                .push_bind(row.amount)
                .push_bind(row.received_on)
                .push_bind(row.due_on)
                .push_bind(row.status)
                .push_bind(row.approved_by)
                .push_bind(row.paid_on)
                .push_bind(row.journal_entry_id)
                .push_bind(row.payment_entry_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in finance.expenses.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into expenses (id, employee_id, department_id, category, expense_account_id,
                amount, incurred_on, description, receipt_s3_key, status, manager_approved_by,
                finance_approved_by, rejected_by, rejection_note, journal_entry_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.employee_id)
                .push_bind(row.department_id)
                .push_bind(row.category)
                .push_bind(row.expense_account_id)
                .push_bind(row.amount)
                .push_bind(row.incurred_on)
                .push_bind(row.description.clone())
                .push_bind(row.receipt_s3_key.clone())
                .push_bind(row.status)
                .push_bind(row.manager_approved_by)
                .push_bind(row.finance_approved_by)
                .push_bind(row.rejected_by)
                .push_bind(row.rejection_note)
                .push_bind(row.journal_entry_id);
        });
        qb.build().execute(&mut **tx).await?;
    }

    if let Some(run) = &finance.payroll_run {
        sqlx::query(
            "insert into payroll_runs (id, period_id, status, total_gross, total_deductions,
                total_net, created_by, approved_by, approved_at, posted_at, journal_entry_id)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(run.id)
        .bind(run.period_id)
        .bind(run.status)
        .bind(run.total_gross)
        .bind(run.total_deductions)
        .bind(run.total_net)
        .bind(run.created_by)
        .bind(run.approved_by)
        .bind(run.approved_at)
        .bind(run.posted_at)
        .bind(run.journal_entry_id)
        .execute(&mut **tx)
        .await?;

        for part in finance.payroll_items.chunks(CHUNK) {
            let mut qb = QueryBuilder::new(
                "insert into payroll_items (id, run_id, employee_id, gross, deductions, net) ",
            );
            qb.push_values(part, |mut b, row| {
                b.push_bind(row.id)
                    .push_bind(row.run_id)
                    .push_bind(row.employee_id)
                    .push_bind(row.gross)
                    .push_bind(row.deductions)
                    .push_bind(row.net);
            });
            qb.build().execute(&mut **tx).await?;
        }
    }
    Ok(())
}

async fn write_comms(tx: &mut Transaction<'_, Postgres>, comms: &Comms) -> Result<()> {
    for part in comms.threads.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into threads (id, kind, subject, created_by, audience, created_at,
                last_message_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.kind)
                .push_bind(row.subject.clone())
                .push_bind(row.created_by)
                .push_bind(row.audience.clone())
                .push_bind(row.created_at)
                .push_bind(row.last_message_at);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in comms.participants.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into thread_participants (thread_id, employee_id, role, last_read_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.thread_id)
                .push_bind(row.employee_id)
                .push_bind(row.role)
                .push_bind(row.last_read_at);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in comms.messages.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into messages (id, thread_id, sender_id, body, importance, sent_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.thread_id)
                .push_bind(row.sender_id)
                .push_bind(row.body.clone())
                .push_bind(row.importance)
                .push_bind(row.sent_at);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in comms.tickets.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into support_tickets (id, ticket_no, thread_id, requester_id, category,
                priority, status, assignee_id, sla_due_at, first_response_at, resolved_at,
                closed_at, satisfaction, created_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.ticket_no.clone())
                .push_bind(row.thread_id)
                .push_bind(row.requester_id)
                .push_bind(row.category)
                .push_bind(row.priority)
                .push_bind(row.status)
                .push_bind(row.assignee_id)
                .push_bind(row.sla_due_at)
                .push_bind(row.first_response_at)
                .push_bind(row.resolved_at)
                .push_bind(row.closed_at)
                .push_bind(row.satisfaction)
                .push_bind(row.created_at);
        });
        qb.build().execute(&mut **tx).await?;
    }
    Ok(())
}

async fn write_platform(
    tx: &mut Transaction<'_, Postgres>,
    notifications: &[NotificationRow],
    audit: &[AuditRow],
) -> Result<()> {
    for part in notifications.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into notifications (id, recipient_id, channel, to_address, subject,
                body_text, status, attempts, created_at, sent_at) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.id)
                .push_bind(row.recipient_id)
                .push_bind("email")
                .push_bind(row.to_address.clone())
                .push_bind(row.subject.clone())
                .push_bind(row.body_text.clone())
                .push_bind(row.status)
                .push_bind(row.attempts)
                .push_bind(row.created_at)
                .push_bind(row.sent_at);
        });
        qb.build().execute(&mut **tx).await?;
    }

    for part in audit.chunks(CHUNK) {
        let mut qb = QueryBuilder::new(
            "insert into audit_log (at, actor_user_id, actor_employee_id, action, entity_type,
                entity_id, after, ip, request_id) ",
        );
        qb.push_values(part, |mut b, row| {
            b.push_bind(row.at)
                .push_bind(row.actor_user_id)
                .push_bind(row.actor_employee_id)
                .push_bind(row.action)
                .push_bind(row.entity_type)
                .push_bind(row.entity_id)
                .push_bind(row.after.clone())
                .push_bind(row.ip.clone())
                .push_unseparated("::inet")
                .push_bind("seed");
        });
        qb.build().execute(&mut **tx).await?;
    }
    Ok(())
}

/// The seeder writes human readable references itself, so the sequences behind them
/// have to catch up before the API issues the next one.
async fn advance_sequences(
    tx: &mut Transaction<'_, Postgres>,
    ops: &Ops,
    finance: &Finance,
    comms: &Comms,
) -> Result<()> {
    for (sequence, used) in [
        ("shipment_ref_seq", ops.shipments.len()),
        ("invoice_no_seq", finance.invoices.len()),
        ("ticket_no_seq", comms.tickets.len()),
    ] {
        if used == 0 {
            continue;
        }
        sqlx::query("select setval($1, $2)")
            .bind(sequence)
            .bind(used as i64)
            .execute(&mut **tx)
            .await
            .with_context(|| format!("advancing {sequence}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

/// Sections of the closing summary: heading, then label and table per line.
const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Organisation",
        &[
            ("departments", "departments"),
            ("positions", "positions"),
            ("employees", "employees"),
            ("users", "users"),
            ("role grants", "user_roles"),
        ],
    ),
    (
        "Operations",
        &[
            ("sites", "sites"),
            ("carriers", "carriers"),
            ("vehicles", "vehicles"),
            ("customers", "customers"),
            ("vendors", "vendors"),
            ("shipments", "shipments"),
            ("shipment legs", "shipment_legs"),
            ("tracking events", "shipment_events"),
            ("shipment documents", "shipment_documents"),
            ("work orders", "work_orders"),
            ("inventory items", "inventory_items"),
        ],
    ),
    (
        "People and HR",
        &[
            ("leave balances", "leave_balances"),
            ("leave requests", "leave_requests"),
            ("shifts", "shifts"),
            ("attendance", "attendance"),
            ("employee documents", "employee_documents"),
        ],
    ),
    (
        "Finance",
        &[
            ("journal entries", "journal_entries"),
            ("journal lines", "journal_lines"),
            ("invoices", "invoices"),
            ("invoice lines", "invoice_lines"),
            ("payments", "payments"),
            ("vendor bills", "vendor_bills"),
            ("expense claims", "expenses"),
            ("payroll runs", "payroll_runs"),
            ("payroll items", "payroll_items"),
        ],
    ),
    (
        "Communications",
        &[
            ("threads", "threads"),
            ("thread participants", "thread_participants"),
            ("messages", "messages"),
            ("support tickets", "support_tickets"),
        ],
    ),
    (
        "Platform",
        &[
            ("notifications", "notifications"),
            ("audit log entries", "audit_log"),
        ],
    ),
];

/// The logins the README, the smoke test and the UI walkthrough rely on.
const WELL_KNOWN: &[&str] = &[
    "ceo",
    "coo",
    "cfo",
    "chro",
    "cto",
    "cco",
    "director.finance",
    "manager.billing",
    "manager.warehouse",
    "supervisor.dock",
    "dispatcher",
    "accountant",
    "hr.admin",
    "support.agent",
    "it.admin",
    "driver",
    "dock.worker",
];

struct Report {
    counts: HashMap<String, i64>,
    ledger_balance: Decimal,
    without_manager: i64,
    depth: i32,
    logins: Vec<(String, String, String)>,
}

impl Report {
    async fn load(pool: &PgPool, org: &Org) -> Result<Report> {
        // Table names come from the SECTIONS table in this file, never from input.
        let query = SECTIONS
            .iter()
            .flat_map(|(_, tables)| tables.iter())
            .map(|(_, table)| {
                format!("select '{table}'::text as area, count(*)::bigint as rows from {table}")
            })
            .collect::<Vec<_>>()
            .join(" union all ");
        let counts: Vec<(String, i64)> = sqlx::query_as(&query)
            .fetch_all(pool)
            .await
            .context("counting seeded rows")?;
        let ledger_balance: Decimal =
            sqlx::query_scalar("select coalesce(sum(balance), 0) from trial_balance")
                .fetch_one(pool)
                .await
                .context("reading the trial balance")?;
        let without_manager: i64 =
            sqlx::query_scalar("select count(*) from employees where manager_id is null")
                .fetch_one(pool)
                .await?;
        let depth: i32 = sqlx::query_scalar("select coalesce(max(nlevel(path)), 0) from employees")
            .fetch_one(pool)
            .await?;
        let logins = WELL_KNOWN
            .iter()
            .map(|local| {
                let employee = org.login(local);
                (
                    employee.email.clone(),
                    employee.title.to_string(),
                    employee.roles.join(", "),
                )
            })
            .collect();
        Ok(Report {
            counts: counts.into_iter().collect(),
            ledger_balance,
            without_manager,
            depth,
            logins,
        })
    }

    fn print(&self, config: &Config) {
        println!("\nBowline Logistics demo data is loaded.\n");
        for (heading, tables) in SECTIONS {
            println!("  {heading}");
            for (label, table) in *tables {
                let count = self.counts.get(*table).copied().unwrap_or_default();
                println!("    {label:.<28} {count:>7}");
            }
        }
        println!("\n  Integrity");
        println!(
            "    {:.<28} {:>7}",
            "trial balance",
            format!("{:.2}", self.ledger_balance)
        );
        println!(
            "    {:.<28} {:>7}",
            "employees without a manager", self.without_manager
        );
        println!("    {:.<28} {:>7}", "deepest reporting chain", self.depth);

        println!("\n  Well known logins, password {}", config.seed.password);
        for (email, title, roles) in &self.logins {
            println!("    {email:<32} {title:<34} {roles}");
        }
        println!(
            "\n  Every other account signs in with the same password. Emails follow\n  \
             firstname.lastname@{EMAIL_DOMAIN}.\n"
        );
    }
}
