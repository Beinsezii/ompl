pub use super::theme::Theme;
mod statusbar;
pub use statusbar::StatusBar;
mod filtertreeview;
pub use filtertreeview::FilterTreeView;
mod queuetable;
pub use queuetable::QueueTable;

/// Self-contained widget does it's own state and render management
pub trait ContainedWidget {
    fn draw<T: tui::backend::Backend>(&mut self, frame: &mut tui::terminal::Frame<T>, theme: Theme);
}

pub trait Clickable {
    fn process_event(&mut self, event: crossterm::event::MouseEvent) -> bool;
}

pub trait Scrollable {
    /// cursor position, view offset, height of view, max length
    fn get_fields(&mut self) -> Option<(&mut usize, &mut usize, usize, usize)>;

    /// scroll down half view length
    fn scroll_down(&mut self) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n(height as i32 / 2, position, view, height, length)
        }
    }

    /// scroll up half view length
    fn scroll_up(&mut self) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n(-(height as i32 / 2), position, view, height, length)
        }
    }

    /// move position by N and lock view to center
    fn scroll_by_n_lock(&mut self, n: i32) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n_lock(n, position, view, height, length)
        }
    }

    /// move position and view down by N
    fn scroll_by_n(&mut self, n: i32) {
        if let Some((position, view, height, length)) = self.get_fields() {
            scroll_by_n(n, position, view, height, length)
        }
    }
}

pub fn scroll_by_n(n: i32, position: &mut usize, view: &mut usize, height: usize, length: usize) {
    *position = (n + *position as i32).max(0).min(length as i32 - 1) as usize;
    *view = (n + *view as i32)
        .max(0)
        .min(length.saturating_sub(height) as i32) as usize;
}

pub fn scroll_by_n_lock(
    n: i32,
    position: &mut usize,
    view: &mut usize,
    height: usize,
    length: usize,
) {
    *position = (n + *position as i32)
        .max(0)
        .min(length.saturating_sub(1) as i32) as usize;
    *view = position
        .saturating_sub(height / 2)
        .min(length.saturating_sub(height));
}
