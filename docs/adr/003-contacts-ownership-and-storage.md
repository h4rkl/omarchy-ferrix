# ADR-003: Contacts ownership, storage, migrations, export

Status: accepted by the lead 2026-09-21.

## Decision

Contacts are a radio-independent Ferrix-owned service, `ferrix-contactsd`,
with contracts `ferrix-contacts-api` / `ferrix-contacts-protocol`. The
primary database is never a SIM or modem phonebook; SIM import is a possible
later adapter and is not in scope.

## Contract (schema 1)

- `ContactId`: opaque, stable, `c_` + 16 lowercase hex from a CSPRNG at
  creation; never reused.
- `Contact { id, display_name: String, given: Option<String>, family: Option<String>, numbers: Vec<PhoneNumber>, created_ms, updated_ms }`.
- `PhoneNumber { label: mobile | home | work | other, raw: String, e164: Option<String> }`.
  Normalization to E.164 through the `phonenumber` crate with a configurable
  default region (`FERRIX_DEFAULT_REGION`, default `US` in the mock profile);
  a number that cannot be normalized keeps `raw` and `e164: None` and is
  matched by digit suffix as a fallback.
- Operations: `create`, `update`, `delete`, `get`, `list`, `search { query }`
  (case-insensitive name and digit-substring), `lookup { number }` (E.164 match
  first, then a trailing 7-digit match only when unique), `export-vcard`,
  `import-vcard`.
- Capabilities: `contacts-read` (get/list/search/lookup/export) and
  `contacts-write` (create/update/delete/import).
- Events: `contact-changed { id, kind: created | updated | deleted }`, `service-restarted`.

## Storage

A single versioned JSON document `/var/lib/ferrix/contacts/contacts.json`
(`FERRIX_CONTACTS_DIR` override), `{ "schema_version": N, "contacts": [...] }`.
Writes are atomic: serialize to `contacts.json.tmp`, `fsync`, rename over the
target, `fsync` the directory. A corrupt or unparsable file remains authoritative,
is copied to `contacts.json.corrupt-<unix-ms>` for diagnosis, and causes the
daemon to fail startup. It never silently presents an empty address book or
loads fixtures over damaged user data. The startup failure record contains no
contact data. Migrations
are explicit functions `v1 -> v2 -> ...` with fixtures under `tests/fixtures/`
and a test per step; an unknown newer version refuses to start with a typed
error rather than rewriting data.

vCard: export to vCard 4.0 (`FN`, `N`, `TEL;TYPE=...`, `UID`), import of
`FN`/`N`/`TEL` from 3.0 or 4.0 with unknown properties ignored. This is a
small hand-rolled subset (documented fit: the maintained Rust vCard crates
are either 3.0-only or unmaintained; the subset is under 300 lines and fully
tested). Exports are written only under user-chosen or ignored output paths.

Deterministic fixtures use synthetic `+1555010xxxx` numbers and names like
"Test Contact 01"; none correspond to real people.
