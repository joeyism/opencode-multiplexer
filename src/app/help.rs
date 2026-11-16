use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HelpState {
    scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpAction {
    Consumed,
    Close,
}

impl HelpState {
    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }

    pub fn set_scroll_offset(&mut self, offset: usize, total: usize, viewport: usize) {
        self.scroll = offset;
        self.reclamp(total, viewport);
    }

    pub fn scroll_by(&mut self, amount: isize, total: usize, viewport: usize) {
        self.scroll = if amount.is_negative() {
            self.scroll.saturating_sub(amount.unsigned_abs())
        } else {
            self.scroll.saturating_add(amount as usize)
        };
        self.reclamp(total, viewport);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self, total: usize, viewport: usize) {
        self.scroll = total.saturating_sub(viewport);
    }

    pub fn reclamp(&mut self, total: usize, viewport: usize) {
        self.scroll = self.scroll.min(total.saturating_sub(viewport));
    }
}

pub fn handle_help_key(
    key: KeyEvent,
    help: &mut HelpState,
    help_key: char,
    total: usize,
    viewport: usize,
) -> HelpAction {
    if key.code == KeyCode::Esc
        || matches!(key.code, KeyCode::Char('q'))
        || matches!(key.code, KeyCode::Char(ch) if ch == help_key)
    {
        return HelpAction::Close;
    }

    let page = (viewport / 2).max(1) as isize;
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => help.scroll_by(1, total, viewport),
        KeyCode::Char('k') | KeyCode::Up => help.scroll_by(-1, total, viewport),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            help.scroll_by(page, total, viewport)
        }
        KeyCode::PageDown => help.scroll_by(page, total, viewport),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            help.scroll_by(-page, total, viewport)
        }
        KeyCode::PageUp => help.scroll_by(-page, total, viewport),
        KeyCode::Char('g') => help.scroll_to_top(),
        KeyCode::Char('G') => help.scroll_to_bottom(total, viewport),
        _ => {}
    }
    help.reclamp(total, viewport);
    HelpAction::Consumed
}
