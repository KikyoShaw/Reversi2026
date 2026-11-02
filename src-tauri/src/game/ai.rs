use super::{rules, Color, LegalMove, Position};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub index: usize,
    pub depth: u8,
    pub exact: bool,
}

const INFINITY: i32 = 1_000_000;
const WIN_SCORE: i32 = 100_000;
const SQUARE_WEIGHTS: [i32; 64] = [
    120, -25, 20, 5, 5, 20, -25, 120,
    -25, -60, -5, -5, -5, -5, -60, -25,
    20, -5, 15, 3, 3, 15, -5, 20,
    5, -5, 3, 3, 3, 3, -5, 5,
    5, -5, 3, 3, 3, 3, -5, 5,
    20, -5, 15, 3, 3, 15, -5, 20,
    -25, -60, -5, -5, -5, -5, -60, -25,
    120, -25, 20, 5, 5, 20, -25, 120,
];

pub fn search(
    position: Position,
    difficulty: u8,
    cancel: Arc<AtomicBool>,
    budget: Option<Duration>,
) -> Option<SearchResult> {
    if position.board.len() != 64 || cancel.load(Ordering::Relaxed) {
        return None;
    }
    let (normal_depth, normal_budget) = match difficulty {
        0 | 1 => (2, Duration::from_millis(150)),
        2 => (4, Duration::from_millis(500)),
        _ => (8, Duration::from_millis(1500)),
    };
    let limit = budget
        .unwrap_or(normal_budget)
        .min(normal_budget)
        .max(Duration::from_millis(1));
    let context = SearchContext {
        cancel: cancel.as_ref(),
        deadline: Instant::now() + limit,
    };

    let mut root_moves = Vec::new();
    let mut root_complete = true;
    for index in 0..64 {
        if context.cancelled() {
            return None;
        }
        if context.expired() && !root_moves.is_empty() {
            root_complete = false;
            break;
        }
        // Even an expired budget must establish one legal fallback. This scan
        // is bounded by 64 squares and remains cancellable at every square.
        let flips = rules::flips(&position, position.turn, index);
        if !flips.is_empty() {
            root_moves.push(LegalMove { index, flips });
        }
    }
    if context.cancelled() {
        return None;
    }
    let mut best = SearchResult {
        index: root_moves.first()?.index,
        depth: 0,
        exact: false,
    };
    if !root_complete || context.expired() {
        return Some(best);
    }
    order_moves(&position, &mut root_moves);
    let empty = position.board.iter().filter(|cell| cell.is_none()).count() as u8;
    let target_depth = if empty <= 12 { empty } else { normal_depth };
    for depth in 1..=target_depth {
        match context.root_iteration(&position, &root_moves, depth) {
            Ok(index) => {
                best = SearchResult {
                    index,
                    depth,
                    exact: depth >= empty,
                };
            }
            Err(Interrupted) => break,
        }
        if best.exact {
            break;
        }
    }
    if context.cancelled() {
        None
    } else {
        Some(best)
    }
}

struct Interrupted;

struct SearchContext<'a> {
    cancel: &'a AtomicBool,
    deadline: Instant,
}

impl SearchContext<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn check(&self) -> Result<(), Interrupted> {
        if self.cancelled() || self.expired() {
            Err(Interrupted)
        } else {
            Ok(())
        }
    }

    fn legal_moves(&self, position: &Position, color: Color) -> Result<Vec<LegalMove>, Interrupted> {
        let mut moves = Vec::new();
        for index in 0..64 {
            self.check()?;
            if position.board[index].is_none() {
                let flips = rules::flips(position, color, index);
                if !flips.is_empty() {
                    moves.push(LegalMove { index, flips });
                }
            }
        }
        Ok(moves)
    }

    fn root_iteration(
        &self,
        position: &Position,
        moves: &[LegalMove],
        depth: u8,
    ) -> Result<usize, Interrupted> {
        self.check()?;
        let mut best_index = moves[0].index;
        let mut best_score = -INFINITY;
        let mut alpha = -INFINITY;
        for candidate in moves {
            self.check()?;
            let child = play_raw(position, candidate);
            let mut score = -self.negamax(&child, depth - 1, -INFINITY, -alpha)?;
            if score == best_score && candidate.index < best_index {
                // A cutoff can return an upper bound equal to the incumbent.
                // Resolve that bound before breaking a tie by square index.
                score = -self.negamax(&child, depth - 1, -INFINITY, INFINITY)?;
            }
            if score > best_score || (score == best_score && candidate.index < best_index) {
                best_score = score;
                best_index = candidate.index;
            }
            alpha = alpha.max(score);
        }
        self.check()?;
        Ok(best_index)
    }

    fn negamax(
        &self,
        position: &Position,
        depth: u8,
        mut alpha: i32,
        beta: i32,
    ) -> Result<i32, Interrupted> {
        self.check()?;
        let mut moves = self.legal_moves(position, position.turn)?;
        if moves.is_empty() {
            let opponent_moves = self.legal_moves(position, position.turn.other())?;
            if opponent_moves.is_empty() {
                return Ok(terminal_score(position));
            }
            // A pass changes perspective but does not spend a placement depth.
            let mut passed = position.clone();
            passed.turn = position.turn.other();
            return Ok(-self.negamax(&passed, depth, -beta, -alpha)?);
        }
        if depth == 0 {
            let opponent_moves = self.legal_moves(position, position.turn.other())?;
            return self.evaluate(position, moves.len(), opponent_moves.len());
        }
        order_moves(position, &mut moves);
        let mut best = -INFINITY;
        for candidate in &moves {
            self.check()?;
            let child = play_raw(position, candidate);
            let score = -self.negamax(&child, depth - 1, -beta, -alpha)?;
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        self.check()?;
        Ok(best)
    }

    fn evaluate(
        &self,
        position: &Position,
        own_mobility: usize,
        other_mobility: usize,
    ) -> Result<i32, Interrupted> {
        self.check()?;
        let (black, white) = position.counts();
        let (own, other) = match position.turn {
            Color::Black => (black, white),
            Color::White => (white, black),
        };
        let occupied = (black + white) as i32;
        let empty = 64 - occupied;
        let mut positional = 0;
        let mut corners = 0;
        let mut frontier = 0;
        for (index, cell) in position.board.iter().enumerate() {
            if index % 8 == 0 {
                self.check()?;
            }
            if let Some(color) = *cell {
                let sign = if color == position.turn { 1 } else { -1 };
                positional += sign * square_weight(position, index, color);
                if is_corner(index) {
                    corners += sign;
                }
                if is_frontier(position, index) {
                    frontier -= sign;
                }
            }
        }
        let disc_weight = 1 + occupied * occupied / 128;
        let mobility_weight = 12 + empty / 4;
        let frontier_weight = 4 + empty / 8;
        let score = (own as i32 - other as i32) * disc_weight
            + (own_mobility as i32 - other_mobility as i32) * mobility_weight
            + corners * 650
            + positional * 3
            + frontier * frontier_weight;
        self.check()?;
        Ok(score)
    }
}

// Search keeps every placement's next turn as the opponent. Pass handling in
// negamax therefore always has an explicit, correct change of perspective.
fn play_raw(position: &Position, candidate: &LegalMove) -> Position {
    let mut board = position.board.clone();
    board[candidate.index] = Some(position.turn);
    for &index in &candidate.flips {
        board[index] = Some(position.turn);
    }
    Position {
        board,
        turn: position.turn.other(),
    }
}

fn terminal_score(position: &Position) -> i32 {
    let (black, white) = position.counts();
    let margin = match position.turn {
        Color::Black => black as i32 - white as i32,
        Color::White => white as i32 - black as i32,
    };
    match margin.cmp(&0) {
        std::cmp::Ordering::Greater => WIN_SCORE + margin * 100,
        std::cmp::Ordering::Less => -WIN_SCORE + margin * 100,
        std::cmp::Ordering::Equal => 0,
    }
}

fn order_moves(position: &Position, moves: &mut [LegalMove]) {
    moves.sort_by(|left, right| {
        move_priority(position, right.index)
            .cmp(&move_priority(position, left.index))
            .then_with(|| left.index.cmp(&right.index))
    });
}

fn move_priority(position: &Position, index: usize) -> i32 {
    if is_corner(index) {
        return 10_000;
    }
    if let Some(corner) = x_square_corner(index) {
        if position.board[corner] != Some(position.turn) {
            return -10_000;
        }
    }
    square_weight(position, index, position.turn)
}

fn square_weight(position: &Position, index: usize, color: Color) -> i32 {
    if let Some(corner) = x_square_corner(index) {
        if position.board[corner] == Some(color) {
            return 15;
        }
    }
    SQUARE_WEIGHTS[index]
}

fn is_corner(index: usize) -> bool {
    matches!(index, 0 | 7 | 56 | 63)
}

fn x_square_corner(index: usize) -> Option<usize> {
    match index {
        9 => Some(0),
        14 => Some(7),
        49 => Some(56),
        54 => Some(63),
        _ => None,
    }
}

fn is_frontier(position: &Position, index: usize) -> bool {
    let row = (index / 8) as i32;
    let column = (index % 8) as i32;
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let r = row + dr;
            let c = column + dc;
            if (0..8).contains(&r)
                && (0..8).contains(&c)
                && position.board[(r * 8 + c) as usize].is_none()
            {
                return true;
            }
        }
    }
    false
}
