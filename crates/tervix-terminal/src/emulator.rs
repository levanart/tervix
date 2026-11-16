use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::Dimensions,
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{self, Color, NamedColor},
};
use std::sync::mpsc::SyncSender;
use tervix_application::terminal::{Cell, GridSize, Lifecycle, Snapshot};

pub(crate) struct Listener(pub SyncSender<Vec<u8>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        // OSC 52 load/store, title changes and external URLs have no side effects.
        // Protocol replies are bounded; a congested writer must not deadlock parsing.
        if let Event::PtyWrite(text) = event {
            let _ = self.0.try_send(text.into_bytes());
        }
    }
}

pub(crate) struct DimensionsOf(pub GridSize);
impl Dimensions for DimensionsOf {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.0.rows)
    }
    fn columns(&self) -> usize {
        usize::from(self.0.columns)
    }
}

pub(crate) fn new(size: GridSize, writer: SyncSender<Vec<u8>>) -> Term<Listener> {
    Term::new(
        Config {
            scrolling_history: 10_000,
            ..Config::default()
        },
        &DimensionsOf(size),
        Listener(writer),
    )
}

pub(crate) fn snapshot(term: &Term<Listener>, size: GridSize, lifecycle: Lifecycle) -> Snapshot {
    let content = term.renderable_content();
    let cells = content
        .display_iter
        .filter_map(|indexed| {
            let row = indexed.point.line.0 + content.display_offset as i32;
            if row < 0 || row >= i32::from(size.rows) {
                return None;
            }
            let cell = indexed.cell;
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                return None;
            }
            let mut foreground = color(cell.fg);
            let mut background = color(cell.bg);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            let mut text = if cell.flags.contains(Flags::HIDDEN) {
                " ".into()
            } else {
                cell.c.to_string()
            };
            if let Some(extra) = cell.zerowidth() {
                text.extend(extra);
            }
            Some(Cell {
                column: indexed.point.column.0 as u16,
                row: row as u16,
                text,
                foreground,
                background,
            })
        })
        .collect::<Vec<_>>();
    Snapshot {
        lifecycle,
        size,
        cells: cells.into(),
        cursor: (content.cursor.shape != ansi::CursorShape::Hidden).then_some((
            content.cursor.point.column.0 as u16,
            content.cursor.point.line.0.max(0) as u16,
        )),
        application_cursor: content.mode.contains(TermMode::APP_CURSOR),
        bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
    }
}

fn color(value: Color) -> [u8; 3] {
    match value {
        Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
        Color::Named(NamedColor::Foreground) => [238, 244, 252],
        Color::Named(NamedColor::Background) => [40, 54, 74],
        Color::Named(value) => indexed(value as u16),
        Color::Indexed(value) => indexed(u16::from(value)),
    }
}
fn indexed(value: u16) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 16] = [
        [25, 29, 38],
        [242, 145, 154],
        [152, 195, 121],
        [229, 192, 123],
        [133, 181, 255],
        [212, 157, 237],
        [86, 182, 194],
        [220, 223, 228],
        [150, 165, 186],
        [255, 125, 135],
        [180, 225, 145],
        [255, 215, 150],
        [135, 200, 255],
        [225, 150, 250],
        [120, 215, 225],
        [255, 255, 255],
    ];
    match value {
        0..=15 => PALETTE[usize::from(value)],
        16..=231 => {
            let n = value - 16;
            let c = |x: u16| if x == 0 { 0 } else { (55 + x * 40) as u8 };
            [c(n / 36), c(n / 6 % 6), c(n % 6)]
        }
        232..=255 => [8 + (value as u8 - 232) * 10; 3],
        _ => [220, 225, 235],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_background_does_not_replace_explicit_black_or_truecolor() {
        let (tx, _) = std::sync::mpsc::sync_channel(8);
        let size = GridSize::new(20, 2);
        let mut term = new(size, tx);
        let mut parser: ansi::Processor = Default::default();
        parser.advance(&mut term, b"A\x1b[40mB\x1b[48;2;18;22;32mC\x1b[49mD");
        let frame = snapshot(&term, size, Lifecycle::Running);
        assert_eq!(frame.cells[0].background, frame.cells[3].background);
        assert_ne!(frame.cells[0].background, frame.cells[1].background);
        assert_eq!(frame.cells[1].background, indexed(0));
        assert_eq!(frame.cells[2].background, [18, 22, 32]);
    }

    #[test]
    fn vt_colors_unicode_cursor_and_clipboard_policy() {
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let size = GridSize::new(40, 5);
        let mut term = new(size, tx);
        let mut parser: ansi::Processor = Default::default();
        parser.advance(
            &mut term,
            "\x1b[38;2;1;2;3m界e\u{301}\x1b]52;c;c2VjcmV0\x07".as_bytes(),
        );
        let frame = snapshot(&term, size, Lifecycle::Running);
        assert_eq!(frame.cells[0].text, "界");
        assert_eq!(frame.cells[0].foreground, [1, 2, 3]);
        assert!(
            frame
                .cells
                .iter()
                .any(|c| c.text == "e\u{301}" && c.column == 2)
        );
        assert!(rx.try_recv().is_err());
        parser.advance(&mut term, b"\x1b[?1049hALT\x1b[?1049l");
        assert_eq!(
            snapshot(&term, size, Lifecycle::Running).cells[0].text,
            "界"
        );
    }
}
