//! Contacts and calendars from the Mailbird store, as vCard 3.0 and iCalendar files that any
//! address book or calendar app can import.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Result;
use base64::Engine;
use rusqlite::Connection;

#[derive(Debug, Default, serde::Serialize)]
pub struct ExtrasStats {
    pub contacts: usize,
    pub collected_addresses: usize,
    pub calendars: usize,
    pub events: usize,
}

pub fn export(db: &Connection, store: &Path, out: &Path) -> Result<ExtrasStats> {
    let mut stats = ExtrasStats::default();
    fs::create_dir_all(out.join("contacts"))?;
    fs::create_dir_all(out.join("calendar"))?;
    stats.contacts = contacts(db, store, &out.join("contacts/address-book.vcf"))?;
    stats.collected_addresses = collected(db, &out.join("contacts/collected-addresses.csv"))?;
    let (calendars, events) = calendars(db, &out.join("calendar"))?;
    stats.calendars = calendars;
    stats.events = events;
    Ok(stats)
}

fn contacts(db: &Connection, store: &Path, path: &Path) -> Result<usize> {
    let mut fields: HashMap<i64, Vec<(i64, Option<String>, String)>> = HashMap::new();
    for row in db
        .prepare("select UserContactId, Category, CustomLabel, Value from UserContactFields where Value is not null")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?)))?
    {
        let (contact, category, label, value) = row?;
        fields.entry(contact).or_default().push((category, label, value));
    }
    let mut addresses: HashMap<i64, Vec<[Option<String>; 5]>> = HashMap::new();
    for row in db
        .prepare("select UserContactId, Street, City, Region, Postcode, Country from UserContactPostalAddresses")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?])))?
    {
        let (contact, parts) = row?;
        addresses.entry(contact).or_default().push(parts);
    }
    let mut groups: HashMap<i64, Vec<String>> = HashMap::new();
    for row in db
        .prepare(
            "select gu.UserContactId, g.Name from UserContactGroups_UserContacts gu
             join UserContactGroups g on g.Id = gu.UserContactGroupId where g.Name is not null",
        )?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
    {
        let (contact, name) = row?;
        groups.entry(contact).or_default().push(name);
    }

    let mut out = String::new();
    let mut count = 0;
    let mut stmt = db.prepare(
        "select Id, FirstName, MiddleName, LastName, Nickname, NamePrefix, NameSuffix from UserContacts",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let name: [Option<String>; 6] = [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?];
        let [first, middle, last, nick, prefix, suffix] = name.map(|n| n.unwrap_or_default().trim().to_string());
        let mine = fields.get(&id).map(Vec::as_slice).unwrap_or(&[]);
        let full = [prefix.as_str(), first.as_str(), middle.as_str(), last.as_str(), suffix.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let display = if !full.is_empty() {
            full
        } else if let Some((_, _, v)) = mine.iter().find(|(c, _, _)| *c == 1) {
            v.clone()
        } else {
            continue;
        };
        out.push_str("BEGIN:VCARD\r\nVERSION:3.0\r\n");
        line(&mut out, &format!("FN:{}", esc(&display)));
        line(&mut out, &format!("N:{};{};{};{};{}", esc(&last), esc(&first), esc(&middle), esc(&prefix), esc(&suffix)));
        if !nick.is_empty() {
            line(&mut out, &format!("NICKNAME:{}", esc(&nick)));
        }
        for (category, label, value) in mine {
            let kind = label.as_deref().map(|l| format!(";TYPE={}", l.to_uppercase().replace([';', ':', ','], ""))).unwrap_or_default();
            let entry = match category {
                1 => format!("EMAIL;TYPE=INTERNET{kind}:{}", esc(value)),
                2 => format!("TEL{kind}:{}", esc(value)),
                4 => format!("URL:{}", esc(value)),
                6 => format!("X-IM;TYPE={}:{}", label.as_deref().unwrap_or("other"), esc(value)),
                7 => format!("ORG:{}", esc(value)),
                8 => format!("TITLE:{}", esc(value)),
                9 => match birthday(value) {
                    Some(d) => format!("BDAY:{d}"),
                    None => continue,
                },
                10 => continue, // Mailbird sync metadata
                _ => format!("NOTE:{}: {}", label.as_deref().unwrap_or("Note"), esc(value)),
            };
            line(&mut out, &entry);
        }
        for parts in addresses.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
            let [street, city, region, postcode, country] = parts.clone().map(|p| esc(&p.unwrap_or_default()));
            line(&mut out, &format!("ADR:;;{street};{city};{region};{postcode};{country}"));
        }
        if let Some(names) = groups.get(&id) {
            line(&mut out, &format!("CATEGORIES:{}", names.iter().map(|n| esc(n)).collect::<Vec<_>>().join(",")));
        }
        if let Ok(photo) = fs::read(store.join("UserContactAvatars").join(format!("{id}.mbi"))) {
            line(&mut out, &format!("PHOTO;ENCODING=b;TYPE=JPEG:{}", base64::engine::general_purpose::STANDARD.encode(photo)));
        }
        out.push_str("END:VCARD\r\n");
        count += 1;
    }
    fs::write(path, out)?;
    Ok(count)
}

/// Every address Mailbird collected from mail, with when it was last seen.
fn collected(db: &Connection, path: &Path) -> Result<usize> {
    let mut out = String::from("email,name,last_message_utc,blocked\n");
    let mut count = 0;
    let mut stmt = db.prepare(
        "select Email, Name, LatestMessageAt_UTC, IsBlocked from Contacts
         where IsValidSingleContact = 1 and Email like '%@%' order by LatestMessageAt_UTC desc",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let email: String = r.get(0)?;
        let name: Option<String> = r.get(1)?;
        let when: Option<String> = r.get(2)?;
        let blocked: Option<bool> = r.get(3)?;
        let _ = writeln!(
            out,
            "{},{},{},{}",
            csv(&email),
            csv(&name.unwrap_or_default().replace("&amp;", "&")),
            when.unwrap_or_default(),
            blocked.unwrap_or(false)
        );
        count += 1;
    }
    fs::write(path, out)?;
    Ok(count)
}

fn calendars(db: &Connection, dir: &Path) -> Result<(usize, usize)> {
    let mut rules: HashMap<i64, Vec<String>> = HashMap::new();
    for row in db
        .prepare("select EventId, Rule from CalendarEventRecurrences where Rule is not null")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
    {
        let (event, rule) = row?;
        rules.entry(event).or_default().push(rule);
    }
    let mut attendees: HashMap<i64, Vec<(Option<String>, String, i64, bool)>> = HashMap::new();
    for row in db
        .prepare("select EventId, Name, Email, ResponseStatus, Organizer from CalendarEventAttendees where Email is not null")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                r.get::<_, Option<bool>>(4)?.unwrap_or(false),
            ))
        })?
    {
        let (event, name, email, status, organizer) = row?;
        attendees.entry(event).or_default().push((name, email, status, organizer));
    }

    let calendars: Vec<(i64, String, Option<String>)> = db
        .prepare(
            "select c.Id, coalesce(c.Name, 'Calendar'), coalesce(ca.Name, acc.Username) from Calendars c
             left join CalendarAccounts ca on ca.Id = c.CalendarAccountId
             left join Accounts acc on acc.Id = ca.AccountId",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;

    let mut used_names: HashMap<String, usize> = HashMap::new();
    let (mut written, mut total) = (0, 0);
    for (calendar_id, name, account) in calendars {
        let mut stmt = db.prepare(
            "select e.Id, e.Name, e.Description, e.Location, e.Start_UTC, e.End_UTC, e.WholeDay, e.Status,
                    e.ICalUID, e.RecurringEventId, e.OriginalStart_UTC, e.OrganizerName, e.OrganizerEmail,
                    m.ICalUID
             from CalendarEvents e left join CalendarEvents m on m.Id = e.RecurringEventId
             where e.CalendarId = ?1",
        )?;
        let mut rows = stmt.query([calendar_id])?;
        let mut body = String::new();
        let mut events = 0;
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            let whole_day = r.get::<_, Option<bool>>(6)?.unwrap_or(false);
            let (Some(start), Some(end)) = (stamp(r.get(4)?, whole_day), stamp(r.get(5)?, whole_day)) else { continue };
            // An exception to a recurring event shares the series UID and names the occurrence.
            let series_uid: Option<String> = r.get(13)?;
            let master: Option<i64> = r.get(9)?;
            let uid: String = match (&series_uid, master) {
                (Some(u), Some(_)) => u.clone(),
                (None, Some(m)) => format!("mailbird-event-{m}@oxymail"),
                _ => r.get::<_, Option<String>>(8)?.unwrap_or_else(|| format!("mailbird-event-{id}@oxymail")),
            };
            body.push_str("BEGIN:VEVENT\r\n");
            line(&mut body, &format!("UID:{}", esc(&uid)));
            line(&mut body, &format!("DTSTAMP:{}", stamp(r.get(4)?, false).unwrap_or_default()));
            let date = if whole_day { ";VALUE=DATE" } else { "" };
            line(&mut body, &format!("DTSTART{date}:{start}"));
            line(&mut body, &format!("DTEND{date}:{end}"));
            if master.is_some() {
                if let Some(original) = stamp(r.get(10)?, whole_day) {
                    line(&mut body, &format!("RECURRENCE-ID{date}:{original}"));
                }
            }
            line(&mut body, &format!("SUMMARY:{}", esc(&r.get::<_, Option<String>>(1)?.unwrap_or_default())));
            for (key, column) in [("DESCRIPTION", 2), ("LOCATION", 3)] {
                if let Some(text) = r.get::<_, Option<String>>(column)?.filter(|t| !t.trim().is_empty()) {
                    line(&mut body, &format!("{key}:{}", esc(&text)));
                }
            }
            if r.get::<_, Option<i64>>(7)? == Some(1) {
                line(&mut body, "STATUS:CANCELLED");
            }
            if let Some(email) = r.get::<_, Option<String>>(12)? {
                let cn = r.get::<_, Option<String>>(11)?.map(|n| format!(";CN={}", quote_param(&n))).unwrap_or_default();
                line(&mut body, &format!("ORGANIZER{cn}:mailto:{email}"));
            }
            for rule in rules.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
                for part in rule.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    line(&mut body, part);
                }
            }
            for (name, email, status, organizer) in attendees.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
                if *organizer {
                    continue;
                }
                let partstat = match status {
                    1 => "ACCEPTED",
                    2 => "DECLINED",
                    3 => "TENTATIVE",
                    _ => "NEEDS-ACTION",
                };
                let cn = name.as_deref().map(|n| format!(";CN={}", quote_param(n))).unwrap_or_default();
                line(&mut body, &format!("ATTENDEE{cn};PARTSTAT={partstat}:mailto:{email}"));
            }
            body.push_str("END:VEVENT\r\n");
            events += 1;
        }
        if events == 0 {
            continue;
        }
        let base = sanitize(&match &account {
            Some(a) if a != &name => format!("{a} - {name}"),
            _ => name.clone(),
        });
        let n = used_names.entry(base.clone()).or_default();
        *n += 1;
        let file = if *n == 1 { format!("{base}.ics") } else { format!("{base} ({n}).ics") };
        let mut ics = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//OxyMail//Mailbird export//EN\r\nCALSCALE:GREGORIAN\r\n");
        line(&mut ics, &format!("X-WR-CALNAME:{}", esc(&name)));
        ics.push_str(&body);
        ics.push_str("END:VCALENDAR\r\n");
        fs::write(dir.join(file), ics)?;
        written += 1;
        total += events;
    }
    Ok((written, total))
}

/// Mailbird `2014-10-21 00:00:00Z` → `20141021` (all-day) or `20141021T000000Z`.
fn stamp(value: Option<String>, date_only: bool) -> Option<String> {
    let v = value?;
    let digits: String = v.chars().filter(char::is_ascii_digit).take(14).collect();
    if digits.len() < 8 {
        return None;
    }
    Some(if date_only || digits.len() < 14 { digits[..8].to_string() } else { format!("{}T{}Z", &digits[..8], &digits[8..14]) })
}

/// `1/19/1981 12:00:00 AM` → `1981-01-19`.
fn birthday(value: &str) -> Option<String> {
    let date = value.split_whitespace().next()?;
    let parts: Vec<u32> = date.split('/').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [m, d, y] if *m >= 1 && *m <= 12 && *d >= 1 && *d <= 31 => Some(format!("{y:04}-{m:02}-{d:02}")),
        _ => None,
    }
}

/// vCard/iCalendar text escaping.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n").replace('\r', "")
}

fn quote_param(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "'"))
}

/// Appends a content line folded at 75 octets, as both formats require.
fn line(out: &mut String, text: &str) {
    let mut width = 0;
    for ch in text.chars() {
        let len = ch.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(ch);
        width += len;
    }
    out.push_str("\r\n");
}

fn csv(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "_".into() } else { cleaned }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_long_lines_at_75_octets() {
        let mut out = String::new();
        line(&mut out, &"x".repeat(100));
        let first = out.split("\r\n").next().unwrap();
        assert_eq!(first.len(), 75);
        assert!(out.contains("\r\n x"));
    }

    #[test]
    fn converts_dates() {
        assert_eq!(stamp(Some("2014-10-21 00:00:00Z".into()), true).as_deref(), Some("20141021"));
        assert_eq!(stamp(Some("2013-11-30 06:00:00Z".into()), false).as_deref(), Some("20131130T060000Z"));
        assert_eq!(birthday("1/19/1981 12:00:00 AM").as_deref(), Some("1981-01-19"));
    }
}
