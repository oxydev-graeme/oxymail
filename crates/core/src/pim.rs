//! Calendars and contacts, stored locally. Filled by importing iCalendar (.ics) and vCard
//! (.vcf) files, such as the Mailbird export, and edited in the app.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use base64::Engine;
use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::Store;

pub(crate) const PIM_SCHEMA: &str = "
create table if not exists calendars (
    id integer primary key,
    name text not null,
    color text not null,
    visible integer not null default 1,
    source text unique
);
create table if not exists events (
    id integer primary key,
    calendar_id integer not null references calendars(id) on delete cascade,
    uid text not null,
    recurrence_id integer,
    summary text not null default '',
    description text,
    location text,
    start integer not null,
    end integer not null,
    all_day integer not null default 0,
    rrule text,
    exdates text,
    status text,
    organizer text,
    attendees text
);
create unique index if not exists events_identity on events(calendar_id, uid, coalesce(recurrence_id, -1));
create index if not exists events_span on events(start, end);
create table if not exists people (
    id integer primary key,
    full_name text not null,
    first_name text,
    last_name text,
    org text,
    title text,
    notes text,
    birthday text,
    photo text,
    groups text,
    updated integer not null default 0
);
create table if not exists person_fields (
    id integer primary key,
    person_id integer not null references people(id) on delete cascade,
    kind text not null,
    label text,
    value text not null
);
create index if not exists person_fields_person on person_fields(person_id);
";

/// Colours given to imported calendars, in order.
const PALETTE: &[&str] = &["#0e7490", "#7c3aed", "#c2410c", "#15803d", "#be123c", "#1d4ed8", "#a16207", "#0f766e", "#9d174d", "#4d7c0f"];

#[derive(Debug, Serialize)]
pub struct Calendar {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub visible: bool,
    pub events: i64,
}

/// One occurrence of an event in a date range (recurring events produce several).
#[derive(Debug, Serialize)]
pub struct Occurrence {
    pub event_id: i64,
    pub calendar_id: i64,
    pub color: String,
    pub summary: String,
    pub location: Option<String>,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub recurring: bool,
    pub cancelled: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Event {
    #[serde(default)]
    pub id: Option<i64>,
    pub calendar_id: i64,
    pub summary: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub rrule: Option<String>,
    #[serde(default)]
    pub organizer: Option<String>,
    /// `[{"name","email","status"}]`
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: String,
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PersonSummary {
    pub id: i64,
    pub full_name: String,
    pub org: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub photo: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Person {
    #[serde(default)]
    pub id: Option<i64>,
    pub full_name: String,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub birthday: Option<String>,
    #[serde(default)]
    pub photo: Option<String>,
    #[serde(default)]
    pub groups: Option<String>,
    #[serde(default)]
    pub fields: Vec<PersonField>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PersonField {
    /// `email`, `phone`, `url`, `address`, `im`
    pub kind: String,
    pub label: Option<String>,
    pub value: String,
}

#[derive(Debug, Default, Serialize)]
pub struct PimImport {
    pub calendars: usize,
    pub events: usize,
    pub people: usize,
}

impl Store {
    // ---- Calendars -------------------------------------------------------------

    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        let mut stmt = self.db.prepare(
            "select c.id, c.name, c.color, c.visible, (select count(*) from events e where e.calendar_id = c.id)
             from calendars c order by c.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Calendar { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, visible: r.get(3)?, events: r.get(4)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn create_calendar(&self, name: &str, color: Option<&str>) -> Result<i64> {
        let color = color.map(str::to_string).unwrap_or_else(|| self.next_color());
        self.db.execute("insert into calendars (name, color) values (?1, ?2)", params![name.trim(), color])?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn update_calendar(&self, id: i64, name: &str, color: &str, visible: bool) -> Result<()> {
        self.db.execute(
            "update calendars set name = ?1, color = ?2, visible = ?3 where id = ?4",
            params![name.trim(), color, visible, id],
        )?;
        Ok(())
    }

    pub fn delete_calendar(&self, id: i64) -> Result<()> {
        self.db.execute("delete from events where calendar_id = ?1", [id])?;
        self.db.execute("delete from calendars where id = ?1", [id])?;
        Ok(())
    }

    fn next_color(&self) -> String {
        let n: i64 = self.db.query_row("select count(*) from calendars", [], |r| r.get(0)).unwrap_or(0);
        PALETTE[n as usize % PALETTE.len()].to_string()
    }

    /// Every occurrence of every visible calendar's events overlapping `[from, to)`.
    pub fn occurrences(&self, from: i64, to: i64) -> Result<Vec<Occurrence>> {
        struct Row {
            id: i64,
            calendar_id: i64,
            color: String,
            uid: String,
            recurrence_id: Option<i64>,
            summary: String,
            location: Option<String>,
            start: i64,
            end: i64,
            all_day: bool,
            rrule: Option<String>,
            exdates: Option<String>,
            status: Option<String>,
        }
        let mut stmt = self.db.prepare(
            "select e.id, e.calendar_id, c.color, e.uid, e.recurrence_id, e.summary, e.location, e.start, e.end,
                    e.all_day, e.rrule, e.exdates, e.status
             from events e join calendars c on c.id = e.calendar_id
             where c.visible = 1 and (e.rrule is not null or (e.start < ?2 and e.end > ?1))",
        )?;
        let rows: Vec<Row> = stmt
            .query_map(params![from, to], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    calendar_id: r.get(1)?,
                    color: r.get(2)?,
                    uid: r.get(3)?,
                    recurrence_id: r.get(4)?,
                    summary: r.get(5)?,
                    location: r.get(6)?,
                    start: r.get(7)?,
                    end: r.get(8)?,
                    all_day: r.get(9)?,
                    rrule: r.get(10)?,
                    exdates: r.get(11)?,
                    status: r.get(12)?,
                })
            })?
            .collect::<Result<_, _>>()?;

        // Occurrences replaced by an exception (same UID, RECURRENCE-ID) are skipped.
        let overridden: HashSet<(i64, String, i64)> = self
            .db
            .prepare("select calendar_id, uid, recurrence_id from events where recurrence_id is not null")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;

        let mut out = Vec::new();
        for row in rows {
            let cancelled = row.status.as_deref() == Some("CANCELLED");
            let make = |start: i64, end: i64, recurring: bool| Occurrence {
                event_id: row.id,
                calendar_id: row.calendar_id,
                color: row.color.clone(),
                summary: row.summary.clone(),
                location: row.location.clone(),
                start,
                end,
                all_day: row.all_day,
                recurring,
                cancelled,
            };
            match (&row.rrule, row.recurrence_id) {
                (Some(rule), None) => {
                    let length = row.end - row.start;
                    let excluded: HashSet<i64> =
                        row.exdates.as_deref().unwrap_or("").split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    // A rule this parser can't read hides only that event, not the whole calendar.
                    let Ok(starts) = expand(rule, row.start, from - length, to) else { continue };
                    for start in starts {
                        if excluded.contains(&start) || overridden.contains(&(row.calendar_id, row.uid.clone(), start)) {
                            continue;
                        }
                        if start < to && start + length > from {
                            out.push(make(start, start + length, true));
                        }
                    }
                }
                _ if row.start < to && row.end > from => out.push(make(row.start, row.end, row.recurrence_id.is_some())),
                _ => {}
            }
        }
        out.sort_by_key(|o| (o.start, !o.all_day));
        Ok(out)
    }

    pub fn event(&self, id: i64) -> Result<Event> {
        Ok(self.db.query_row(
            "select id, calendar_id, summary, description, location, start, end, all_day, rrule, organizer, attendees, status
             from events where id = ?1",
            [id],
            |r| {
                Ok(Event {
                    id: r.get(0)?,
                    calendar_id: r.get(1)?,
                    summary: r.get(2)?,
                    description: r.get(3)?,
                    location: r.get(4)?,
                    start: r.get(5)?,
                    end: r.get(6)?,
                    all_day: r.get(7)?,
                    rrule: r.get(8)?,
                    organizer: r.get(9)?,
                    attendees: r
                        .get::<_, Option<String>>(10)?
                        .and_then(|a| serde_json::from_str(&a).ok())
                        .unwrap_or_default(),
                    status: r.get(11)?,
                })
            },
        )?)
    }

    /// Creates or updates an event. Editing a recurring event changes the whole series.
    pub fn save_event(&self, event: &Event) -> Result<i64> {
        if event.end < event.start {
            bail!("the event ends before it starts");
        }
        let attendees = serde_json::to_string(&event.attendees)?;
        let rrule = event.rrule.as_deref().map(str::trim).filter(|r| !r.is_empty());
        if let Some(rule) = rrule {
            expand(rule, event.start, event.start, event.start + 1).context("the repeat rule isn't valid")?;
        }
        match event.id {
            Some(id) => {
                self.db.execute(
                    "update events set calendar_id = ?1, summary = ?2, description = ?3, location = ?4, start = ?5,
                         end = ?6, all_day = ?7, rrule = ?8, attendees = ?9 where id = ?10",
                    params![event.calendar_id, event.summary, event.description, event.location, event.start, event.end, event.all_day, rrule, attendees, id],
                )?;
                Ok(id)
            }
            None => {
                let uid = format!("oxymail-{}-{}@oxymail", event.start, random_suffix());
                self.db.execute(
                    "insert into events (calendar_id, uid, summary, description, location, start, end, all_day, rrule, attendees)
                     values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![event.calendar_id, uid, event.summary, event.description, event.location, event.start, event.end, event.all_day, rrule, attendees],
                )?;
                Ok(self.db.last_insert_rowid())
            }
        }
    }

    pub fn delete_event(&self, id: i64) -> Result<()> {
        // Deleting a series also removes its exceptions.
        self.db.execute(
            "delete from events where id = ?1 or (recurrence_id is not null and (calendar_id, uid) =
                (select calendar_id, uid from events where id = ?1))",
            [id],
        )?;
        Ok(())
    }

    /// Imports a calendar file into a calendar of its own (re-importing the same file updates it).
    pub fn import_ics(&mut self, path: &Path) -> Result<(i64, usize)> {
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let lines = unfold(&text);
        let name = lines
            .iter()
            .find(|(n, _, _)| n == "X-WR-CALNAME")
            .map(|(_, _, v)| unescape(v))
            .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Calendar".into()));
        let source = path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_string();
        let existing: Option<i64> = self.db.query_row("select id from calendars where source = ?1", [&source], |r| r.get(0)).optional()?;
        let calendar_id = match existing {
            Some(id) => id,
            None => {
                let color = self.next_color();
                self.db.execute("insert into calendars (name, color, source) values (?1, ?2, ?3)", params![name, color, source])?;
                self.db.last_insert_rowid()
            }
        };

        let tx = self.db.transaction()?;
        let mut count = 0;
        let mut current: Option<HashMap<String, Vec<(HashMap<String, String>, String)>>> = None;
        for (name, params_, value) in lines {
            match (name.as_str(), value.as_str()) {
                ("BEGIN", "VEVENT") => current = Some(HashMap::new()),
                ("END", "VEVENT") => {
                    if let Some(props) = current.take() {
                        if store_ics_event(&tx, calendar_id, &props)? {
                            count += 1;
                        }
                    }
                }
                _ => {
                    if let Some(props) = current.as_mut() {
                        props.entry(name).or_default().push((params_, value));
                    }
                }
            }
        }
        tx.commit()?;
        Ok((calendar_id, count))
    }

    // ---- Contacts --------------------------------------------------------------

    /// People in the address book matching `query` (name, organisation, email or phone).
    pub fn people(&self, query: &str) -> Result<Vec<PersonSummary>> {
        let q = format!("%{}%", query.trim().to_lowercase());
        let mut stmt = self.db.prepare(
            "select p.id, p.full_name, p.org, p.photo,
                    (select value from person_fields f where f.person_id = p.id and f.kind = 'email' order by f.id limit 1),
                    (select value from person_fields f where f.person_id = p.id and f.kind = 'phone' order by f.id limit 1)
             from people p
             where ?1 = '%%' or lower(p.full_name) like ?1 or lower(coalesce(p.org, '')) like ?1
                or exists (select 1 from person_fields f where f.person_id = p.id and lower(f.value) like ?1)
             order by lower(p.full_name)",
        )?;
        let rows = stmt.query_map([q], |r| {
            Ok(PersonSummary { id: r.get(0)?, full_name: r.get(1)?, org: r.get(2)?, photo: r.get(3)?, email: r.get(4)?, phone: r.get(5)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn person(&self, id: i64) -> Result<Person> {
        let mut person = self.db.query_row(
            "select id, full_name, first_name, last_name, org, title, notes, birthday, photo, groups from people where id = ?1",
            [id],
            |r| {
                Ok(Person {
                    id: r.get(0)?,
                    full_name: r.get(1)?,
                    first_name: r.get(2)?,
                    last_name: r.get(3)?,
                    org: r.get(4)?,
                    title: r.get(5)?,
                    notes: r.get(6)?,
                    birthday: r.get(7)?,
                    photo: r.get(8)?,
                    groups: r.get(9)?,
                    fields: Vec::new(),
                })
            },
        )?;
        let mut stmt = self.db.prepare("select kind, label, value from person_fields where person_id = ?1 order by id")?;
        person.fields = stmt
            .query_map([id], |r| Ok(PersonField { kind: r.get(0)?, label: r.get(1)?, value: r.get(2)? }))?
            .collect::<Result<_, _>>()?;
        Ok(person)
    }

    pub fn save_person(&mut self, person: &Person) -> Result<i64> {
        let name = person.full_name.trim();
        if name.is_empty() {
            bail!("give the contact a name");
        }
        let tx = self.db.transaction()?;
        let id = upsert_person(&tx, person)?;
        tx.commit()?;
        Ok(id)
    }

    pub fn delete_person(&self, id: i64) -> Result<()> {
        self.db.execute("delete from person_fields where person_id = ?1", [id])?;
        self.db.execute("delete from people where id = ?1", [id])?;
        Ok(())
    }

    /// Imports a vCard file. People already present (same name and first email) are skipped.
    pub fn import_vcf(&mut self, path: &Path) -> Result<usize> {
        let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let tx = self.db.transaction()?;
        let mut count = 0;
        let mut card: Option<Vec<(String, HashMap<String, String>, String)>> = None;
        for (name, params_, value) in unfold(&text) {
            match (name.as_str(), value.to_ascii_uppercase().as_str()) {
                ("BEGIN", "VCARD") => card = Some(Vec::new()),
                ("END", "VCARD") => {
                    if let Some(props) = card.take() {
                        if let Some(person) = person_from_vcard(&props) {
                            let first_email = person.fields.iter().find(|f| f.kind == "email").map(|f| f.value.to_lowercase());
                            let duplicate: bool = tx.query_row(
                                "select count(*) > 0 from people p where p.full_name = ?1 and (?2 is null or exists
                                     (select 1 from person_fields f where f.person_id = p.id and lower(f.value) = ?2))",
                                params![person.full_name, first_email],
                                |r| r.get(0),
                            )?;
                            if !duplicate {
                                upsert_person(&tx, &person)?;
                                count += 1;
                            }
                        }
                    }
                }
                _ => {
                    if let Some(props) = card.as_mut() {
                        props.push((name, params_, value));
                    }
                }
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Imports every .ics and .vcf in a folder tree, such as `~/MailbirdExport`.
    pub fn import_pim_folder(&mut self, dir: &Path) -> Result<PimImport> {
        let mut stats = PimImport::default();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = fs::read_dir(&d) else { continue };
            let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.is_dir() {
                    // The mail tree is large and has no calendars or contacts in it.
                    if p.file_name().is_some_and(|n| n != "mail" && n != "raw") {
                        stack.push(p);
                    }
                    continue;
                }
                match p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                    Some("ics") => {
                        let (_, n) = self.import_ics(&p)?;
                        stats.calendars += 1;
                        stats.events += n;
                    }
                    Some("vcf") => stats.people += self.import_vcf(&p)?,
                    _ => {}
                }
            }
        }
        Ok(stats)
    }
}

fn upsert_person(tx: &rusqlite::Transaction, person: &Person) -> Result<i64> {
    let now = Utc::now().timestamp();
    let id = match person.id {
        Some(id) => {
            tx.execute(
                "update people set full_name = ?1, first_name = ?2, last_name = ?3, org = ?4, title = ?5, notes = ?6,
                     birthday = ?7, photo = ?8, groups = ?9, updated = ?10 where id = ?11",
                params![person.full_name.trim(), person.first_name, person.last_name, person.org, person.title, person.notes, person.birthday, person.photo, person.groups, now, id],
            )?;
            tx.execute("delete from person_fields where person_id = ?1", [id])?;
            id
        }
        None => {
            tx.execute(
                "insert into people (full_name, first_name, last_name, org, title, notes, birthday, photo, groups, updated)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![person.full_name.trim(), person.first_name, person.last_name, person.org, person.title, person.notes, person.birthday, person.photo, person.groups, now],
            )?;
            tx.last_insert_rowid()
        }
    };
    for f in person.fields.iter().filter(|f| !f.value.trim().is_empty()) {
        tx.execute(
            "insert into person_fields (person_id, kind, label, value) values (?1, ?2, ?3, ?4)",
            params![id, f.kind, f.label, f.value.trim()],
        )?;
        if f.kind == "email" {
            // Address-book people rank first in compose suggestions.
            crate::note_contact(tx, f.value.trim(), Some(person.full_name.trim()), 0, 0)?;
            tx.execute("update contacts set in_address_book = 1 where email = lower(?1)", [f.value.trim()])?;
        }
    }
    Ok(id)
}

/// Occurrence start times of `rule` for an event starting at `start`, within `[from, to)`.
fn expand(rule: &str, start: i64, from: i64, to: i64) -> Result<Vec<i64>> {
    let dtstart = Utc.timestamp_opt(start, 0).single().context("bad start time")?;
    let mut text = format!("DTSTART:{}\n", dtstart.format("%Y%m%dT%H%M%SZ"));
    for line in rule.lines().map(str::trim).filter(|l| !l.is_empty()).map(normalize_until) {
        if line.starts_with("RRULE:") || line.starts_with("EXRULE:") || line.starts_with("RDATE") || line.starts_with("EXDATE") {
            text.push_str(&line);
        } else {
            text.push_str("RRULE:");
            text.push_str(&line);
        }
        text.push('\n');
    }
    let set: rrule::RRuleSet = text.trim_end().parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    let utc = rrule::Tz::UTC;
    let after = utc.timestamp_opt(from.max(start) - 1, 0).single().context("bad range")?;
    let before = utc.timestamp_opt(to, 0).single().context("bad range")?;
    Ok(set.after(after).before(before).all(2000).dates.into_iter().map(|d| d.timestamp()).collect())
}

/// The rrule crate needs UNTIL in UTC when DTSTART is UTC; many calendars write a plain date
/// (`UNTIL=20250322`) or a floating time. Both are read as the end of that day / that time in UTC.
fn normalize_until(line: &str) -> String {
    let Some(pos) = line.find("UNTIL=") else { return line.to_string() };
    let value_start = pos + "UNTIL=".len();
    let value_end = line[value_start..].find(';').map(|i| value_start + i).unwrap_or(line.len());
    let value = &line[value_start..value_end];
    let fixed = if value.ends_with('Z') {
        value.to_string()
    } else if value.len() == 8 {
        format!("{value}T235959Z")
    } else {
        format!("{value}Z")
    };
    format!("{}{}{}", &line[..value_start], fixed, &line[value_end..])
}

/// Splits iCalendar/vCard text into `(NAME, params, value)` with folded lines joined.
fn unfold(text: &str) -> Vec<(String, HashMap<String, String>, String)> {
    let mut joined: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !joined.is_empty() {
            joined.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            joined.push(line.to_string());
        }
    }
    joined
        .into_iter()
        .filter_map(|line| {
            // The value starts at the first ':' outside a quoted parameter.
            let mut quoted = false;
            let split = line.char_indices().find(|(_, c)| {
                if *c == '"' {
                    quoted = !quoted;
                }
                *c == ':' && !quoted
            })?;
            let (head, value) = (&line[..split.0], &line[split.0 + 1..]);
            let mut parts = head.split(';');
            let name = parts.next()?.to_ascii_uppercase();
            // Drop vCard group prefixes such as "item1.EMAIL".
            let name = name.rsplit('.').next().unwrap_or(&name).to_string();
            let params_ = parts
                .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string())))
                .collect();
            Some((name, params_, value.to_string()))
        })
        .collect()
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// An iCalendar date or date-time as `(unix seconds, is_date_only)`. TZID times are converted
/// with the named zone; floating times are read as local time.
fn ics_time(value: &str, params_: &HashMap<String, String>) -> Option<(i64, bool)> {
    let v = value.trim();
    if params_.get("VALUE").map(String::as_str) == Some("DATE") || v.len() == 8 {
        let d = NaiveDate::parse_from_str(&v[..8], "%Y%m%d").ok()?;
        return Some((d.and_hms_opt(0, 0, 0)?.and_utc().timestamp(), true));
    }
    let naive = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    if v.ends_with('Z') {
        return Some((naive.and_utc().timestamp(), false));
    }
    if let Some(tz) = params_.get("TZID").and_then(|t| t.parse::<chrono_tz::Tz>().ok()) {
        return Some((tz.from_local_datetime(&naive).earliest()?.timestamp(), false));
    }
    Some((chrono::Local.from_local_datetime(&naive).earliest()?.timestamp(), false))
}

/// `PT1H30M`, `P1D`, `P1W` → seconds.
fn ics_duration(value: &str) -> Option<i64> {
    let (mut total, mut number, mut in_time) = (0i64, 0i64, false);
    for c in value.trim().trim_start_matches(['+', '-']).chars() {
        match c {
            'P' => {}
            'T' => in_time = true,
            '0'..='9' => number = number * 10 + c.to_digit(10)? as i64,
            'W' => (total, number) = (total + number * 604_800, 0),
            'D' => (total, number) = (total + number * 86_400, 0),
            'H' if in_time => (total, number) = (total + number * 3_600, 0),
            'M' if in_time => (total, number) = (total + number * 60, 0),
            'S' if in_time => (total, number) = (total + number, 0),
            _ => return None,
        }
    }
    Some(total)
}

fn store_ics_event(
    tx: &rusqlite::Transaction,
    calendar_id: i64,
    props: &HashMap<String, Vec<(HashMap<String, String>, String)>>,
) -> Result<bool> {
    let first = |name: &str| props.get(name).and_then(|v| v.first());
    let text = |name: &str| first(name).map(|(_, v)| unescape(v)).filter(|t| !t.trim().is_empty());
    let Some((start, all_day)) = first("DTSTART").and_then(|(p, v)| ics_time(v, p)) else { return Ok(false) };
    let end = first("DTEND")
        .and_then(|(p, v)| ics_time(v, p))
        .map(|(t, _)| t)
        .or_else(|| first("DURATION").and_then(|(_, v)| ics_duration(v)).map(|d| start + d))
        .unwrap_or(if all_day { start + 86_400 } else { start + 3_600 })
        .max(start);
    let uid = text("UID").unwrap_or_else(|| format!("import-{start}-{}", random_suffix()));
    let recurrence_id = first("RECURRENCE-ID").and_then(|(p, v)| ics_time(v, p)).map(|(t, _)| t);
    let rrule = props.get("RRULE").map(|rules| rules.iter().map(|(_, v)| format!("RRULE:{v}")).collect::<Vec<_>>().join("\n"));
    let exdates: Vec<String> = props
        .get("EXDATE")
        .into_iter()
        .flatten()
        .flat_map(|(p, v)| v.split(',').filter_map(|d| ics_time(d, p)).map(|(t, _)| t.to_string()).collect::<Vec<_>>())
        .collect();
    let mail = |v: &str| v.trim_start_matches("mailto:").trim_start_matches("MAILTO:").to_string();
    let attendees: Vec<Attendee> = props
        .get("ATTENDEE")
        .into_iter()
        .flatten()
        .map(|(p, v)| Attendee { name: p.get("CN").cloned(), email: mail(v), status: p.get("PARTSTAT").cloned() })
        .collect();
    let organizer = first("ORGANIZER").map(|(p, v)| match p.get("CN") {
        Some(cn) => format!("{cn} <{}>", mail(v)),
        None => mail(v),
    });
    tx.execute(
        "insert into events (calendar_id, uid, recurrence_id, summary, description, location, start, end, all_day,
             rrule, exdates, status, organizer, attendees)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         on conflict (calendar_id, uid, coalesce(recurrence_id, -1)) do update set
             summary = excluded.summary, description = excluded.description, location = excluded.location,
             start = excluded.start, end = excluded.end, all_day = excluded.all_day, rrule = excluded.rrule,
             exdates = excluded.exdates, status = excluded.status, organizer = excluded.organizer,
             attendees = excluded.attendees",
        params![
            calendar_id,
            uid,
            recurrence_id,
            text("SUMMARY").unwrap_or_default(),
            text("DESCRIPTION"),
            text("LOCATION"),
            start,
            end,
            all_day,
            rrule,
            (!exdates.is_empty()).then(|| exdates.join(",")),
            text("STATUS").map(|s| s.to_uppercase()),
            organizer,
            serde_json::to_string(&attendees)?,
        ],
    )?;
    Ok(true)
}

fn person_from_vcard(props: &[(String, HashMap<String, String>, String)]) -> Option<Person> {
    let get = |name: &str| props.iter().find(|(n, _, _)| n == name).map(|(_, p, v)| (p, v.as_str()));
    let label = |p: &HashMap<String, String>| {
        p.get("TYPE")
            .map(|t| t.split(',').filter(|x| !x.eq_ignore_ascii_case("INTERNET") && !x.eq_ignore_ascii_case("PREF")).collect::<Vec<_>>().join(", "))
            .filter(|t| !t.is_empty())
            .map(|t| {
                let mut c = t.to_lowercase().chars().collect::<Vec<_>>();
                if let Some(f) = c.first_mut() {
                    *f = f.to_ascii_uppercase();
                }
                c.into_iter().collect::<String>()
            })
    };
    let n: Vec<String> = get("N").map(|(_, v)| v.split(';').map(unescape).collect()).unwrap_or_default();
    let last = n.first().cloned().filter(|s| !s.is_empty());
    let first = n.get(1).cloned().filter(|s| !s.is_empty());
    let mut fields = Vec::new();
    for (name, p, v) in props {
        let kind = match name.as_str() {
            "EMAIL" => "email",
            "TEL" => "phone",
            "URL" => "url",
            "IMPP" | "X-IM" => "im",
            "ADR" => "address",
            _ => continue,
        };
        let value = if kind == "address" {
            v.split(';').map(unescape).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
        } else {
            unescape(v)
        };
        fields.push(PersonField { kind: kind.into(), label: label(p), value });
    }
    let full_name = get("FN")
        .map(|(_, v)| unescape(v).trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| Some([first.clone(), last.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty()))
        .or_else(|| fields.iter().find(|f| f.kind == "email").map(|f| f.value.clone()))?;
    let photo = get("PHOTO").and_then(|(p, v)| {
        if v.starts_with("data:") {
            return Some(v.to_string());
        }
        let encoded = p.get("ENCODING").is_some_and(|e| e.eq_ignore_ascii_case("b") || e.eq_ignore_ascii_case("BASE64"));
        let kind = p.get("TYPE").map(|t| t.to_lowercase()).unwrap_or_else(|| "jpeg".into());
        (encoded && base64::engine::general_purpose::STANDARD.decode(v.trim()).is_ok()).then(|| format!("data:image/{kind};base64,{}", v.trim()))
    });
    Some(Person {
        id: None,
        full_name,
        first_name: first,
        last_name: last,
        org: get("ORG").map(|(_, v)| unescape(v).replace(';', " ").trim().to_string()).filter(|s| !s.is_empty()),
        title: get("TITLE").map(|(_, v)| unescape(v)).filter(|s| !s.is_empty()),
        notes: {
            let notes: Vec<String> = props.iter().filter(|(n, _, _)| n == "NOTE").map(|(_, _, v)| unescape(v)).collect();
            (!notes.is_empty()).then(|| notes.join("\n"))
        },
        birthday: get("BDAY").map(|(_, v)| v.trim().to_string()),
        photo,
        groups: get("CATEGORIES").map(|(_, v)| unescape(v)),
        fields,
    })
}

fn random_suffix() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_and_expands_a_recurring_calendar() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("oxymail-pim-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        let ics = dir.join("cal.ics");
        fs::write(
            &ics,
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Family\r\nBEGIN:VEVENT\r\nUID:swim@x\r\nDTSTART:20250106T080000Z\r\nDTEND:20250106T090000Z\r\n\
             SUMMARY:Swimming\\, lessons\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nEXDATE:20250113T080000Z\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:swim@x\r\nRECURRENCE-ID:20250120T080000Z\r\nDTSTART:20250120T100000Z\r\nDTEND:20250120T110000Z\r\n\
             SUMMARY:Swimming (moved)\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:hol@x\r\nDTSTART;VALUE=DATE:20250101\r\n\
             DTEND;VALUE=DATE:20250102\r\nSUMMARY:New Year\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )?;
        let mut store = Store::open(&dir.join("t.db"))?;
        let (_, n) = store.import_ics(&ics)?;
        assert_eq!(n, 3);
        assert_eq!(store.calendars()?[0].name, "Family");
        let jan = store.occurrences(1735689600, 1738368000)?; // January 2025
        let titles: Vec<&str> = jan.iter().map(|o| o.summary.as_str()).collect();
        // 4 weekly: 6th, 13th (excluded), 20th (moved), 27th → 6th, 27th + the moved one + New Year.
        assert_eq!(titles, ["New Year", "Swimming, lessons", "Swimming (moved)", "Swimming, lessons"]);
        assert!(jan[0].all_day);
        assert_eq!(store.import_ics(&ics)?.1, 3, "re-import updates in place");
        assert_eq!(store.occurrences(1735689600, 1738368000)?.len(), 4);

        let vcf = dir.join("people.vcf");
        fs::write(
            &vcf,
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Paul McCree\r\nN:McCree;Paul;;;\r\nEMAIL;TYPE=INTERNET;TYPE=HOME:paul@sarongi.co.za\r\n\
             TEL;TYPE=CELL:082 575 6646\r\nORG:Sarongi\r\nEND:VCARD\r\n",
        )?;
        assert_eq!(store.import_vcf(&vcf)?, 1);
        assert_eq!(store.import_vcf(&vcf)?, 0, "no duplicates");
        let found = store.people("sarongi")?;
        assert_eq!(found[0].full_name, "Paul McCree");
        assert_eq!(found[0].email.as_deref(), Some("paul@sarongi.co.za"));
        let p = store.person(found[0].id)?;
        assert_eq!(p.fields.iter().find(|f| f.kind == "phone").and_then(|f| f.label.clone()).as_deref(), Some("Cell"));
        drop(store); // Windows won't delete a database that is still open.
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn date_only_until_is_accepted() {
        assert_eq!(normalize_until("RRULE:FREQ=YEARLY;UNTIL=20250322;WKST=TU"), "RRULE:FREQ=YEARLY;UNTIL=20250322T235959Z;WKST=TU");
        assert_eq!(expand("RRULE:FREQ=YEARLY;UNTIL=20250322", 1679443200, 1600000000, 1800000000).unwrap().len(), 3);
    }

    #[test]
    fn parses_durations() {
        assert_eq!(ics_duration("PT1H30M"), Some(5400));
        assert_eq!(ics_duration("P1D"), Some(86400));
        assert_eq!(ics_duration("P1W"), Some(604800));
    }
}
