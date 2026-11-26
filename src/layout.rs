use crate::pane::PaneId;
use crate::renderer::Rect;

/// Breite der Trennlinie zwischen zwei Panes in Pixeln.
pub const DIVIDER: usize = 1;

/// Wohin die neue Pane beim Teilen kommt (iTerm2: ⌘D = rechts, ⇧⌘D = unten).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDir {
    /// Nebeneinander, senkrechte Trennlinie
    Right,
    /// Untereinander, waagerechte Trennlinie
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Eine Trennlinie mit Pfad zu ihrem Split-Knoten (false = a, true = b), zum Ziehen mit der Maus.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub rect: Rect,
    /// Rechteck des Split-Knotens, zu dem die Linie gehört
    pub parent: Rect,
    pub dir: SplitDir,
    pub path: Vec<bool>,
}

impl Divider {
    /// Verhältnis, das sich ergibt, wenn die Linie an Position (x, y) gezogen wird.
    pub fn ratio_at(&self, x: f64, y: f64) -> f32 {
        let (pos, start, len) = match self.dir {
            SplitDir::Right => (x, self.parent.x, self.parent.w),
            SplitDir::Down => (y, self.parent.y, self.parent.h),
        };
        let avail = len.saturating_sub(DIVIDER).max(1) as f64;
        (((pos - start as f64) / avail) as f32).clamp(0.1, 0.9)
    }
}

#[derive(Debug)]
pub enum Node {
    Leaf(PaneId),
    Split {
        dir: SplitDir,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

impl Node {
    /// Zerlegt `rect` rekursiv: Rechteck je Pane + Rechtecke der Trennlinien.
    pub fn layout(&self, rect: Rect, panes: &mut Vec<(PaneId, Rect)>, dividers: &mut Vec<Rect>) {
        match self {
            Node::Leaf(id) => panes.push((*id, rect)),
            Node::Split { dir, ratio, a, b } => {
                let (ra, div, rb) = split_rect(rect, *dir, *ratio);
                dividers.push(div);
                a.layout(ra, panes, dividers);
                b.layout(rb, panes, dividers);
            }
        }
    }

    /// Ersetzt das Blatt `target` durch einen Split aus `target` und `new`.
    pub fn split(&mut self, target: PaneId, dir: SplitDir, new: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                *self = Node::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(Node::Leaf(target)),
                    b: Box::new(Node::Leaf(new)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(target, dir, new) || b.split(target, dir, new),
        }
    }

    /// Entfernt das Blatt `target`; der Geschwisterknoten rückt nach.
    /// Gibt `None` zurück, wenn danach nichts mehr übrig ist.
    pub fn remove(self, target: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(id) if id == target => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split { dir, ratio, a, b } => match (a.remove(target), b.remove(target)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
        }
    }

    /// Verschiebt die nächstgelegene passende Trennlinie über `target`.
    /// `delta` > 0 schiebt nach rechts bzw. unten.
    pub fn resize(&mut self, target: PaneId, axis: SplitDir, delta: f32) -> bool {
        self.resize_inner(target, axis, delta) == ResizeState::Done
    }

    fn resize_inner(&mut self, target: PaneId, axis: SplitDir, delta: f32) -> ResizeState {
        match self {
            Node::Leaf(id) if *id == target => ResizeState::Found,
            Node::Leaf(_) => ResizeState::NotFound,
            Node::Split { dir, ratio, a, b } => {
                let mut r = a.resize_inner(target, axis, delta);
                if r == ResizeState::NotFound {
                    r = b.resize_inner(target, axis, delta);
                }
                if r == ResizeState::Found && *dir == axis {
                    *ratio = (*ratio + delta).clamp(0.1, 0.9);
                    return ResizeState::Done;
                }
                r
            }
        }
    }

    /// Alle Trennlinien mit Pfad zum jeweiligen Split.
    pub fn dividers(&self, rect: Rect) -> Vec<Divider> {
        let mut out = Vec::new();
        self.dividers_inner(rect, &mut Vec::new(), &mut out);
        out
    }

    fn dividers_inner(&self, rect: Rect, path: &mut Vec<bool>, out: &mut Vec<Divider>) {
        if let Node::Split { dir, ratio, a, b } = self {
            let (ra, div, rb) = split_rect(rect, *dir, *ratio);
            out.push(Divider {
                rect: div,
                parent: rect,
                dir: *dir,
                path: path.clone(),
            });
            path.push(false);
            a.dividers_inner(ra, path, out);
            path.pop();
            path.push(true);
            b.dividers_inner(rb, path, out);
            path.pop();
        }
    }

    /// Setzt das Verhältnis des Splits am Ende von `path`.
    pub fn set_ratio(&mut self, path: &[bool], r: f32) {
        if let Node::Split { ratio, a, b, .. } = self {
            match path.split_first() {
                None => *ratio = r.clamp(0.1, 0.9),
                Some((false, rest)) => a.set_ratio(rest, r),
                Some((true, rest)) => b.set_ratio(rest, r),
            }
        }
    }

    /// Alle Pane-Ids in Reihenfolge (links/oben zuerst).
    pub fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }
}

#[derive(PartialEq, Eq)]
enum ResizeState {
    NotFound,
    Found,
    Done,
}

fn split_rect(r: Rect, dir: SplitDir, ratio: f32) -> (Rect, Rect, Rect) {
    match dir {
        SplitDir::Right => {
            let avail = r.w.saturating_sub(DIVIDER);
            let wa = (avail as f32 * ratio).round() as usize;
            let a = Rect {
                x: r.x,
                y: r.y,
                w: wa,
                h: r.h,
            };
            let div = Rect {
                x: r.x + wa,
                y: r.y,
                w: DIVIDER,
                h: r.h,
            };
            let b = Rect {
                x: r.x + wa + DIVIDER,
                y: r.y,
                w: avail - wa,
                h: r.h,
            };
            (a, div, b)
        }
        SplitDir::Down => {
            let avail = r.h.saturating_sub(DIVIDER);
            let ha = (avail as f32 * ratio).round() as usize;
            let a = Rect {
                x: r.x,
                y: r.y,
                w: r.w,
                h: ha,
            };
            let div = Rect {
                x: r.x,
                y: r.y + ha,
                w: r.w,
                h: DIVIDER,
            };
            let b = Rect {
                x: r.x,
                y: r.y + ha + DIVIDER,
                w: r.w,
                h: avail - ha,
            };
            (a, div, b)
        }
    }
}

/// Sucht die Pane, die von `from` aus in Richtung `dir` am nächsten liegt
/// (muss sich auf der Querachse mit `from` überlappen).
pub fn neighbor(panes: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let (_, f) = *panes.iter().find(|(id, _)| *id == from)?;
    let overlap = |a0: usize, a1: usize, b0: usize, b1: usize| a0 < b1 && b0 < a1;
    panes
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, r)| {
            let dist = match dir {
                Direction::Left if r.x + r.w <= f.x && overlap(r.y, r.y + r.h, f.y, f.y + f.h) => {
                    f.x - (r.x + r.w)
                }
                Direction::Right if r.x >= f.x + f.w && overlap(r.y, r.y + r.h, f.y, f.y + f.h) => {
                    r.x - (f.x + f.w)
                }
                Direction::Up if r.y + r.h <= f.y && overlap(r.x, r.x + r.w, f.x, f.x + f.w) => {
                    f.y - (r.y + r.h)
                }
                Direction::Down if r.y >= f.y + f.h && overlap(r.x, r.x + r.w, f.x, f.x + f.w) => {
                    r.y - (f.y + f.h)
                }
                _ => return None,
            };
            Some((dist, *id))
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: usize, y: usize, w: usize, h: usize) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn split_and_remove() {
        let mut t = Node::Leaf(PaneId(0));
        assert!(t.split(PaneId(0), SplitDir::Right, PaneId(1)));
        assert!(t.split(PaneId(1), SplitDir::Down, PaneId(2)));
        let mut ids = vec![];
        t.leaves(&mut ids);
        assert_eq!(ids, vec![PaneId(0), PaneId(1), PaneId(2)]);

        let t = t.remove(PaneId(1)).unwrap();
        let mut ids = vec![];
        t.leaves(&mut ids);
        assert_eq!(ids, vec![PaneId(0), PaneId(2)]);

        let t = t.remove(PaneId(0)).unwrap().remove(PaneId(2));
        assert!(t.is_none());
    }

    #[test]
    fn layout_and_neighbor() {
        let mut t = Node::Leaf(PaneId(0));
        t.split(PaneId(0), SplitDir::Right, PaneId(1));
        t.split(PaneId(1), SplitDir::Down, PaneId(2));
        let (mut panes, mut divs) = (vec![], vec![]);
        t.layout(r(0, 0, 201, 101), &mut panes, &mut divs);
        assert_eq!(divs.len(), 2);
        assert_eq!(panes[0].1, r(0, 0, 100, 101));
        assert_eq!(
            neighbor(&panes, PaneId(0), Direction::Right),
            Some(PaneId(1))
        );
        assert_eq!(
            neighbor(&panes, PaneId(1), Direction::Down),
            Some(PaneId(2))
        );
        assert_eq!(
            neighbor(&panes, PaneId(2), Direction::Left),
            Some(PaneId(0))
        );
        assert_eq!(neighbor(&panes, PaneId(0), Direction::Left), None);
    }

    #[test]
    fn divider_drag() {
        let mut t = Node::Leaf(PaneId(0));
        t.split(PaneId(0), SplitDir::Right, PaneId(1));
        t.split(PaneId(1), SplitDir::Down, PaneId(2));
        let area = r(0, 0, 201, 101);
        let divs = t.dividers(area);
        assert_eq!(divs.len(), 2);
        assert_eq!(divs[0].path, Vec::<bool>::new());
        assert_eq!(divs[1].path, vec![true]);
        assert_eq!(divs[1].dir, SplitDir::Down);

        let ratio = divs[0].ratio_at(50.0, 0.0);
        assert!((ratio - 0.25).abs() < 1e-6);
        t.set_ratio(&divs[0].path, ratio);
        t.set_ratio(&divs[1].path, 0.0); // wird auf 0.1 begrenzt
        let (mut panes, mut d) = (vec![], vec![]);
        t.layout(area, &mut panes, &mut d);
        assert_eq!(panes[0].1.w, 50);
        assert_eq!(panes[1].1.h, 10);
    }

    #[test]
    fn resize_moves_nearest_matching_divider() {
        let mut t = Node::Leaf(PaneId(0));
        t.split(PaneId(0), SplitDir::Right, PaneId(1));
        t.split(PaneId(1), SplitDir::Down, PaneId(2));
        assert!(t.resize(PaneId(2), SplitDir::Right, 0.1));
        assert!(t.resize(PaneId(2), SplitDir::Down, -0.1));
        match &t {
            Node::Split { ratio, b, .. } => {
                assert!((ratio - 0.6).abs() < 1e-6);
                match b.as_ref() {
                    Node::Split { ratio, .. } => assert!((ratio - 0.4).abs() < 1e-6),
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }
}
