pub mod ai;
pub mod rules;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Color {
    Black,
    White,
}

impl Color {
    pub fn other(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Position {
    pub board: Vec<Option<Color>>,
    pub turn: Color,
}

impl Position {
    pub fn initial() -> Self {
        let mut board = vec![None; 64];
        board[27] = Some(Color::White);
        board[28] = Some(Color::Black);
        board[35] = Some(Color::Black);
        board[36] = Some(Color::White);
        Self {
            board,
            turn: Color::Black,
        }
    }

    pub fn legal_moves(&self) -> Vec<LegalMove> {
        self.moves_for(self.turn)
    }

    pub fn moves_for(&self, color: Color) -> Vec<LegalMove> {
        rules::legal_moves(self, color)
    }

    pub fn counts(&self) -> (usize, usize) {
        let mut black = 0;
        let mut white = 0;
        for cell in &self.board {
            match cell {
                Some(Color::Black) => black += 1,
                Some(Color::White) => white += 1,
                None => {}
            }
        }
        (black, white)
    }

    /// Automatically skips an opponent with no move. If neither side can move,
    /// `passed` is `None` and `turn` remains the opponent of the last mover.
    pub fn after_move(
        &self,
        index: usize,
    ) -> Result<(Position, Vec<usize>, Option<Color>), String> {
        let applied = advance(self, index)?;
        Ok((applied.position, applied.flips, applied.passed))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegalMove {
    pub index: usize,
    pub flips: Vec<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MoveRecord {
    pub ply: usize,
    pub color: Color,
    pub index: usize,
    pub coordinate: String,
    pub flips: Vec<usize>,
    pub passed: Option<Color>,
    pub black_count: usize,
    pub white_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameResult {
    pub winner: Option<Color>,
    pub reason: String,
    pub black_count: usize,
    pub white_count: usize,
}

#[derive(Clone)]
pub struct Game {
    pub position: Position,
    pub history: Vec<MoveRecord>,
    pub result: Option<GameResult>,
    before: Vec<Position>,
}

impl Game {
    pub fn new() -> Self {
        Self {
            position: Position::initial(),
            history: Vec::new(),
            result: None,
            before: Vec::new(),
        }
    }

    pub fn from_position(mut position: Position) -> Result<Self, String> {
        validate_position(&position)?;
        let result = match classify_turn(&mut position) {
            TurnState::Ongoing { .. } => None,
            TurnState::Finished => Some(no_moves_result(&position)),
        };
        Ok(Self {
            position,
            history: Vec::new(),
            result,
            before: Vec::new(),
        })
    }

    pub fn apply_move(&mut self, index: usize) -> Result<(), String> {
        if self.result.is_some() {
            return Err("对局已结束，无法继续落子。".into());
        }
        let applied = advance(&self.position, index)?;
        let (black_count, white_count) = applied.position.counts();
        let record = MoveRecord {
            ply: self.history.len() + 1,
            color: self.position.turn,
            index,
            coordinate: format!("{}{}", (b'A' + (index % 8) as u8) as char, index / 8 + 1),
            flips: applied.flips,
            passed: applied.passed,
            black_count,
            white_count,
        };
        let result = if applied.finished {
            Some(no_moves_result(&applied.position))
        } else {
            None
        };
        let previous = std::mem::replace(&mut self.position, applied.position);
        self.before.push(previous);
        self.history.push(record);
        self.result = result;
        Ok(())
    }

    pub fn undo(&mut self, plies: usize) -> Result<(), String> {
        if plies == 0 || plies > self.history.len() {
            return Err("悔棋步数必须大于零，且不能超过已落子步数。".into());
        }
        if self.before.len() != self.history.len() {
            return Err("悔棋记录不一致，无法悔棋。".into());
        }
        let keep = self.history.len() - plies;
        let position = self.before[keep].clone();
        self.before.truncate(keep);
        self.history.truncate(keep);
        self.position = position;
        self.result = None;
        Ok(())
    }

    pub fn replay(history: &[MoveRecord], result: Option<GameResult>) -> Result<Self, String> {
        if history.len() > 60 {
            return Err("标准对局的落子记录不能超过 60 步。".into());
        }
        let mut game = Self::new();
        for (offset, expected) in history.iter().enumerate() {
            game.apply_move(expected.index)
                .map_err(|error| format!("回放第 {} 步落子无效：{}", offset + 1, error))?;
            if game.history.last() != Some(expected) {
                return Err(format!("回放第 {} 步记录与实际落子结果不一致。", offset + 1));
            }
        }
        if let Some(supplied) = result {
            match supplied.reason.as_str() {
                "noMoves" => {
                    if game.result.as_ref() != Some(&supplied) {
                        return Err("双方无合法落子的终局结果与回放对局不一致。".into());
                    }
                }
                "timeout" | "resign" => {
                    if game.result.is_some() {
                        return Err("对局已因双方无合法落子而结束，不能再以超时或认输结束。".into());
                    }
                    if supplied.winner.is_none() {
                        return Err("超时或认输结束的对局必须指定获胜方。".into());
                    }
                    let (black_count, white_count) = game.position.counts();
                    if supplied.black_count != black_count || supplied.white_count != white_count {
                        return Err("对局结果中的棋子数量与回放棋盘不一致。".into());
                    }
                    game.result = Some(supplied);
                }
                _ => return Err("无法识别对局结束原因。".into()),
            }
        }
        Ok(game)
    }
}

fn validate_position(position: &Position) -> Result<(), String> {
    if position.board.len() != 64 {
        return Err("黑白棋棋盘必须包含 64 个格子。".into());
    }
    // Option<Color> already restricts every cell to black, white, or empty.
    // Imported analysis positions need not be reachable from the initial board.
    Ok(())
}

enum TurnState {
    Ongoing { passed: Option<Color> },
    Finished,
}

fn classify_turn(position: &mut Position) -> TurnState {
    if rules::has_legal_move(position, position.turn) {
        return TurnState::Ongoing { passed: None };
    }
    let skipped = position.turn;
    if rules::has_legal_move(position, skipped.other()) {
        position.turn = skipped.other();
        return TurnState::Ongoing {
            passed: Some(skipped),
        };
    }
    TurnState::Finished
}

struct AppliedMove {
    position: Position,
    flips: Vec<usize>,
    passed: Option<Color>,
    finished: bool,
}

fn advance(position: &Position, index: usize) -> Result<AppliedMove, String> {
    validate_position(position)?;
    if index >= 64 {
        return Err("落子坐标无效，请选择棋盘内的格子。".into());
    }
    if position.board[index].is_some() {
        return Err("该格已有棋子，请选择其他位置。".into());
    }
    let flips = rules::flips(position, position.turn, index);
    if flips.is_empty() {
        return Err("落子必须至少翻转一枚对方棋子。".into());
    }
    let mut next = position.clone();
    next.board[index] = Some(position.turn);
    for &flipped in &flips {
        next.board[flipped] = Some(position.turn);
    }
    next.turn = position.turn.other();
    let (passed, finished) = match classify_turn(&mut next) {
        TurnState::Ongoing { passed } => (passed, false),
        TurnState::Finished => (None, true),
    };
    Ok(AppliedMove {
        position: next,
        flips,
        passed,
        finished,
    })
}

fn no_moves_result(position: &Position) -> GameResult {
    let (black_count, white_count) = position.counts();
    let winner = match black_count.cmp(&white_count) {
        std::cmp::Ordering::Greater => Some(Color::Black),
        std::cmp::Ordering::Less => Some(Color::White),
        std::cmp::Ordering::Equal => None,
    };
    GameResult {
        winner,
        reason: "noMoves".into(),
        black_count,
        white_count,
    }
}
