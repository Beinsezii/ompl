use super::{Clickable, ContainedWidget};
use crate::library::Library;

use std::sync::{Arc, Weak};

use crossterm::event;
use tui::{layout::Rect, widgets::Paragraph};

#[derive(Clone)]
pub enum MTree<T> {
    Tree(Vec<(String, MTree<T>)>),
    Action(T),
}

#[derive(Clone)]
pub struct MenuBar<T> {
    lib_weak: Weak<Library>,
    tree: MTree<T>,
    nav: Vec<usize>,
    pub area: Rect,
}

impl<T: Clone> MenuBar<T> {
    pub fn new(library: &Arc<Library>, tree: MTree<T>) -> Self {
        Self {
            lib_weak: Arc::downgrade(library),
            tree,
            nav: vec![],
            area: Rect::default(),
        }
    }

    /// Fetches action if navved to and resets tree
    pub fn receive(&mut self) -> Option<T> {
        let result = match self.navigate() {
            MTree::Tree(_) => None,
            MTree::Action(a) => Some((*a).clone()),
        };
        if result.is_some() {
            self.nav = vec![]
        }
        result
    }

    /// Remove last nav index
    pub fn up(&mut self) {
        self.nav.pop();
    }

    /// Append nav index if safe
    pub fn down(&mut self, index: usize) {
        match self.navigate() {
            MTree::Action(_) => (),
            MTree::Tree(v) => {
                if index < v.len() {
                    self.nav.push(index)
                }
            }
        }
    }

    /// Navigate all the way down to a tree or action.
    fn navigate(&self) -> &MTree<T> {
        let mut ptr = &self.tree;
        for n in self.nav.iter() {
            match ptr {
                MTree::Action(_) => break,
                MTree::Tree(v) => ptr = &v[*n].1,
            }
        }
        ptr
    }

    /// Nav to the deepest tree. Good for display.
    fn nav_to_tree(&self) -> Option<&Vec<(String, MTree<T>)>> {
        let mut ptr = None;
        let mut tree_ptr = &self.tree;
        for n in self.nav.iter() {
            match tree_ptr {
                MTree::Action(_) => break,
                MTree::Tree(v) => {
                    ptr = Some(v);
                    tree_ptr = &v[*n].1;
                }
            }
        }
        // if ends on tree
        match tree_ptr {
            MTree::Tree(v) => ptr = Some(v),
            MTree::Action(_) => (),
        }
        ptr
    }
}

impl<T: Clone> ContainedWidget for MenuBar<T> {
    fn draw<B: tui::backend::Backend>(
        &mut self,
        frame: &mut tui::terminal::Frame<B>,
        theme: super::Theme,
    ) {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return,
        };

        if let Some(tree) = self.nav_to_tree() {
            frame.render_widget(
                Paragraph::new(
                    String::from(" [0] <- | ")
                        + &tree
                            .iter()
                            .enumerate()
                            .map(|(n, t)| format!("[{}] {}", n + 1, t.0))
                            .collect::<Vec<String>>()
                            .join(" | "),
                ),
                self.area,
            );
        } else {
            frame.render_widget(
                Paragraph::new("MenuBar Placeholder").style(theme.base),
                self.area,
            );
        }
    }
}

impl<T: Clone> Clickable for MenuBar<T> {
    fn process_event(&mut self, event: event::MouseEvent) -> bool {
        let library = match self.lib_weak.upgrade() {
            Some(l) => l,
            None => return false,
        };

        if event.kind == event::MouseEventKind::Down(event::MouseButton::Left) {
            if self
                .area
                .intersects(Rect::new(event.column, event.row, 1, 1))
            {
                match event.column {
                    1..=6 => self.up(),
                    _ => (),
                }
            }
        }
        false
    }
}
