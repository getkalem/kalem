//! The universal argument (Doom's `SPC u`, Emacs's `C-u`; T2.7i.18): a
//! count for the next command. `SPC u` alone is 4, again 16; digits typed
//! after it give the count instead. The next command runs that many
//! times, or once when it asks something of the frontend (a list, a file),
//! as [`crate::CommandRegistry::execute_times`] does.

/// The command that starts or multiplies the count.
pub const COMMAND: &str = "app.universalArgument";

/// The most times a command repeats.
pub const MAX: usize = 10_000;

/// A count being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixArg {
    /// 4 to the times `SPC u` was pressed.
    times: usize,
    /// The digits typed after it.
    digits: Option<usize>,
}

impl Default for PrefixArg {
    fn default() -> Self {
        PrefixArg::new()
    }
}

impl PrefixArg {
    /// `SPC u` pressed once: 4.
    pub fn new() -> PrefixArg {
        PrefixArg {
            times: 4,
            digits: None,
        }
    }

    /// `SPC u` pressed again: four times as many, unless digits were
    /// typed.
    pub fn again(&mut self) {
        if self.digits.is_none() {
            self.times = (self.times * 4).min(MAX);
        }
    }

    /// Key `key` typed after it: a digit is taken (`true`), else nothing.
    pub fn key(&mut self, key: &str) -> bool {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return false;
        };
        let Some(d) = c.to_digit(10) else {
            return false;
        };
        self.digits = Some((self.digits.unwrap_or(0) * 10 + d as usize).min(MAX));
        true
    }

    /// How many times the next command runs.
    pub fn count(&self) -> usize {
        self.digits.unwrap_or(self.times).clamp(1, MAX)
    }

    /// For the status bar.
    pub fn label(&self) -> String {
        crate::tr!("msg-universal-argument", n = self.count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_as_emacs_does() {
        let mut p = PrefixArg::new();
        assert_eq!(p.count(), 4);
        p.again();
        assert_eq!(p.count(), 16);
        assert!(p.key("1"));
        assert!(p.key("2"));
        assert!(!p.key("x"));
        assert!(!p.key("12"));
        assert_eq!(p.count(), 12);
        p.again();
        assert_eq!(p.count(), 12);
        assert!(p.key("0"));
        assert_eq!(p.count(), 120);
        let mut z = PrefixArg::new();
        z.key("0");
        assert_eq!(z.count(), 1);
        for _ in 0..20 {
            z.key("9");
        }
        assert_eq!(z.count(), MAX);
    }

    #[test]
    fn a_command_that_asks_runs_once() {
        let reg = crate::CommandRegistry::with_builtins();
        let config = crate::settings::Config::default();
        let mut clip = crate::command::Clipboard::default();
        let mut ctx = crate::command::EditorContext::new(
            None,
            &mut clip,
            &config,
            std::time::Instant::now(),
            jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
        );
        reg.execute_times("view.palette", &mut ctx, &serde_json::Value::Null, 5)
            .unwrap();
        assert_eq!(ctx.requests.len(), 1);
    }
}
