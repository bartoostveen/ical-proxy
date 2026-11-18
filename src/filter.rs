use chrono::{DateTime, Utc};
use icalendar::{CalendarDateTime, Component, DatePerhapsTime, Event};

#[derive(Debug, Clone)]
pub struct Filter {
    kind: FilterKind,
    invert: bool,
}

#[derive(Debug, Clone)]
enum FilterKind {
    Before(DateTime<Utc>),
    After(DateTime<Utc>),
    EndsBefore(DateTime<Utc>),
    EndsAfter(DateTime<Utc>),
    TextContains {
        field: TextField,
        value: String,
        ignore_case: bool,
    },
    FieldContains {
        name: String,
        value: String,
        ignore_case: bool,
    },
}

#[derive(Debug, Clone, Copy)]
enum TextField {
    Summary,
    Description,
    Location,
    Organizer,
}

impl TextField {
    fn value(self, event: &Event) -> Option<&str> {
        match self {
            Self::Summary => event.property_value("SUMMARY"),
            Self::Description => event.property_value("DESCRIPTION"),
            Self::Location => event.property_value("LOCATION"),
            Self::Organizer => organizer_email(event),
        }
    }
}

impl Filter {
    pub fn matches(&self, event: &Event) -> bool {
        let result = match &self.kind {
            FilterKind::Before(timestamp) => {
                event_date(event, "DTSTART").is_some_and(|date| date < *timestamp)
            }
            FilterKind::After(timestamp) => {
                event_date(event, "DTSTART").is_some_and(|date| date > *timestamp)
            }
            FilterKind::EndsBefore(timestamp) => {
                event_date(event, "DTEND").is_some_and(|date| date < *timestamp)
            }
            FilterKind::EndsAfter(timestamp) => {
                event_date(event, "DTEND").is_some_and(|date| date > *timestamp)
            }
            FilterKind::TextContains {
                field,
                value,
                ignore_case,
            } => field.value(event).is_some_and(|text| {
                if *ignore_case {
                    contains_ignore_case(text, value)
                } else {
                    text.contains(value)
                }
            }),
            FilterKind::FieldContains {
                name,
                value,
                ignore_case,
            } => field_value(event, name, *ignore_case).is_some_and(|text| {
                if *ignore_case {
                    contains_ignore_case(text, value)
                } else {
                    text.contains(value)
                }
            }),
        };

        if self.invert { !result } else { result }
    }
}

/// Parses an URL-encoded query string into the filters it describes.
///
/// Unknown parameters (including `url` and `ttl`) are ignored, just like the
/// original implementation.
pub fn parse_filters(query: &str) -> Result<Vec<Filter>, String> {
    let mut filters = Vec::new();

    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let key = key.into_owned();
        let value = value.into_owned();

        let invert = key.starts_with('!');
        let mut name = key.trim().to_lowercase();
        if invert {
            name.remove(0);
        }

        if name == "url" || name == "ttl" {
            continue;
        }

        if let Some(filter) = build_filter(&name, value, invert)? {
            filters.push(filter);
        }
    }

    Ok(filters)
}

fn build_filter(name: &str, value: String, invert: bool) -> Result<Option<Filter>, String> {
    let kind = match name {
        "before" => FilterKind::Before(parse_instant(&value)),
        "after" => FilterKind::After(parse_instant(&value)),
        "endsbefore" => FilterKind::EndsBefore(parse_instant(&value)),
        "endsafter" => FilterKind::EndsAfter(parse_instant(&value)),
        "summarycontains" => FilterKind::TextContains {
            field: TextField::Summary,
            value,
            ignore_case: false,
        },
        "descriptioncontains" => FilterKind::TextContains {
            field: TextField::Description,
            value,
            ignore_case: false,
        },
        "locationcontains" => FilterKind::TextContains {
            field: TextField::Location,
            value,
            ignore_case: false,
        },
        "organizercontains" => FilterKind::TextContains {
            field: TextField::Organizer,
            value,
            ignore_case: false,
        },
        "summarycontainsignorecase" => FilterKind::TextContains {
            field: TextField::Summary,
            value,
            ignore_case: true,
        },
        "descriptioncontainsignorecase" => FilterKind::TextContains {
            field: TextField::Description,
            value,
            ignore_case: true,
        },
        "locationcontainsignorecase" => FilterKind::TextContains {
            field: TextField::Location,
            value,
            ignore_case: true,
        },
        "organizercontainsignorecase" => FilterKind::TextContains {
            field: TextField::Organizer,
            value,
            ignore_case: true,
        },
        "fieldcontains" => {
            let (name, value) = split_pair(&value)?;
            FilterKind::FieldContains {
                name: name.to_owned(),
                value: value.to_owned(),
                ignore_case: false,
            }
        }
        "fieldcontainsignorecase" => {
            let (name, value) = split_pair(&value)?;
            FilterKind::FieldContains {
                name: name.to_owned(),
                value: value.to_owned(),
                ignore_case: true,
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(Filter { kind, invert }))
}

fn parse_instant(value: &str) -> DateTime<Utc> {
    value
        .parse::<i64>()
        .ok()
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        .unwrap_or_else(Utc::now)
}

fn split_pair(value: &str) -> Result<(&str, &str), String> {
    let mut parts = value.split(',');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().ok_or_else(|| "Invalid pair".to_owned())?;
    Ok((first, second))
}

fn event_date(event: &Event, property: &str) -> Option<DateTime<Utc>> {
    let property = event.properties().get(property)?;
    let value = DatePerhapsTime::from_property(property)?;

    match value {
        DatePerhapsTime::Date(date) => date
            .and_hms_opt(0, 0, 0)
            .map(|date_time| date_time.and_utc()),
        DatePerhapsTime::DateTime(date_time) => match &date_time {
            CalendarDateTime::Floating(date_time) => Some(date_time.and_utc()),
            CalendarDateTime::Utc(date_time) => Some(*date_time),
            CalendarDateTime::WithTimezone { .. } => date_time.try_into_utc(),
        },
    }
}

fn organizer_email(event: &Event) -> Option<&str> {
    let organizer = event.property_value("ORGANIZER")?;
    let (scheme, email) = organizer.split_once(':')?;

    scheme.eq_ignore_ascii_case("mailto").then_some(email)
}

fn field_value<'a>(event: &'a Event, name: &str, ignore_case: bool) -> Option<&'a str> {
    event
        .properties()
        .values()
        .chain(event.multi_properties().values().flatten())
        .find(|property| {
            if ignore_case {
                property.key().eq_ignore_ascii_case(name)
            } else {
                property.key() == name
            }
        })
        .map(|property| property.value())
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use icalendar::EventLike;

    fn event() -> Event {
        Event::new()
            .uid("example")
            .summary("Team Meeting")
            .description("Discuss the roadmap")
            .location("Room 42")
            .add_property("ORGANIZER", "mailto:jane@example.com")
            .add_property("DTSTART", "20240115T100000Z")
            .add_property("DTEND", "20240115T110000Z")
            .add_property("X-CUSTOM", "some value")
            .done()
    }

    fn matches(query: &str, event: &Event) -> bool {
        let filters = parse_filters(query).unwrap();
        filters.iter().all(|filter| filter.matches(event))
    }

    #[test]
    fn date_filters() {
        assert!(matches("before=1705312801", &event()));
        assert!(matches("after=1705312799", &event()));
        assert!(matches("endsBefore=1705316401", &event()));
        assert!(matches("endsAfter=1705316399", &event()));

        assert!(!matches("before=1705312800", &event()));
        assert!(!matches("after=1705312800", &event()));
        assert!(!matches("endsBefore=1705316400", &event()));
        assert!(!matches("endsAfter=1705316400", &event()));
    }

    #[test]
    fn text_filters() {
        assert!(matches("summaryContains=Team", &event()));
        assert!(matches("summaryContainsIgnoreCase=team", &event()));
        assert!(matches("descriptionContains=roadmap", &event()));
        assert!(matches("locationContains=42", &event()));
        assert!(matches("organizerContains=jane@example.com", &event()));
        assert!(matches("fieldContains=X-CUSTOM,some", &event()));
        assert!(matches("fieldContainsIgnoreCase=x-custom,SOME", &event()));

        assert!(!matches("summaryContains=team", &event()));
        assert!(!matches("locationContains=43", &event()));
    }

    #[test]
    fn inverted_filters() {
        assert!(matches("!summaryContains=holiday", &event()));
        assert!(!matches("!summaryContains=Team", &event()));
        assert!(matches("!url=ignored&!ttl=ignored", &event()));
    }

    #[test]
    fn multiple_values_and_unknown_filters() {
        assert!(matches(
            "summaryContains=Team&summaryContains=Meeting&unknown=value&url=x&ttl=1",
            &event()
        ));
        assert!(!matches(
            "summaryContains=Team&summaryContains=holiday",
            &event()
        ));
    }

    #[test]
    fn invalid_pair_is_rejected() {
        assert!(parse_filters("fieldContains=onlyone").is_err());
        assert!(parse_filters("fieldContains=").is_err());
    }
}
