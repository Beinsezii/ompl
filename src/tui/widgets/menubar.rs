#![warn(missing_docs)]

use super::{Action, Clickable, ContainedWidget, StyleSheet};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

#[derive(Clone)]
pub enum MTree<T> {
    Tree(Vec<(String, MTree<T>)>),
    Action(T),
}

#[derive(Clone)]
pub struct MenuBar<T> {
    tree: MTree<T>,
    nav: Vec<usize>,
    area: Rect,
}

impl<T: Clone> MenuBar<T> {
    pub fn new(tree: MTree<T>) -> Self {
        Self {
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
    fn render(&mut self, buf: &mut Buffer, area: Rect, stylesheet: StyleSheet) {
        self.area = area;
        if let Some(tree) = self.nav_to_tree() {
            let mut spans = vec![Span::from(if !self.nav.is_empty() { " 0.<- | " } else { " " })];
            for (n, t) in tree.iter().enumerate() {
                spans.push(Span::styled(
                    (n + 1).to_string() + ".",
                    match t.1 {
                        MTree::Tree(..) => stylesheet.base,
                        MTree::Action(..) => stylesheet.active,
                    },
                ));
                spans.push(Span::from(t.0.clone()));
                if n + 1 < tree.len() {
                    spans.push(Span::from(" | "));
                }
            }
            Paragraph::new(Line::from(spans)).style(stylesheet.base).render(area, buf);
        } else {
            Paragraph::new("MenuBar Placeholder").style(stylesheet.base).render(area, buf);
        }
    }
}

impl<T: Clone> Clickable for MenuBar<T> {
    fn process_event(&mut self, event: MouseEvent) -> Action {
        let mut result = Action::None;
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if self.area.intersects(Rect::new(event.column, event.row, 1, 1)) {
                match event.column + if !self.nav.is_empty() { 0 } else { 7 } {
                    1..=4 => {
                        self.up();
                        result = Action::Draw
                    }
                    x => {
                        if let Some(tree) = self.nav_to_tree() {
                            result = Action::Draw;
                            // " 0.<- | " == 7
                            let mut base = 8;
                            for (num, (string, _)) in tree.iter().enumerate() {
                                // add 2 for num + '.'
                                // WARNING won't work if you ever create trees with > 9 items in a
                                // single level. Won't fix for now as that'd need new event code
                                // too.
                                if (base..(base + string.len() + 2)).contains(&x.into()) {
                                    self.down(num);
                                    break;
                                } else {
                                    // 2 for num + '.', 3 for " | "
                                    base += string.len() + 5
                                }
                            }
                        }
                    }
                }
            }
        }
        result
    }
}
