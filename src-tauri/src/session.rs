use std::{collections::HashSet, path::PathBuf, sync::{Arc, Mutex, MutexGuard, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, watch};
use crate::{game::{ai, Color, Game, GameResult, LegalMove, MoveRecord}, network::{self, PeerRequest, PeerResponse, Wire}, storage};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GameMode { Computer, Local, Lan }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase { Ready, Running, Paused }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub mode: GameMode, pub human_color: Color, pub difficulty: u8, pub step_seconds: u32,
    pub sound: bool, pub show_hints: bool, pub player_name: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self { mode: GameMode::Computer, human_color: Color::Black, difficulty: 2,
            step_seconds: 0, sound: true, show_hints: true, player_name: "我".into() }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        validate_name(&self.player_name)?;
        if !(1..=3).contains(&self.difficulty) { return Err("请选择有效的电脑难度".into()); }
        if self.step_seconds > 600 { return Err("每步时限应为 0～600 秒，0 表示不限时".into()); }
        Ok(())
    }
}
fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
        return Err("昵称应为 1～24 个字符，不能包含控制字符".into());
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GameAction {
    Start, Pause, Resume, Move { index: usize }, Undo, Restart, Ready { value: bool }, Resign,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkView {
    pub status: String, pub role: Option<String>, pub local_color: Option<Color>, pub address: String,
    pub message: String, pub peer_name: String, pub local_ready: bool, pub peer_ready: bool,
}
impl Default for NetworkView {
    fn default() -> Self { Self { status: "offline".into(), role: None, local_color: None, address: String::new(),
        message: "离线对局".into(), peer_name: String::new(), local_ready: false, peer_ready: false } }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRequest { pub id: u64, pub kind: String, pub from: Color }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSignal { pub id: u64, pub kind: String, pub index: Option<usize>, pub flips: Vec<usize> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: u64, pub session_id: u64, pub game_id: u64, pub turn_token: u64, pub phase: Phase,
    pub board: Vec<Option<Color>>, pub turn: Color, pub legal_moves: Vec<LegalMove>, pub history: Vec<MoveRecord>,
    pub black_count: usize, pub white_count: usize, pub result: Option<GameResult>, pub thinking: bool,
    pub remaining_ms: Option<u64>, pub settings: Settings, pub network: NetworkView,
    pub pending_request: Option<PendingRequest>, pub action: ActionSignal,
    pub can_start: bool, pub can_control: bool, pub can_move: bool, pub can_undo: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub revision: u64, pub session_id: u64, pub game_id: u64, pub turn_token: u64,
    pub index: usize, pub flips: Vec<usize>, pub depth: u8, pub exact: bool,
}
struct Session {
    game: Game, settings: Settings, phase: Phase,
    revision: u64, session_id: u64, game_id: u64, turn_token: u64,
    deadline: Option<Instant>, frozen: Option<Duration>, thinking: bool,
    ai_cancel: Arc<AtomicBool>, hint_cancel: Option<Arc<AtomicBool>>,
    network: NetworkView, generation: u64, network_cancel: Option<watch::Sender<bool>>,
    sender: Option<mpsc::Sender<Wire>>, remote: Option<Snapshot>,
    request_id: u64, last_peer_request: u64,
    pending: Option<PendingRequest>, pending_at: Option<Instant>, action: ActionSignal,
}
impl Session {
    fn new(settings: Settings) -> Self {
        let mut state = Self { game: Game::new(), settings, phase: Phase::Ready,
            revision: 1, session_id: 1, game_id: 1, turn_token: 1, deadline: None, frozen: None,
            thinking: false, ai_cancel: Arc::new(AtomicBool::new(false)), hint_cancel: None,
            network: NetworkView::default(), generation: 0, network_cancel: None, sender: None, remote: None,
            request_id: 0, last_peer_request: 0, pending: None, pending_at: None,
            action: ActionSignal { id: 0, kind: "ready".into(), index: None, flips: Vec::new() } };
        state.reset_clock();
        state
    }
    fn guest(&self) -> bool { self.network.role.as_deref() == Some("guest") }
    fn connected(&self) -> bool { self.settings.mode != GameMode::Lan || self.network.status == "connected" }
    fn running(&self) -> bool { self.phase == Phase::Running && self.game.result.is_none() && self.connected() }
    fn touch(&mut self) { self.revision += 1; }
    fn cancel_searches(&mut self) {
        self.ai_cancel.store(true, Ordering::Relaxed);
        if let Some(cancel) = self.hint_cancel.take() { cancel.store(true, Ordering::Relaxed); }
        self.thinking = false;
    }
    fn remaining_time(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|deadline| deadline.saturating_duration_since(now)).or(self.frozen)
    }
    fn remaining_ms(&self) -> Option<u64> { self.remaining_time(Instant::now()).map(|time| time.as_millis() as u64) }
    fn freeze(&mut self, now: Instant) { self.frozen = self.remaining_time(now); self.deadline = None; }
    fn resume_clock(&mut self) { self.deadline = self.frozen.take().map(|remaining| Instant::now() + remaining); }
    fn reset_clock(&mut self) {
        self.deadline = None;
        self.frozen = if self.settings.step_seconds > 0 && self.game.result.is_none() {
            Some(Duration::from_secs(u64::from(self.settings.step_seconds)))
        } else { None };
        if self.running() { self.resume_clock(); }
    }
    fn clear_pending(&mut self) { self.pending = None; self.pending_at = None; }
    fn signal(&mut self, kind: &str, index: Option<usize>, flips: Vec<usize>) {
        self.action.id += 1;
        self.action.kind = kind.into(); self.action.index = index; self.action.flips = flips;
    }
    fn changed(&mut self, kind: &str, index: Option<usize>, flips: Vec<usize>) {
        self.cancel_searches(); self.turn_token += 1; self.clear_pending(); self.reset_clock();
        self.signal(kind, index, flips); self.touch();
    }
    fn reset_game(&mut self) {
        self.game = Game::new(); self.game_id += 1; self.phase = Phase::Ready;
        self.network.local_ready = false; self.network.peer_ready = false;
        self.changed("newGame", None, Vec::new());
    }
    fn own_color(&self) -> Color {
        if self.settings.mode == GameMode::Local { self.game.position.turn }
        else if self.settings.mode == GameMode::Lan { self.network.local_color.unwrap_or(self.settings.human_color) }
        else { self.settings.human_color }
    }
    fn undo_plies(&self) -> Option<usize> {
        if self.game.history.is_empty() { return None; }
        if self.settings.mode == GameMode::Computer {
            self.game.history.iter().rposition(|entry| entry.color == self.settings.human_color)
                .map(|index| self.game.history.len() - index)
        } else { Some(1) }
    }
    fn can_start(&self) -> bool {
        !self.guest() && self.phase == Phase::Ready && self.game.result.is_none() && self.pending.is_none()
            && self.connected() && (self.settings.mode != GameMode::Lan || (self.network.local_ready && self.network.peer_ready))
    }
    fn can_control(&self) -> bool {
        !self.guest() && self.phase != Phase::Ready && self.game.result.is_none() && self.pending.is_none() && self.connected()
    }
    fn can_move(&self) -> bool {
        self.running() && !self.thinking && self.game.position.turn == self.own_color()
    }
    fn can_undo(&self) -> bool { self.undo_plies().is_some() && self.connected() && self.pending.is_none() }
    fn snapshot(&self) -> Snapshot {
        if let Some(remote) = self.remote.as_ref().filter(|_| self.guest()) {
            let mut view = remote.clone();
            view.revision = self.revision; view.session_id = self.session_id;
            view.phase = self.phase; view.settings = self.settings.clone(); view.network = self.network.clone();
            view.remaining_ms = self.remaining_ms(); view.action = self.action.clone();
            view.thinking = false; view.can_start = false; view.can_control = false;
            view.can_move = self.can_move(); view.can_undo = self.can_undo();
            view.pending_request = self.pending.clone();
            return view;
        }
        let (black_count, white_count) = self.game.position.counts();
        Snapshot { revision: self.revision, session_id: self.session_id, game_id: self.game_id, turn_token: self.turn_token,
            phase: self.phase, board: self.game.position.board.clone(), turn: self.game.position.turn,
            legal_moves: if self.game.result.is_none() { self.game.position.legal_moves() } else { Vec::new() },
            history: self.game.history.clone(), black_count, white_count, result: self.game.result.clone(), thinking: self.thinking,
            remaining_ms: self.remaining_ms(), settings: self.settings.clone(), network: self.network.clone(),
            pending_request: self.pending.clone(), action: self.action.clone(), can_start: self.can_start(),
            can_control: self.can_control(), can_move: self.can_move(), can_undo: self.can_undo() }
    }
    fn finish(&mut self, loser: Color, reason: &str) {
        let (black_count, white_count) = self.game.position.counts();
        self.game.result = Some(GameResult { winner: Some(loser.other()), reason: reason.into(), black_count, white_count });
        self.changed("finish", None, Vec::new());
    }
    fn expire(&mut self, now: Instant) -> bool {
        if !self.guest() && self.running() && self.deadline.is_some_and(|deadline| now >= deadline) {
            self.finish(self.game.position.turn, "timeout"); true
        } else { false }
    }
    fn apply_move(&mut self, index: usize) -> Result<(), String> {
        if !self.running() { return Err("请先开始或继续对局".into()); }
        self.game.apply_move(index)?;
        let flips = self.game.history.last().map(|entry| entry.flips.clone()).unwrap_or_default();
        self.changed(if self.game.result.is_some() { "finish" } else { "move" }, Some(index), flips);
        Ok(())
    }
    fn perform_undo(&mut self) -> Result<(), String> {
        let plies = self.undo_plies().ok_or("没有可以撤销的玩家落子")?;
        let ended = self.game.result.is_some();
        self.game.undo(plies)?;
        if ended { self.phase = Phase::Paused; }
        self.changed("undo", None, Vec::new());
        Ok(())
    }
    fn request(&mut self, kind: &str, color: Color) -> Result<(), String> {
        if !self.connected() { return Err("请先连接对手".into()); }
        if self.pending.is_some() { return Err("请先处理当前协商请求".into()); }
        if kind == "undo" && self.game.history.is_empty() { return Err("尚无可撤销的落子".into()); }
        self.request_id += 1;
        self.pending = Some(PendingRequest { id: self.request_id, kind: kind.into(), from: color });
        self.pending_at = Some(Instant::now()); self.touch();
        Ok(())
    }
    fn respond(&mut self, id: u64, accept: bool, color: Color) -> Result<(), String> {
        let pending = self.pending.clone().ok_or("请求已失效")?;
        if pending.id != id { return Err("请求已失效".into()); }
        if pending.from == color { return Err("请等待对方回复".into()); }
        self.clear_pending();
        if accept {
            match pending.kind.as_str() {
                "undo" => self.perform_undo()?,
                "restart" => self.reset_game(),
                _ => return Err("未知协商类型".into()),
            }
            self.network.message = "双方已同意请求".into();
        } else { self.network.message = "对方已拒绝请求".into(); self.touch(); }
        Ok(())
    }
    fn validate_envelope(&self, revision: u64, session_id: u64, game_id: u64, turn_token: u64, ready: bool) -> Result<(), String> {
        if session_id != self.session_id || game_id != self.game_id || revision > self.revision
            || (!ready && turn_token != self.turn_token) { return Err("棋局已更新，请重新操作".into()); }
        Ok(())
    }
    fn user_action(&mut self, action: GameAction, color: Color, host: bool) -> Result<(), String> {
        match action {
            GameAction::Move { index } => {
                if !self.running() || self.game.position.turn != color { return Err("当前不能落子".into()); }
                self.apply_move(index)
            }
            GameAction::Start => {
                if !host || !self.can_start() { return Err("请等待双方准备，并由房主开始".into()); }
                self.phase = Phase::Running; self.turn_token += 1; self.resume_clock(); self.touch(); Ok(())
            }
            GameAction::Pause => {
                if !host || !self.can_control() || self.phase != Phase::Running { return Err("当前不能暂停，或请等待房主操作".into()); }
                let now = Instant::now();
                if self.expire(now) { return Err("本步已超时".into()); }
                self.freeze(now); self.cancel_searches(); self.phase = Phase::Paused; self.turn_token += 1; self.touch(); Ok(())
            }
            GameAction::Resume => {
                if !host || !self.can_control() || self.phase != Phase::Paused { return Err("当前不能继续，或请等待房主操作".into()); }
                self.phase = Phase::Running; self.turn_token += 1; self.resume_clock(); self.touch(); Ok(())
            }
            GameAction::Undo => if self.settings.mode == GameMode::Lan { self.request("undo", color) } else { self.perform_undo() },
            GameAction::Restart => if self.settings.mode == GameMode::Lan { self.request("restart", color) } else { self.reset_game(); Ok(()) },
            GameAction::Ready { value } => {
                if self.settings.mode != GameMode::Lan || self.phase != Phase::Ready || !self.connected() || self.game.result.is_some() {
                    return Err("当前不能修改准备状态".into());
                }
                if host { self.network.local_ready = value; } else { self.network.peer_ready = value; }
                self.touch(); Ok(())
            }
            GameAction::Resign => {
                if self.phase == Phase::Ready || self.game.result.is_some() || !self.connected() { return Err("请先开始对局".into()); }
                self.finish(color, "resign"); Ok(())
            }
        }
    }
    fn stop_network(&mut self) {
        self.generation += 1;
        if let Some(cancel) = self.network_cancel.take() { let _ = cancel.send(true); }
        self.sender = None; self.remote = None; self.last_peer_request = 0;
    }
}

pub struct Hub { inner: Mutex<Session>, app: AppHandle, directory: PathBuf }
impl Hub {
    pub fn new(app: AppHandle, directory: PathBuf) -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(Session::new(storage::load(&directory))), app, directory })
    }
    fn lock(&self) -> MutexGuard<'_, Session> { self.inner.lock().unwrap_or_else(|error| error.into_inner()) }
    pub fn snapshot(&self) -> Snapshot { self.lock().snapshot() }
    pub fn shutdown(&self) { let mut state = self.lock(); state.cancel_searches(); state.stop_network(); }
    pub fn refresh(self: &Arc<Self>) -> Snapshot {
        let (view, sender, token, job) = {
            let mut state = self.lock();
            let job = if !state.guest() && state.running() && state.settings.mode == GameMode::Computer
                && state.game.position.turn != state.settings.human_color && !state.thinking {
                state.ai_cancel = Arc::new(AtomicBool::new(false)); state.thinking = true; state.touch();
                Some((state.game.position.clone(), state.settings.difficulty, state.ai_cancel.clone(),
                    state.session_id, state.game_id, state.turn_token,
                    state.remaining_ms().map(|time| Duration::from_millis(time.saturating_sub(80).max(1)))))
            } else { None };
            (state.snapshot(), if !state.guest() { state.sender.clone() } else { None }, state.generation, job)
        };
        let _ = self.app.emit("game:snapshot", &view);
        if let Some(sender) = sender {
            if sender.try_send(Wire::State { snapshot: Box::new(view.clone()) }).is_err() {
                self.network_lost(token, "连接发送队列不可用，请重新创建房间".into());
            }
        }
        if let Some((position, difficulty, cancel, session_id, game_id, turn_token, budget)) = job {
            let hub = self.clone();
            tauri::async_runtime::spawn(async move {
                let flag = cancel.clone();
                let answer = tauri::async_runtime::spawn_blocking(move || ai::search(position, difficulty, flag, budget)).await;
                let changed = {
                    let mut state = hub.lock();
                    if cancel.load(Ordering::Relaxed) || state.session_id != session_id || state.game_id != game_id
                        || state.turn_token != turn_token || !state.running() || state.settings.mode != GameMode::Computer
                        || state.game.position.turn == state.settings.human_color { false }
                    else if state.expire(Instant::now()) { true }
                    else {
                        state.thinking = false;
                        let index = answer.ok().flatten().map(|result| result.index)
                            .or_else(|| state.game.position.legal_moves().first().map(|mv| mv.index));
                        if let Some(index) = index { let _ = state.apply_move(index); }
                        else { state.touch(); }
                        true
                    }
                };
                if changed { hub.refresh(); }
            });
        }
        view
    }
    pub fn start_clock(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                let Some(hub) = weak.upgrade() else { break; };
                let changed = {
                    let mut state = hub.lock();
                    if state.expire(Instant::now()) { true }
                    else if !state.guest() && state.pending_at.is_some_and(|at| at.elapsed() >= Duration::from_secs(30)) {
                        state.clear_pending(); state.network.message = "协商请求已超时".into(); state.touch(); true
                    } else { false }
                };
                if changed { hub.refresh(); }
            }
        });
    }
    pub fn new_game(self: &Arc<Self>, mut settings: Settings) -> Result<Snapshot, String> {
        settings.player_name = settings.player_name.trim().into(); settings.validate()?;
        {
            let mut state = self.lock();
            if state.settings.mode == GameMode::Lan || settings.mode == GameMode::Lan { return Err("请先离开房间，联机请使用创建/加入房间".into()); }
            storage::save(&self.directory, &settings)?;
            state.settings = settings; state.session_id += 1; state.reset_game();
        }
        Ok(self.refresh())
    }
    pub fn preferences(self: &Arc<Self>, sound: bool, show_hints: bool) -> Result<Snapshot, String> {
        {
            let mut state = self.lock(); let mut settings = state.settings.clone();
            settings.sound = sound; settings.show_hints = show_hints; storage::save(&self.directory, &settings)?;
            state.settings = settings; state.touch();
        }
        Ok(self.refresh())
    }
    pub fn action(self: &Arc<Self>, action: GameAction, revision: u64, session_id: u64, game_id: u64, turn_token: u64) -> Result<Snapshot, String> {
        let result = {
            let mut state = self.lock();
            state.validate_envelope(revision, session_id, game_id, turn_token, matches!(&action, GameAction::Ready { .. }))?;
            if state.guest() {
                if state.network.status != "connected" { return Err("连接尚未建立或已断开".into()); }
                if matches!(&action, GameAction::Start | GameAction::Pause | GameAction::Resume) { return Err("开始、暂停和继续由房主控制".into()); }
                let remote = state.remote.as_ref().ok_or("请等待房主同步棋局")?;
                let mut request = PeerRequest { request_id: 0, revision: remote.revision, session_id: remote.session_id,
                    game_id: remote.game_id, turn_token: remote.turn_token, action };
                state.request_id += 1; request.request_id = state.request_id;
                let queued = state.sender.as_ref().is_some_and(|sender| sender.try_send(Wire::Action { request }).is_ok());
                if !queued { let token = state.generation; drop(state); self.network_lost(token, "连接发送队列已关闭".into()); return Err("连接已断开".into()); }
                return Ok(state.snapshot());
            }
            if state.expire(Instant::now()) { Err("本步已超时".into()) }
            else { let color = state.own_color(); state.user_action(action, color, true) }
        };
        let view = self.refresh(); result.map(|_| view)
    }
    pub fn respond(self: &Arc<Self>, id: u64, accept: bool, revision: u64, session_id: u64, game_id: u64, turn_token: u64) -> Result<Snapshot, String> {
        let result = {
            let mut state = self.lock(); state.validate_envelope(revision, session_id, game_id, turn_token, false)?;
            if !state.connected() || state.settings.mode != GameMode::Lan { return Err("连接已断开".into()); }
            if state.guest() {
                let remote = state.remote.as_ref().ok_or("请等待棋局同步")?;
                let mut request = PeerResponse { request_id: 0, revision: remote.revision, session_id: remote.session_id,
                    game_id: remote.game_id, turn_token: remote.turn_token, id, accept };
                state.request_id += 1; request.request_id = state.request_id;
                let queued = state.sender.as_ref().is_some_and(|sender| sender.try_send(Wire::Respond { request }).is_ok());
                if !queued { let token = state.generation; drop(state); self.network_lost(token, "连接发送队列已关闭".into()); return Err("连接已断开".into()); }
                return Ok(state.snapshot());
            }
            if state.expire(Instant::now()) { Err("请求已失效，本步已超时".into()) }
            else { let color = state.own_color(); state.respond(id, accept, color) }
        };
        let view = self.refresh(); result.map(|_| view)
    }
    pub async fn hint(self: &Arc<Self>, revision: u64, session_id: u64, game_id: u64, turn_token: u64) -> Result<Hint, String> {
        let (position, difficulty, cancel, budget) = {
            let mut state = self.lock(); state.validate_envelope(revision, session_id, game_id, turn_token, false)?;
            if !state.can_move() { return Err("请在自己的行棋回合获取提示".into()); }
            if let Some(old) = state.hint_cancel.take() { old.store(true, Ordering::Relaxed); }
            let cancel = Arc::new(AtomicBool::new(false)); state.hint_cancel = Some(cancel.clone());
            (state.game.position.clone(), state.settings.difficulty, cancel,
                state.remaining_ms().map(|time| Duration::from_millis(time.saturating_sub(80).max(1))))
        };
        let flag = cancel.clone();
        let answer = tauri::async_runtime::spawn_blocking(move || ai::search(position, difficulty, flag, budget)).await
            .map_err(|error| format!("提示暂不可用：{error}"))?.ok_or("当前无法给出提示")?;
        let mut state = self.lock();
        if cancel.load(Ordering::Relaxed) || state.session_id != session_id || state.game_id != game_id
            || state.turn_token != turn_token || !state.can_move() { return Err("棋局已更新，提示已失效".into()); }
        let flips = state.game.position.legal_moves().into_iter().find(|mv| mv.index == answer.index).ok_or("提示已失效")?.flips;
        state.hint_cancel = None;
        Ok(Hint { revision: state.revision, session_id, game_id, turn_token,
            index: answer.index, flips, depth: answer.depth, exact: answer.exact })
    }
    pub fn begin_host(self: &Arc<Self>, listener: tokio::net::TcpListener, mut settings: Settings, port: u16) -> Result<Snapshot, String> {
        settings.player_name = settings.player_name.trim().into(); settings.mode = GameMode::Lan; settings.validate()?;
        let (token, receiver) = {
            let mut state = self.lock(); if state.settings.mode == GameMode::Lan { return Err("请先离开当前房间".into()); }
            storage::save(&self.directory, &settings)?; state.cancel_searches(); state.stop_network();
            state.settings = settings; state.session_id += 1;
            state.network = NetworkView { status: "hosting".into(), role: Some("host".into()), local_color: Some(state.settings.human_color),
                address: format!("0.0.0.0:{port}"), message: format!("等待对手，请告知对方本机局域网 IP 和端口 {port}"), ..NetworkView::default() };
            state.reset_game();
            let (cancel, receiver) = watch::channel(false); state.network_cancel = Some(cancel); (state.generation, receiver)
        };
        network::host(self.clone(), listener, token, receiver); Ok(self.refresh())
    }
    pub fn begin_join(self: &Arc<Self>, address: String, port: u16, name: String) -> Result<Snapshot, String> {
        validate_name(&name)?;
        if address.trim().is_empty() || address.len() > 253 || port == 0 { return Err("请输入有效的主机地址与端口".into()); }
        let (token, receiver) = {
            let mut state = self.lock(); if state.settings.mode == GameMode::Lan { return Err("请先离开当前房间".into()); }
            let mut settings = state.settings.clone(); settings.player_name = name.trim().into(); storage::save(&self.directory, &settings)?;
            state.cancel_searches(); state.stop_network(); state.settings = settings; state.settings.mode = GameMode::Lan; state.session_id += 1;
            state.network = NetworkView { status: "connecting".into(), role: Some("guest".into()), address: format!("{}:{port}", address.trim()),
                message: "正在连接房主…".into(), ..NetworkView::default() }; state.reset_game();
            let (cancel, receiver) = watch::channel(false); state.network_cancel = Some(cancel); (state.generation, receiver)
        };
        network::join(self.clone(), address.trim().into(), port, name.trim().into(), token, receiver); Ok(self.refresh())
    }
    pub fn leave(self: &Arc<Self>) -> Result<Snapshot, String> {
        {
            let mut state = self.lock();
            let mut settings = state.settings.clone(); settings.mode = GameMode::Local; storage::save(&self.directory, &settings)?;
            state.expire(Instant::now()); state.freeze(Instant::now()); state.cancel_searches();
            state.stop_network(); state.settings = settings; state.session_id += 1;
            state.network = NetworkView { message: "已转为本地双人，棋局和棋谱保留，请手动开始或继续".into(), ..NetworkView::default() };
            if state.phase == Phase::Running { state.phase = Phase::Paused; }
            state.turn_token += 1; state.clear_pending(); state.touch();
        }
        Ok(self.refresh())
    }
    pub fn network_is_current(&self, token: u64) -> bool { self.lock().generation == token }
    pub fn host_peer_connected(self: &Arc<Self>, token: u64, name: String, address: String, sender: mpsc::Sender<Wire>) -> Result<(), String> {
        validate_name(&name)?;
        {
            let mut state = self.lock();
            if state.generation != token || state.network.role.as_deref() != Some("host") || state.sender.is_some() { return Err("房间已关闭或已有对手".into()); }
            state.sender = Some(sender); state.network.status = "connected".into(); state.network.peer_name = name;
            state.network.address = address; state.network.message = "对手已连接，双方准备后由房主开始".into(); state.touch();
        }
        self.refresh(); Ok(())
    }
    pub fn guest_connected(self: &Arc<Self>, token: u64, sender: mpsc::Sender<Wire>, address: String) -> bool {
        {
            let mut state = self.lock(); if state.generation != token || !state.guest() { return false; }
            state.sender = Some(sender); state.network.status = "connected".into(); state.network.address = address;
            state.network.message = "已连接，等待房主同步棋局".into(); state.touch();
        }
        self.refresh(); true
    }
    pub fn receive_peer(self: &Arc<Self>, token: u64, request: PeerRequest) {
        let error = {
            let mut state = self.lock(); if state.generation != token || state.guest() || state.network.status != "connected" { return; }
            if request.request_id <= state.last_peer_request { None }
            else {
                state.last_peer_request = request.request_id;
                let result = state.validate_envelope(request.revision, request.session_id, request.game_id, request.turn_token,
                    matches!(&request.action, GameAction::Ready { .. }));
                match result {
                    Err(error) => Some(error),
                    Ok(()) => if state.expire(Instant::now()) { Some("本步已超时".into()) }
                        else { let color = state.own_color().other(); state.user_action(request.action, color, false).err() },
                }
            }
        };
        if let Some(message) = error { self.send_error(token, message); }
        self.refresh();
    }
    pub fn receive_response(self: &Arc<Self>, token: u64, request: PeerResponse) {
        let error = {
            let mut state = self.lock(); if state.generation != token || state.guest() || state.network.status != "connected" { return; }
            if request.request_id <= state.last_peer_request { None }
            else {
                state.last_peer_request = request.request_id;
                match state.validate_envelope(request.revision, request.session_id, request.game_id, request.turn_token, false) {
                    Err(error) => Some(error),
                    Ok(()) => if state.expire(Instant::now()) { Some("请求已失效，本步已超时".into()) }
                        else { let color = state.own_color().other(); state.respond(request.id, request.accept, color).err() },
                }
            }
        };
        if let Some(message) = error { self.send_error(token, message); }
        self.refresh();
    }
    fn send_error(self: &Arc<Self>, token: u64, message: String) {
        let sender = self.lock().sender.clone();
        if let Some(sender) = sender {
            if sender.try_send(Wire::Error { message }).is_err() { self.network_lost(token, "连接不可用".into()); }
        }
    }
    pub fn receive_snapshot(self: &Arc<Self>, token: u64, view: Snapshot) -> Result<(), String> {
        {
            let state = self.lock(); if state.generation != token || !state.guest() { return Ok(()); }
            if state.remote.as_ref().is_some_and(|old| view.session_id == old.session_id && view.revision <= old.revision) { return Ok(()); }
        }
        let game = validate_snapshot(&view)?;
        {
            let mut state = self.lock(); if state.generation != token || !state.guest() { return Ok(()); }
            state.cancel_searches(); state.game = game; state.game_id = view.game_id; state.turn_token = view.turn_token; state.phase = view.phase;
            state.settings.mode = GameMode::Lan; state.settings.human_color = view.settings.human_color.other();
            state.settings.step_seconds = view.settings.step_seconds; state.settings.difficulty = view.settings.difficulty;
            state.network.local_color = Some(view.network.local_color.ok_or("房主未分配执色")?.other());
            state.network.peer_name = view.settings.player_name.clone(); state.network.message = view.network.message.clone();
            state.network.local_ready = view.network.peer_ready; state.network.peer_ready = view.network.local_ready;
            state.pending = view.pending_request.clone(); state.pending_at = None;
            state.frozen = view.remaining_ms.map(Duration::from_millis);
            state.deadline = if view.phase == Phase::Running && view.result.is_none() { state.frozen.take().map(|time| Instant::now() + time) } else { None };
            if state.remote.as_ref().is_some_and(|old| old.action.id != view.action.id) {
                state.signal(&view.action.kind, view.action.index, view.action.flips.clone());
            }
            state.remote = Some(view); state.touch();
        }
        self.refresh(); Ok(())
    }
    pub fn network_lost(self: &Arc<Self>, token: u64, message: String) {
        {
            let mut state = self.lock(); if state.generation != token || state.network.status == "disconnected" { return; }
            state.expire(Instant::now()); state.freeze(Instant::now()); state.cancel_searches();
            if state.phase == Phase::Running { state.phase = Phase::Paused; }
            if let Some(cancel) = state.network_cancel.take() { let _ = cancel.send(true); }
            state.generation += 1; state.sender = None; state.network.status = "disconnected".into();
            state.network.message = format!("连接已断开，棋局保留：{message}"); state.network.local_ready = false; state.network.peer_ready = false;
            state.clear_pending(); state.touch();
        }
        let _ = self.app.emit("game:snapshot", self.snapshot());
    }
    pub fn network_notice(self: &Arc<Self>, token: u64, message: String) {
        {
            let mut state = self.lock(); if state.generation != token { return; }
            state.network.message = message; state.touch();
        }
        self.refresh();
    }
}
fn validate_snapshot(view: &Snapshot) -> Result<Game, String> {
    view.settings.validate()?;
    if view.settings.mode != GameMode::Lan || view.board.len() != 64 || view.history.len() > 60
        || view.network.role.as_deref() != Some("host") || view.network.local_color != Some(view.settings.human_color)
        || view.remaining_ms.is_some_and(|time| time > 600_000)
        || view.legal_moves.iter().any(|mv| mv.index >= 64 || mv.flips.len() > 63 || mv.flips.iter().any(|index| *index >= 64))
        || view.action.index.is_some_and(|index| index >= 64) || view.action.flips.iter().any(|index| *index >= 64) {
        return Err("收到无效的棋局状态".into());
    }
    let game = Game::replay(&view.history, view.result.clone())?;
    let (black, white) = game.position.counts();
    if game.position.board != view.board || game.position.turn != view.turn || game.result != view.result
        || black != view.black_count || white != view.white_count {
        return Err("棋谱与棋盘状态不一致".into());
    }
    let legal = if game.result.is_none() { game.position.legal_moves() } else { Vec::new() };
    let supplied: HashSet<usize> = view.legal_moves.iter().map(|mv| mv.index).collect();
    if supplied.len() != view.legal_moves.len() || supplied.len() != legal.len()
        || legal.iter().any(|mv| !view.legal_moves.iter().any(|other| mv.index == other.index && mv.flips == other.flips)) {
        return Err("收到无效的合法落点".into());
    }
    if let Some(pending) = &view.pending_request {
        if !["undo", "restart"].contains(&pending.kind.as_str()) { return Err("无效的协商请求".into()); }
    }
    Ok(game)
}