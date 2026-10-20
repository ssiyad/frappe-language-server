//! Cron expressions as Frappe's scheduler reads them, through `croniter`.
//!
//! An expression this rejects is one the scheduler refuses at sync time, far
//! from the edit ([12](../../../docs/specs/12-surface-hooks.md)). The check
//! errs towards accepting: anything one supported `croniter` release accepts
//! passes, so `15W` passes although 6.0 refuses it and 6.2 takes it.

/// Whether `croniter` would accept `expression`.
#[must_use]
pub fn is_cron(expression: &str) -> bool {
    let expression = expression.trim();
    if let Some(alias) = expression.strip_prefix('@') {
        return ALIASES.contains(&alias.to_ascii_lowercase().as_str());
    }
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if !(5..=7).contains(&fields.len()) {
        return false;
    }
    fields
        .iter()
        .zip(FIELDS)
        .all(|(text, field)| field.accepts(text))
}

const ALIASES: [&str; 7] = [
    "annually", "daily", "hourly", "midnight", "monthly", "weekly", "yearly",
];

/// The fields in order: five standard, then seconds, then the year.
const FIELDS: [Field; 7] = [
    Field::numeric(0, 59),
    Field::numeric(0, 23),
    Field {
        low: 1,
        high: 31,
        names: &[],
        kind: Kind::DayOfMonth,
    },
    Field {
        low: 1,
        high: 12,
        names: &[
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        ],
        kind: Kind::Plain,
    },
    Field {
        low: 0,
        high: 7,
        names: &["sun", "mon", "tue", "wed", "thu", "fri", "sat"],
        kind: Kind::DayOfWeek,
    },
    Field::numeric(0, 59),
    Field::numeric(1970, 2099),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    /// Also takes `?`, `L` and a lone `nW`.
    DayOfMonth,
    /// Also takes `?` and a lone `d#n`.
    DayOfWeek,
}

struct Field {
    low: u32,
    high: u32,
    /// Names for `low`, `low + 1`, …; matched without case.
    names: &'static [&'static str],
    kind: Kind,
}

impl Field {
    const fn numeric(low: u32, high: u32) -> Self {
        Self {
            low,
            high,
            names: &[],
            kind: Kind::Plain,
        }
    }

    fn accepts(&self, text: &str) -> bool {
        let text = text.to_ascii_lowercase();
        // `?`, `nW` and `d#n` stand alone: croniter refuses them in a list.
        if text == "?" {
            return self.kind != Kind::Plain;
        }
        if self.kind == Kind::DayOfMonth
            && let Some(day) = text.strip_suffix('w')
        {
            return self.value(day).is_some();
        }
        if self.kind == Kind::DayOfWeek
            && let Some((day, nth)) = text.split_once('#')
        {
            return self.value(day).is_some() && matches!(nth, "1" | "2" | "3" | "4" | "5");
        }
        text.split(',').all(|part| self.accepts_part(part))
    }

    /// One list item: `*`, `R`, a value or a range, with an optional step.
    fn accepts_part(&self, part: &str) -> bool {
        if part == "l" {
            return self.kind == Kind::DayOfMonth;
        }
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => (range, Some(step)),
            None => (part, None),
        };
        if step.is_some_and(|step| !step.parse::<u32>().is_ok_and(|step| step > 0)) {
            return false;
        }
        match range {
            "*" | "r" => true,
            _ => match range.split_once('-') {
                Some((from, to)) => self.value(from).is_some() && self.value(to).is_some(),
                None => self.value(range).is_some(),
            },
        }
    }

    fn value(&self, text: &str) -> Option<u32> {
        let number = match self.names.iter().position(|name| *name == text) {
            Some(index) => self.low + u32::try_from(index).ok()?,
            None if text.bytes().all(|byte| byte.is_ascii_digit()) => text.parse().ok()?,
            None => return None,
        };
        (self.low..=self.high).contains(&number).then_some(number)
    }
}

#[cfg(test)]
mod tests {
    use super::is_cron;

    // Each case was checked against `croniter.is_valid` in 6.0.0 and 6.2.3.
    #[test]
    fn accepts_what_croniter_accepts() {
        for good in [
            "* * * * *",
            "0 3 * * *",
            "*/15 * * * *",
            "0/5 * * * *",
            "1-10/2 * * * *",
            "10-1 * * * *",
            "*/100 * * * *",
            "00 00 * * *",
            "0\t0 * * *  ",
            "0 0 31 2 *",
            "0 0 * * 7",
            "0 0 * * 0-7",
            "0 0 * * mon",
            "0 0 * * MON-fri",
            "0 0 * * sun,sat",
            "0 0 * * mon-7",
            "0 0 * Jan-mar *",
            "0 0 * jan-dec/2 *",
            "0 0 L * *",
            "0 0 L,15 * *",
            "0 0 15W * *",
            "0 0 ? * *",
            "0 0 * * ?",
            "0 0 * * 1#2",
            "0 0 * * sun#2",
            "0 0 * * 0#5",
            "R * * * *",
            "R/5 * * * *",
            "* * * * * 0/5",
            "* * * * * 59",
            "0 0 1 1 * 0 2030",
            "0 0 * * * * 2099",
            "@hourly",
            "@HOURLY",
            "@annually",
            "@midnight",
        ] {
            assert!(is_cron(good), "{good}");
        }
    }

    #[test]
    fn refuses_what_croniter_refuses() {
        for bad in [
            "",
            "* * * *",
            "* * * * * * * *",
            "60 * * * *",
            "0 24 * * *",
            "0 0 0 * *",
            "0 0 * 0 *",
            "0 0 * 13 *",
            "0 0 * * 8",
            "-1 * * * *",
            "*/0 * * * *",
            "5/0 * * * *",
            "*/5/2 * * * *",
            "5-10-15 * * * *",
            "0 0 1, * *",
            ",1 * * * *",
            "0 0 a * *",
            "0 0 * * monday",
            "0 0 * mon * *",
            "0 0 * * jan",
            "0 0 * * 5L",
            "0 0 * L *",
            "0 0 LW * *",
            "0 0 L-2 * *",
            "0 0 L/2 * *",
            "0 0 32W * *",
            "0 0 0W * *",
            "0 0 15W,1 * *",
            "? * * * *",
            "0 0 * ? *",
            "0 0 * * ?/2",
            "0 0 * * 1#6",
            "0 0 * * 1#0",
            "0 0 * * 1#2,3",
            "H * * * *",
            "* * * * * 60",
            "* * * * * ?",
            "0 0 1 1 * 0 1969",
            "0 0 1 1 * 0 2100",
            "@reboot",
            "@every",
            "@hourly *",
        ] {
            assert!(!is_cron(bad), "{bad}");
        }
    }
}
