use std::collections::HashMap;

use crate::layout::{self, Direction, Divider, Node, SplitDir};
use crate::pane::{Pane, PaneId};
use crate::renderer::Rect;

/// Ein Tab: Split-Baum + die zugehörigen Panes + Fokus.
pub struct Tab {
    pub panes: HashMap<PaneId, Pane>,
    tree: Node,
    pub focused: PaneId,
    /// Fokussierte Pane füllt den ganzen Tab (iTerm2: ⇧⌘↩).
    pub zoomed: bool,
}

impl Tab {
    pub fn new(pane: Pane) -> Self {
        let id = pane.id;
        let mut panes = HashMap::new();
        panes.insert(id, pane);
        Self {
            panes,
            tree: Node::Leaf(id),
            focused: id,
            zoomed: false,
        }
    }

    /// Rechtecke aller sichtbaren Panes und der Trennlinien in `area`.
    pub fn layout(&self, area: Rect) -> (Vec<(PaneId, Rect)>, Vec<Rect>) {
        let (mut panes, mut dividers) = (Vec::new(), Vec::new());
        if self.zoomed {
            panes.push((self.focused, area));
        } else {
            self.tree.layout(area, &mut panes, &mut dividers);
        }
        (panes, dividers)
    }

    pub fn focused_pane(&self) -> &Pane {
        &self.panes[&self.focused]
    }

    /// Teilt die fokussierte Pane; die neue Pane bekommt den Fokus.
    pub fn split(&mut self, dir: SplitDir, new: Pane) {
        let id = new.id;
        if self.tree.split(self.focused, dir, id) {
            self.panes.insert(id, new);
            self.focused = id;
            self.zoomed = false;
        }
    }

    /// Entfernt eine Pane. Gibt `true` zurück, wenn der Tab danach leer ist.
    pub fn remove(&mut self, id: PaneId) -> bool {
        if !self.panes.contains_key(&id) {
            return self.panes.is_empty();
        }
        let mut before = Vec::new();
        self.tree.leaves(&mut before);
        let idx = before.iter().position(|p| *p == id).unwrap_or(0);

        self.panes.remove(&id);
        let tree = std::mem::replace(&mut self.tree, Node::Leaf(id));
        match tree.remove(id) {
            None => return true,
            Some(t) => self.tree = t,
        }

        if self.focused == id {
            let mut after = Vec::new();
            self.tree.leaves(&mut after);
            self.focused = after[idx.saturating_sub(1).min(after.len() - 1)];
            self.zoomed = false;
        }
        false
    }

    /// Fokus in eine Richtung verschieben. Gibt `true` zurück, wenn sich der Fokus geändert hat.
    pub fn focus_dir(&mut self, area: Rect, dir: Direction) -> bool {
        if self.zoomed {
            return false;
        }
        let (panes, _) = self.layout(area);
        match layout::neighbor(&panes, self.focused, dir) {
            Some(id) => {
                self.focused = id;
                true
            }
            None => false,
        }
    }

    pub fn focus_at(&mut self, area: Rect, x: f64, y: f64) -> bool {
        let (panes, _) = self.layout(area);
        match panes.iter().find(|(_, r)| r.contains(x, y)) {
            Some((id, _)) if *id != self.focused => {
                self.focused = *id;
                true
            }
            _ => false,
        }
    }

    /// Trennlinien des sichtbaren Layouts (keine im Zoom-Modus).
    pub fn dividers(&self, area: Rect) -> Vec<Divider> {
        if self.zoomed {
            Vec::new()
        } else {
            self.tree.dividers(area)
        }
    }

    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) {
        self.tree.set_ratio(path, ratio);
    }

    /// Pane unter dem Punkt (x, y) samt Rechteck.
    pub fn pane_at(&self, area: Rect, x: f64, y: f64) -> Option<(PaneId, Rect)> {
        self.layout(area)
            .0
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
    }

    /// Verschiebt die Trennlinie neben der fokussierten Pane.
    pub fn resize_split(&mut self, dir: Direction) -> bool {
        const STEP: f32 = 0.05;
        let (axis, delta) = match dir {
            Direction::Left => (SplitDir::Right, -STEP),
            Direction::Right => (SplitDir::Right, STEP),
            Direction::Up => (SplitDir::Down, -STEP),
            Direction::Down => (SplitDir::Down, STEP),
        };
        !self.zoomed && self.tree.resize(self.focused, axis, delta)
    }
}
