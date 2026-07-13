//! RAII terminal setup/teardown. A screen runs either fullscreen (raw mode + alternate screen) or in
//! a small inline viewport of a fixed height (raw mode, no alternate screen — like `fzf`).
//!
//! An inline screen must leave a **git-native footprint**: on exit it erases its own drawing and
//! returns the cursor to the row where the command was invoked, optionally printing a one-line report
//! of what happened, then a fresh line for the shell prompt — never a lingering blank block. That
//! clean teardown is [`TerminalGuard::finish_inline`]; [`Drop`] is the safety net that restores raw
//! mode / the alternate screen even on panic or early return.

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub struct TerminalGuard {
    pub terminal: Tui,
    /// Whether we entered the alternate screen (fullscreen) and must leave it on drop.
    alternate: bool,
}

impl TerminalGuard {
    /// Enter fullscreen: raw mode + alternate screen (the default for the log/status/diff screens).
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self {
            terminal,
            alternate: true,
        })
    }

    /// Enter an inline viewport `height` rows tall in the current terminal (no alternate screen), so
    /// the screen appears as a small window that leaves the surrounding scrollback intact.
    pub fn enter_inline(height: u16) -> Result<Self> {
        enable_raw_mode()?;
        let stdout = io::stdout();
        let terminal = Terminal::with_options(
            CrosstermBackend::new(stdout),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;
        Ok(Self {
            terminal,
            alternate: false,
        })
    }

    /// Clean up an inline screen so it leaves a git-native footprint: erase the viewport, put the
    /// cursor back at the row the command was invoked on, and — if the screen has something to say —
    /// print a single persistent `report` line before dropping to a fresh line for the shell prompt.
    /// A no-op for fullscreen screens (the alternate screen restores itself on drop).
    pub fn finish_inline(&mut self, report: Option<String>) -> Result<()> {
        if self.alternate {
            return Ok(());
        }
        let top = self.terminal.get_frame().area().y;
        let mut stdout = io::stdout();
        // Wipe everything from the viewport's top row down — the TUI's whole footprint — and leave the
        // cursor there so the shell prompt (or our report) continues exactly where output would.
        execute!(stdout, MoveTo(0, top), Clear(ClearType::FromCursorDown))?;
        if let Some(msg) = report.filter(|m| !m.is_empty()) {
            // Raw mode is still on, so translate the newline explicitly.
            execute!(stdout, Print(msg), Print("\r\n"))?;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        if self.alternate {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
        let _ = self.terminal.show_cursor();
    }
}
