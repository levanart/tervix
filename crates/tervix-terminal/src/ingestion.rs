//! Preflight using the pinned VT parser, before bytes can allocate in the emulator.
use alacritty_terminal::vte::{Params, Parser, Perform};
use unicode_width::UnicodeWidthChar;

const SEQUENCE_LIMIT: usize = 64 * 1024;
const EXPANSION_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct Guard {
    parser: Parser,
    budget: Budget,
}

#[derive(Default)]
struct Budget {
    pending: usize,
    expansions: usize,
}

impl Guard {
    pub(crate) fn check(&mut self, bytes: &[u8]) -> Result<(), String> {
        for byte in bytes {
            // Check before advancing either parser; even unterminated OSC is bounded.
            if self.budget.pending >= SEQUENCE_LIMIT {
                return Err("Terminal VT sequence exceeded 64 KiB".into());
            }
            self.budget.pending += 1;
            self.parser.advance(&mut self.budget, &[*byte]);
            if self.budget.expansions > EXPANSION_LIMIT {
                return Err("Terminal Unicode/repeat expansion budget exceeded (1048576)".into());
            }
        }
        Ok(())
    }
}

impl Perform for Budget {
    fn print(&mut self, c: char) {
        self.pending = 0;
        if c.width() == Some(0) {
            self.expansions += 1;
        }
    }
    fn execute(&mut self, _: u8) {
        self.pending = 0;
    }
    fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) {
        self.pending = 0;
    }
    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        self.pending = 0;
        // REP can append combining characters without a print callback. Charge all
        // REP expansions conservatively, including ordinary spacing characters.
        if action == 'b' {
            self.expansions += usize::from(params.iter().next().map_or(1, |p| p[0].max(1)));
        }
    }
    fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {
        self.pending = 0;
    }
    fn unhook(&mut self) {
        self.pending = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combining_flood_and_rep_fail_before_emulator_growth() {
        for chunk in ["\u{301}".repeat(4096).into_bytes(), b"\x1b[65535b".to_vec()] {
            let mut guard = Guard::default();
            let mut accepted = 0;
            while guard.check(&chunk).is_ok() {
                accepted += 1;
                assert!(accepted <= EXPANSION_LIMIT);
            }
            assert!(accepted > 0);
            assert!(guard.budget.expansions <= EXPANSION_LIMIT + 65535);
        }
    }

    #[test]
    fn unterminated_osc_is_bounded_across_chunks_and_unicode_is_lossless() {
        let mut guard = Guard::default();
        for byte in "界e\u{301}👩\u{200d}💻\x1b[31m".as_bytes() {
            assert!(guard.check(&[*byte]).is_ok());
        }
        assert!(guard.check(b"\x1b]52;c;").is_ok());
        for _ in 0..7 {
            assert!(guard.check(&[b'x'; 8192]).is_ok());
        }
        assert!(guard.check(&[b'x'; 8192]).is_err());
        assert!(guard.budget.pending <= SEQUENCE_LIMIT);
    }
}
