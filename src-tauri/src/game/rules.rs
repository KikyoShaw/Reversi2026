use super::{Color, LegalMove, Position};

const DIRECTIONS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

/// Returns captured indices in ascending order, independent of ray traversal.
/// Invalid boards, out-of-range indices, and occupied squares have no captures.
pub fn flips(position: &Position, color: Color, index: usize) -> Vec<usize> {
    if position.board.len() != 64 || index >= 64 || position.board[index].is_some() {
        return Vec::new();
    }
    let row = (index / 8) as i32;
    let column = (index % 8) as i32;
    let opponent = color.other();
    let mut captured = Vec::new();
    for (dr, dc) in DIRECTIONS {
        let mut r = row + dr;
        let mut c = column + dc;
        let start = captured.len();
        let mut bracketed = false;
        while (0..8).contains(&r) && (0..8).contains(&c) {
            let square = (r * 8 + c) as usize;
            match position.board[square] {
                Some(occupant) if occupant == opponent => captured.push(square),
                Some(occupant) if occupant == color => {
                    bracketed = captured.len() > start;
                    break;
                }
                _ => break,
            }
            r += dr;
            c += dc;
        }
        if !bracketed {
            captured.truncate(start);
        }
    }
    // Distinct rays from one square cannot overlap, so no deduplication is needed.
    captured.sort_unstable();
    captured
}

pub fn legal_moves(position: &Position, color: Color) -> Vec<LegalMove> {
    if position.board.len() != 64 {
        return Vec::new();
    }
    let mut moves = Vec::new();
    for index in 0..64 {
        if position.board[index].is_none() {
            let captured = flips(position, color, index);
            if !captured.is_empty() {
                moves.push(LegalMove {
                    index,
                    flips: captured,
                });
            }
        }
    }
    moves
}

pub(super) fn has_legal_move(position: &Position, color: Color) -> bool {
    if position.board.len() != 64 {
        return false;
    }
    (0..64).any(|index| {
        position.board[index].is_none() && !flips(position, color, index).is_empty()
    })
}
