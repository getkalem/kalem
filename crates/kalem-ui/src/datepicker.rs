//! The date picker (T1.5.18): a month calendar for arguments whose schema
//! says `format: date`, such as `org.insert.date`'s. Arrows move by day
//! and week, Page Up and Page Down by month; typing a date expression
//! (`+3d`, `fri 10:00`, `2026-10-01`, see `kalem_core::dates`) moves too.

use gpui::{
    Context, InteractiveElement, IntoElement, Keystroke, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use jiff::Span;
use jiff::civil::{Date, Time};
use kalem_core::dates;
use serde_json::Value;

use crate::editor::Editor;

/// The open date picker.
#[derive(Debug, Clone)]
pub struct DatePicker {
    command: String,
    args: Value,
    name: String,
    /// The chosen day.
    pub date: Date,
    /// The time, if the input or the timestamp being changed has one.
    pub time: Option<Time>,
    /// A typed date expression.
    pub input: String,
    today: Date,
}

impl DatePicker {
    /// The argument for the chosen date.
    pub fn argument(&self) -> String {
        dates::argument(
            self.date.to_datetime(self.time.unwrap_or(Time::midnight())),
            self.time.is_some(),
        )
    }

    fn step(&mut self, span: Span) {
        if let Ok(d) = self.date.checked_add(span) {
            self.date = d;
        }
    }

    /// The 42 days of the calendar page: the weeks (Monday first) around
    /// the chosen day's month.
    pub fn page(&self) -> Vec<Date> {
        let first = self.date.first_of_month();
        let back = i64::from(first.weekday().to_monday_zero_offset());
        let start = first.checked_sub(Span::new().days(back)).unwrap_or(first);
        (0..42)
            .filter_map(|i| start.checked_add(Span::new().days(i)).ok())
            .collect()
    }
}

impl Editor {
    /// Opens the date picker for argument `name` of `command`, at the
    /// timestamp under the cursor or today.
    pub fn open_date_picker(
        &mut self,
        command: &str,
        args: Value,
        name: String,
        cx: &mut Context<'_, Self>,
    ) {
        let now = jiff::Zoned::now().datetime();
        let at = self
            .doc
            .parse()
            .and_then(|(p, _)| dates::timestamp_at(&p.syntax(), self.doc.selection.head));
        let (date, time) = match at {
            Some((dt, with_time)) => (dt.date(), with_time.then(|| dt.time())),
            None => (now.date(), None),
        };
        self.completion = None;
        self.palette = None;
        self.date_picker = Some(DatePicker {
            command: command.to_string(),
            args,
            name,
            date,
            time,
            input: String::new(),
            today: now.date(),
        });
        cx.notify();
    }

    /// Runs the command with the chosen date.
    fn pick_date(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(p) = self.date_picker.take() else {
            return;
        };
        let args = kalem_core::command::with_argument(p.args.clone(), &p.name, p.argument().into());
        self.run_command(&p.command, args, window, cx);
        cx.notify();
    }

    /// Typed text for the date picker: a date expression.
    pub fn date_input(&mut self, text: &str, cx: &mut Context<'_, Self>) -> bool {
        let Some(p) = &mut self.date_picker else {
            return false;
        };
        p.input.push_str(text);
        p.reparse();
        cx.notify();
        true
    }

    /// Keys for the date picker; `true` if used.
    pub fn date_key(
        &mut self,
        k: &Keystroke,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(p) = &mut self.date_picker else {
            return false;
        };
        match k.key.as_str() {
            "escape" => self.date_picker = None,
            "enter" => self.pick_date(window, cx),
            "left" => p.step(Span::new().days(-1)),
            "right" => p.step(Span::new().days(1)),
            "up" => p.step(Span::new().days(-7)),
            "down" => p.step(Span::new().days(7)),
            "pageup" => p.step(Span::new().months(-1)),
            "pagedown" => p.step(Span::new().months(1)),
            "backspace" => {
                p.input.pop();
                p.reparse();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The date picker, when open.
    pub fn date_picker_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let p = self.date_picker.as_ref()?;
        let theme = &self.theme;
        let cell = px(34.);
        let month = p.date.month();
        let header = div()
            .flex()
            .flex_row()
            .justify_between()
            .items_center()
            .child(
                div()
                    .id("date-previous")
                    .px(px(8.))
                    .cursor_pointer()
                    .child("‹")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(p) = &mut this.date_picker {
                            p.step(Span::new().months(-1));
                        }
                        cx.notify();
                    })),
            )
            .child(SharedString::from(format!(
                "{} {}",
                kalem_core::l10n::tr(&format!("month-{}", p.date.month())),
                p.date.year()
            )))
            .child(
                div()
                    .id("date-next")
                    .px(px(8.))
                    .cursor_pointer()
                    .child("›")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(p) = &mut this.date_picker {
                            p.step(Span::new().months(1));
                        }
                        cx.notify();
                    })),
            );
        let weekdays = kalem_core::l10n::tr("weekday-short");
        let names = div().flex().flex_row().text_color(theme.muted).children(
            weekdays
                .split_whitespace()
                .map(|n| {
                    div()
                        .w(cell)
                        .flex()
                        .justify_center()
                        .child(SharedString::from(n.to_string()))
                })
                .collect::<Vec<_>>(),
        );
        let days = p.page();
        let weeks = days.chunks(7).map(|week| {
            div().flex().flex_row().children(week.iter().map(|d| {
                let d = *d;
                let mut c = div()
                    .id(SharedString::from(format!("date-{d}")))
                    .debug_selector(|| format!("date-{d}"))
                    .w(cell)
                    .h(px(26.))
                    .flex()
                    .justify_center()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .child(SharedString::from(d.day().to_string()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(p) = &mut this.date_picker {
                            p.date = d;
                        }
                        this.pick_date(window, cx);
                    }));
                if d.month() != month {
                    c = c.text_color(theme.muted);
                }
                if d == p.today {
                    c = c.border_1().border_color(theme.caret);
                }
                if d == p.date {
                    c = c.bg(theme.selection);
                }
                c
            }))
        });
        let chosen = p
            .date
            .to_datetime(p.time.unwrap_or(Time::midnight()))
            .strftime(if p.time.is_some() {
                "%a %Y-%m-%d %H:%M"
            } else {
                "%a %Y-%m-%d"
            })
            .to_string();
        Some(
            div()
                .absolute()
                .top(px(8.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("date-picker")
                        .debug_selector(|| "date-picker".into())
                        .occlude()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .p(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.bar)
                        .text_size(px(theme.size * 0.85))
                        .child(header)
                        .child(names)
                        .children(weeks)
                        .child(
                            div()
                                .mt(px(4.))
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(SharedString::from(format!("> {}▏", p.input)))
                                .child(
                                    div()
                                        .text_color(theme.muted)
                                        .child(SharedString::from(chosen)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl DatePicker {
    /// Moves to the typed date, when it reads as one.
    fn reparse(&mut self) {
        let now = self.today.to_datetime(jiff::Zoned::now().datetime().time());
        if let Some((dt, with_time)) = dates::parse(&self.input, now) {
            self.date = dt.date();
            self.time = with_time.then(|| dt.time());
        }
    }
}
