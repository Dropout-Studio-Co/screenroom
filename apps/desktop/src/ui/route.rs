use crate::app::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FocusPos {
    pub rail: usize,

    pub column: usize,
}

impl FocusPos {
    pub const FIRST: FocusPos = FocusPos { rail: 0, column: 0 };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Left,

    Right,

    Up,

    Down,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nav {
    stack: Vec<(Screen, FocusPos)>,
}

fn clamped(column: usize, len: usize) -> usize {
    if len == 0 { 0 } else { column.min(len - 1) }
}

impl Nav {
    pub fn new(screen: Screen) -> Nav {
        Nav {
            stack: vec![(screen, FocusPos::FIRST)],
        }
    }

    pub fn screen(&self) -> Screen {
        self.stack
            .last()
            .map(|entry| entry.0)
            .unwrap_or(Screen::Home)
    }

    pub fn focus(&self) -> FocusPos {
        self.stack
            .last()
            .map(|entry| entry.1)
            .unwrap_or(FocusPos::FIRST)
    }

    pub fn push(&mut self, screen: Screen) {
        self.stack.push((screen, FocusPos::FIRST));
    }

    pub fn set(&mut self, screen: Screen) {
        let focus = self.focus();
        self.stack.clear();
        self.stack.push((screen, focus));
    }

    pub fn pop(&mut self) -> Option<Screen> {
        if self.stack.len() <= 1 {
            return None;
        }
        self.stack.pop().map(|entry| entry.0)
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn set_focus(&mut self, pos: FocusPos) {
        if let Some(entry) = self.stack.last_mut() {
            entry.1 = pos;
        }
    }

    pub fn move_focus(&mut self, direction: Focus, rails: &[usize], columns: usize) {
        let mut pos = self.focus();

        if rails.is_empty() {
            self.set_focus(FocusPos::FIRST);
            return;
        }

        pos.rail = pos.rail.min(rails.len() - 1);
        pos.column = clamped(pos.column, rails[pos.rail]);

        match direction {
            Focus::Left => pos.column = pos.column.saturating_sub(1),
            Focus::Right => {
                pos.column = clamped(pos.column.saturating_add(1), rails[pos.rail]);
            }
            Focus::Up | Focus::Down if rails.len() > 1 => {
                let step = i64::from(matches!(direction, Focus::Down));
                let rail = (pos.rail as i64 + if step > 0 { 1 } else { -1 })
                    .clamp(0, rails.len() as i64 - 1) as usize;
                pos.rail = rail;
                pos.column = clamped(pos.column, rails[rail]);
            }
            Focus::Up => {
                pos.column = pos.column.saturating_sub(columns.max(1));
            }
            Focus::Down => {
                pos.column = clamped(pos.column.saturating_add(columns.max(1)), rails[pos.rail]);
            }
        }

        self.set_focus(pos);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Screen;
    use crate::ui::route::{Focus, FocusPos, Nav};

    #[test]
    fn focus_clamps_inside_a_rail_instead_of_wrapping() {
        let mut nav = Nav::new(Screen::Home);
        nav.move_focus(Focus::Left, &[3, 3], 6);
        assert_eq!(
            nav.focus(),
            FocusPos { rail: 0, column: 0 },
            "left at the start must not wrap"
        );

        nav.move_focus(Focus::Right, &[3, 3], 6);
        nav.move_focus(Focus::Right, &[3, 3], 6);
        nav.move_focus(Focus::Right, &[3, 3], 6);
        assert_eq!(
            nav.focus(),
            FocusPos { rail: 0, column: 2 },
            "right stops at the last card"
        );
    }

    #[test]
    fn moving_down_keeps_the_column_when_the_lower_rail_is_shorter() {
        let mut nav = Nav::new(Screen::Home);
        nav.set_focus(FocusPos { rail: 0, column: 5 });
        nav.move_focus(Focus::Down, &[8, 2], 6);
        assert_eq!(
            nav.focus(),
            FocusPos { rail: 1, column: 1 },
            "the shorter rail clamps the column"
        );
    }

    #[test]
    fn a_single_item_rail_is_navigable_and_does_not_panic() {
        let mut nav = Nav::new(Screen::Home);
        nav.move_focus(Focus::Right, &[1], 6);
        assert_eq!(nav.focus(), FocusPos { rail: 0, column: 0 });
        nav.move_focus(Focus::Down, &[1], 6);
        assert_eq!(
            nav.focus(),
            FocusPos { rail: 0, column: 0 },
            "the last rail absorbs Down"
        );
    }

    #[test]
    fn an_empty_library_has_a_focus_that_is_not_a_card() {
        let mut nav = Nav::new(Screen::Home);
        nav.move_focus(Focus::Down, &[], 6);
        assert_eq!(nav.focus(), FocusPos { rail: 0, column: 0 });
    }

    #[test]
    fn going_back_restores_the_focus_the_screen_was_left_with() {
        let mut nav = Nav::new(Screen::Browse);
        nav.set_focus(FocusPos { rail: 0, column: 4 });
        nav.push(Screen::Detail);
        nav.set_focus(FocusPos { rail: 1, column: 2 });
        nav.pop();
        assert_eq!(nav.screen(), Screen::Browse);
        assert_eq!(nav.focus(), FocusPos { rail: 0, column: 4 });
    }
}
