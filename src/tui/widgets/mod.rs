pub use super::theme::Theme;
mod statusbar;
pub use statusbar::StatusBar;
mod queue;
pub use queue::{Queue, QueueState};

use tui::widgets::{StatefulWidget, Widget};

pub trait ClickableWidget: Widget {
    fn process_event<T: AsRef<crate::Library>>(
        event: crossterm::event::MouseEvent,
        area: tui::layout::Rect,
        library: T,
    ) -> bool;
}

pub trait ClickableStatefulWidget: StatefulWidget {
    fn process_stateful_event<T: AsRef<crate::Library>>(
        event: crossterm::event::MouseEvent,
        area: tui::layout::Rect,
        library: T,
        state: &mut Self::State,
    ) -> bool;
}

/// Assumes there is a 2 character border
pub fn scroll_down(position: &mut usize, view: &mut usize, mut height: usize, length: usize) {
    height = height.saturating_sub(2);
    *view = std::cmp::min(*view + height / 2, length.saturating_sub(height));
    *position = std::cmp::min(*position + height / 2, length - 1);
}

/// Assumes there is a 2 character border
pub fn scroll_up(position: &mut usize, view: &mut usize, mut height: usize) {
    height = height.saturating_sub(2);
    *view = view.saturating_sub(height / 2);
    *position = position.saturating_sub(height / 2);
}

/// Assumes there is a 2 character border
pub fn scroll_to(position: usize, view: &mut usize, mut height: usize, length: usize) {
    height = height.saturating_sub(2);
    *view = std::cmp::min(
        position.saturating_sub(height / 2),
        length.saturating_sub(height),
    );
}
