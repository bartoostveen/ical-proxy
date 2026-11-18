use crate::cache::Cache;
use crate::error::{Error, Result};
use crate::filter::{self, Filter};
use icalendar::{Calendar, CalendarComponent, Component};
use reqwest::header::ACCEPT;

#[derive(Clone)]
pub struct CalendarService {
    http: reqwest::Client,
    cache: Cache,
}

impl CalendarService {
    pub fn new(http: reqwest::Client, cache: Cache) -> Self {
        Self { http, cache }
    }

    pub async fn get_calendar_filtered(&self, url: &str, query: &str, ttl: i64) -> Result<String> {
        let result_key = format!("result/{url}/{query}");
        if let Some(result) = self.cache.get(&result_key).await {
            return Ok(result);
        }

        let calendars = self.get_calendar(url, ttl).await?;
        let filters = filter::parse_filters(query).map_err(Error::bad_request)?;
        let result = filter_calendars(calendars, &filters);

        if let Some(timestamp) = self.cache.expire_time(&format!("raw/{url}")).await {
            self.cache.set_ex_at(&result_key, &result, timestamp).await;
        } else {
            self.cache.set_ex(&result_key, &result, ttl).await;
        }

        Ok(result)
    }

    async fn get_calendar(&self, url: &str, ttl: i64) -> Result<Vec<Calendar>> {
        let raw = self
            .get_raw_cached_or_fetch(url, ttl)
            .await
            .ok_or(Error::NotFound)?;

        parse_calendars(&raw)
    }

    async fn get_raw_cached_or_fetch(&self, url: &str, ttl: i64) -> Option<String> {
        let key = format!("raw/{url}");
        if let Some(raw) = self.cache.get(&key).await {
            return Some(raw);
        }

        let raw = match self
            .http
            .get(url)
            .header(ACCEPT, "text/calendar")
            .send()
            .await
        {
            Ok(response) => match response.text().await {
                Ok(raw) => Some(raw),
                Err(error) => {
                    tracing::warn!("failed to read response for {url}: {error}");
                    None
                }
            },
            Err(error) => {
                tracing::warn!("failed to fetch {url}: {error}");
                None
            }
        }
        .filter(|raw| !raw.trim().is_empty());

        if let Some(raw) = &raw {
            self.cache.set_ex(&key, raw, ttl).await;
        }

        raw
    }
}

fn parse_calendars(raw: &str) -> Result<Vec<Calendar>> {
    let unfolded = icalendar::parser::unfold(raw);
    let normalized = normalize_content_lines(&unfolded);
    let mut calendars = Vec::new();

    for block in split_calendars(&normalized) {
        let calendar = block.parse::<Calendar>().map_err(|error| {
            tracing::debug!("failed to parse calendar: {error}");
            Error::bad_request("Invalid calendar")
        })?;
        calendars.push(calendar);
    }

    if calendars.is_empty() {
        return Err(Error::bad_request("Invalid calendar"));
    }

    Ok(calendars)
}

fn normalize_content_lines(input: &str) -> String {
    let mut normalized = String::with_capacity(input.len());

    for line in input.split_inclusive('\n') {
        let (body, ending) = if let Some(body) = line.strip_suffix("\r\n") {
            (body, "\r\n")
        } else if let Some(body) = line.strip_suffix('\n') {
            (body, "\n")
        } else {
            (line, "")
        };
        let body = body.strip_prefix('\u{feff}').unwrap_or(body);

        if body.is_empty() || body.starts_with(' ') || body.starts_with('\t') {
            normalized.push_str(body);
            normalized.push_str(ending);
            continue;
        }

        normalized.push_str(&normalize_content_line(body));
        normalized.push_str(ending);
    }

    normalized
}

fn normalize_content_line(line: &str) -> String {
    let (header, value) = match find_unquoted(line, ':') {
        Some(separator) => (&line[..separator], Some(&line[separator + 1..])),
        None => (line, None),
    };

    let mut normalized = String::with_capacity(line.len());
    let mut uppercase = true;
    let mut quoted = false;

    for character in header.chars() {
        if quoted {
            if character == '"' {
                quoted = false;
            }
            normalized.push(character);
            continue;
        }

        match character {
            '"' => {
                quoted = true;
                normalized.push(character);
            }
            ';' => {
                uppercase = true;
                normalized.push(character);
            }
            '=' => {
                uppercase = false;
                normalized.push(character);
            }
            _ if uppercase => normalized.push(character.to_ascii_uppercase()),
            _ => normalized.push(character),
        }
    }

    if let Some(value) = value {
        normalized.push(':');

        let name = header.split(';').next().unwrap_or_default();
        if name.eq_ignore_ascii_case("BEGIN") || name.eq_ignore_ascii_case("END") {
            normalized.push_str(&value.to_ascii_uppercase());
        } else {
            normalized.push_str(value);
        }
    }

    normalized
}

fn find_unquoted(input: &str, needle: char) -> Option<usize> {
    let mut quoted = false;

    for (index, character) in input.char_indices() {
        match character {
            '"' => quoted = !quoted,
            _ if character == needle && !quoted => return Some(index),
            _ => {}
        }
    }

    None
}

fn split_calendars(input: &str) -> Vec<&str> {
    let mut calendars = Vec::new();
    let mut start = None;
    let mut offset = 0;

    for line in input.split_inclusive('\n') {
        let trimmed = line
            .trim_end_matches(['\r', '\n'])
            .trim()
            .trim_start_matches('\u{feff}');

        if trimmed.eq_ignore_ascii_case("BEGIN:VCALENDAR") {
            start = Some(offset);
        } else if trimmed.eq_ignore_ascii_case("END:VCALENDAR")
            && let Some(start) = start.take()
        {
            calendars.push(&input[start..offset + line.len()]);
        }

        offset += line.len();
    }

    calendars
}

fn filter_calendars(mut calendars: Vec<Calendar>, filters: &[Filter]) -> String {
    let mut result = String::new();

    for calendar in &mut calendars {
        calendar.components.retain(|component| match component {
            CalendarComponent::Event(event) => filters.iter().all(|filter| filter.matches(event)),
            _ => true,
        });
        write_calendar(calendar, &mut result);
    }

    result
}

fn write_calendar(calendar: &Calendar, out: &mut String) {
    out.push_str("BEGIN:VCALENDAR\r\n");

    for property in &calendar.properties {
        write_property(property, out);
    }

    for component in &calendar.components {
        match component {
            CalendarComponent::Event(component) => write_component(component, out),
            CalendarComponent::Todo(component) => write_component(component, out),
            CalendarComponent::Venue(component) => write_component(component, out),
            CalendarComponent::Other(component) => write_component(component, out),
            _ => {}
        }
    }

    out.push_str("END:VCALENDAR\r\n");
}

fn write_component<C: Component>(component: &C, out: &mut String) {
    out.push_str("BEGIN:");
    out.push_str(&component.component_kind());
    out.push_str("\r\n");

    for property in component.properties().values() {
        write_property(property, out);
    }
    for property in component.multi_properties().values().flatten() {
        write_property(property, out);
    }
    for child in component.components() {
        write_component(child, out);
    }

    out.push_str("END:");
    out.push_str(&component.component_kind());
    out.push_str("\r\n");
}

fn write_property(property: &icalendar::Property, out: &mut String) {
    let mut line = String::with_capacity(150);
    line.push_str(property.key());

    for parameter in property.params().values() {
        line.push(';');
        line.push_str(parameter.key());
        line.push('=');
        line.push_str(&quote_parameter(parameter.value()));
    }

    line.push(':');

    let value = match property.value_type() {
        Some(icalendar::ValueType::Text) if !is_text_list(property.key()) => {
            escape_text(property.value())
        }
        _ => property.value().to_owned(),
    };
    line.push_str(&value);

    out.push_str(&fold_line(&line));
    out.push_str("\r\n");
}

fn is_text_list(property: &str) -> bool {
    property.eq_ignore_ascii_case("CATEGORIES") || property.eq_ignore_ascii_case("RESOURCES")
}

fn quote_parameter(value: &str) -> String {
    if value.starts_with('"') && value.ends_with('"') {
        return value.to_owned();
    }

    if value.contains([':', ';', ',']) {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

fn escape_text(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace(',', r"\,")
        .replace(';', r"\;")
        .replace('\n', r"\n")
}

fn fold_line(line: &str) -> String {
    const LIMIT: usize = 75;

    let mut folded = String::with_capacity(line.len() + line.len() / LIMIT * 3);
    let mut start = 0;

    while line.len() - start > LIMIT {
        let mut end = start + LIMIT;
        while !line.is_char_boundary(end) {
            end -= 1;
        }

        folded.push_str(&line[start..end]);
        folded.push_str("\r\n ");
        start = end;
    }

    folded.push_str(&line[start..]);
    folded
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALENDAR: &str = concat!(
        "BEGIN:VCALENDAR\r\n",
        "VERSION:2.0\r\n",
        "PRODID:test\r\n",
        "BEGIN:VEVENT\r\n",
        "UID:first\r\n",
        "DTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20240115T100000Z\r\n",
        "DTEND:20240115T110000Z\r\n",
        "SUMMARY:Team Meeting\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\n",
        "UID:second\r\n",
        "DTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20240116T100000Z\r\n",
        "DTEND:20240116T110000Z\r\n",
        "SUMMARY:Holiday\r\n",
        "END:VEVENT\r\n",
        "END:VCALENDAR\r\n",
    );

    #[test]
    fn parses_and_filters_events() {
        let calendars = parse_calendars(CALENDAR).unwrap();
        assert_eq!(calendars.len(), 1);

        let filters = filter::parse_filters("summaryContains=Team").unwrap();
        let result = filter_calendars(calendars, &filters);

        assert!(result.contains("UID:first"));
        assert!(!result.contains("UID:second"));
        assert!(result.contains("SUMMARY:Team Meeting"));
    }

    #[test]
    fn keeps_multiple_calendars() {
        let raw = format!("{CALENDAR}{CALENDAR}");
        let calendars = parse_calendars(&raw).unwrap();
        assert_eq!(calendars.len(), 2);
    }

    #[test]
    fn does_not_invent_uids_for_other_components() {
        let raw = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "PRODID:test\r\n",
            "BEGIN:VTIMEZONE\r\n",
            "TZID:Europe/Amsterdam\r\n",
            "BEGIN:STANDARD\r\n",
            "DTSTART:19701025T030000\r\n",
            "TZOFFSETFROM:+0200\r\n",
            "TZOFFSETTO:+0100\r\n",
            "END:STANDARD\r\n",
            "END:VTIMEZONE\r\n",
            "END:VCALENDAR\r\n",
        );

        let calendars = parse_calendars(raw).unwrap();
        let result = filter_calendars(calendars, &[]);

        assert!(!result.contains("UID:"));
        assert!(!result.contains("DTSTAMP:"));
        assert!(result.contains("TZID:Europe/Amsterdam"));
    }

    #[test]
    fn escapes_text_properties() {
        let event = icalendar::Event::new()
            .uid("example")
            .summary("Comma, semicolon; backslash\\")
            .done();
        let calendar = Calendar::from([event]);
        let result = filter_calendars(vec![calendar], &[]);

        assert!(result.contains(r"SUMMARY:Comma\, semicolon\; backslash\\"));
    }

    #[test]
    fn normalizes_case_insensitive_names() {
        let raw = concat!(
            "begin:vcalendar\r\n",
            "version:2.0\r\n",
            "prodid:test\r\n",
            "begin:vevent\r\n",
            "uid:lower\r\n",
            "dtstamp:20240101T000000Z\r\n",
            "dtstart;tzid=Europe/Amsterdam:20240115T100000\r\n",
            "summary:Lowercase Event\r\n",
            "end:vevent\r\n",
            "end:vcalendar\r\n",
        );

        let calendars = parse_calendars(raw).unwrap();
        let filters = filter::parse_filters("before=1705309201").unwrap();
        assert!(filter_calendars(calendars.clone(), &filters).contains("BEGIN:VEVENT"));

        let filters = filter::parse_filters("before=1705309200").unwrap();
        assert!(!filter_calendars(calendars.clone(), &filters).contains("BEGIN:VEVENT"));

        let calendars = parse_calendars(raw).unwrap();
        let result = filter_calendars(calendars, &[]);

        assert!(result.contains("BEGIN:VEVENT"));
        assert!(result.contains("DTSTART;TZID=Europe/Amsterdam:20240115T100000"));
        assert!(result.contains("SUMMARY:Lowercase Event"));
        assert!(result.contains("UID:lower"));
    }

    #[test]
    fn rejects_invalid_calendars() {
        assert!(parse_calendars("not a calendar").is_err());
        assert!(parse_calendars("").is_err());
        assert!(parse_calendars("BEGIN:VEVENT\r\nEND:VEVENT\r\n").is_err());
    }
}
