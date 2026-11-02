use crate::session::{Hub, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    future::{poll_fn, Future},
    io,
    sync::Arc,
    task::Poll,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
    time::{self, Instant, MissedTickBehavior},
};

pub const PRODUCT: &str = "reversi2026";
pub const PROTOCOL: u16 = 1;

const MAX_FRAME: usize = 65_536;
const MAX_HELLO: usize = 256;
const QUEUE_CAPACITY: usize = 128;
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_TIMEOUT: Duration = Duration::from_secs(35);
const HEARTBEAT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Wire {
    Hello {
        product: String,
        protocol: u16,
        name: String,
    },
    State {
        snapshot: Box<Snapshot>,
    },
    Action {
        request: PeerRequest,
    },
    Respond {
        request: PeerResponse,
    },
    Error {
        message: String,
    },
    Ping,
    Pong,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeerRequest {
    pub request_id: u64,
    pub revision: u64,
    pub session_id: u64,
    pub game_id: u64,
    pub turn_token: u64,
    pub action: crate::session::GameAction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeerResponse {
    pub request_id: u64,
    pub revision: u64,
    pub session_id: u64,
    pub game_id: u64,
    pub turn_token: u64,
    pub id: u64,
    pub accept: bool,
}

#[derive(Clone, Copy)]
enum Role {
    Host,
    Guest,
}

enum Either<A, B> {
    First(A),
    Second(B),
}

// Poll cancellation/control first. Borrowed pinned futures remain alive when
// another branch wins, so a heartbeat never abandons a partially read frame.
async fn race<A: Future, B: Future>(first: A, second: B) -> Either<A::Output, B::Output> {
    let mut first = Box::pin(first);
    let mut second = Box::pin(second);
    poll_fn(move |cx| {
        if let Poll::Ready(value) = first.as_mut().poll(cx) {
            return Poll::Ready(Either::First(value));
        }
        if let Poll::Ready(value) = second.as_mut().poll(cx) {
            return Poll::Ready(Either::Second(value));
        }
        Poll::Pending
    })
    .await
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}

fn is_current(hub: &Hub, token: u64, cancel: &watch::Receiver<bool>) -> bool {
    !*cancel.borrow() && hub.network_is_current(token)
}

pub fn host(
    hub: Arc<Hub>,
    listener: TcpListener,
    token: u64,
    cancel: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(host_loop(hub, listener, token, cancel));
}

pub fn join(
    hub: Arc<Hub>,
    address: String,
    port: u16,
    name: String,
    token: u64,
    mut cancel: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        if !is_current(&hub, token, &cancel) {
            return;
        }
        let result = race(
            cancelled(&mut cancel),
            join_room(&hub, address, port, name, token),
        )
        .await;
        if let Either::Second(Err(message)) = result {
            if is_current(&hub, token, &cancel) {
                hub.network_lost(token, message);
            }
        }
    });
}

async fn host_loop(
    hub: Arc<Hub>,
    listener: TcpListener,
    token: u64,
    mut cancel: watch::Receiver<bool>,
) {
    while is_current(&hub, token, &cancel) {
        let accepted = match race(cancelled(&mut cancel), listener.accept()).await {
            Either::First(()) => return,
            Either::Second(Ok(accepted)) => accepted,
            Either::Second(Err(error)) => {
                if is_current(&hub, token, &cancel) {
                    hub.network_lost(token, format!("无法接受连接：{error}"));
                }
                return;
            }
        };
        let (mut stream, address) = accepted;
        let _ = stream.set_nodelay(true);
        let hello = race(
            cancelled(&mut cancel),
            time::timeout(HELLO_TIMEOUT, host_hello(&mut stream)),
        )
        .await;
        let name = match hello {
            Either::First(()) => return,
            Either::Second(Ok(Ok(name))) => name,
            Either::Second(result) => {
                let message = match result {
                    Ok(Err(message)) => message,
                    Err(_) => "握手超时，请确认对方已打开同版本游戏".to_owned(),
                    Ok(Ok(_)) => unreachable!(),
                };
                if !is_current(&hub, token, &cancel)
                    || !reject(&mut stream, &message, &mut cancel).await
                {
                    return;
                }
                if is_current(&hub, token, &cancel) {
                    hub.network_notice(token, format!("已拒绝来自 {address} 的连接：{message}"));
                }
                continue;
            }
        };
        if !is_current(&hub, token, &cancel) {
            return;
        }
        let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
        let weak_sender = sender.downgrade();
        // Hub queues the first authoritative State before either I/O loop starts.
        // Hub must call network_lost on Full/Closed from its own try_send calls.
        if let Err(message) =
            hub.host_peer_connected(token, name, address.to_string(), sender)
        {
            if !is_current(&hub, token, &cancel)
                || !reject(&mut stream, &message, &mut cancel).await
            {
                return;
            }
            continue;
        }
        // A room accepts exactly one guest. Reconnecting requires a new room.
        drop(listener);
        if !is_current(&hub, token, &cancel) {
            return;
        }
        let result = race(
            cancelled(&mut cancel),
            connected(&hub, stream, token, Role::Host, receiver, weak_sender),
        )
        .await;
        if let Either::Second(Err(message)) = result {
            if is_current(&hub, token, &cancel) {
                hub.network_lost(token, message);
            }
        }
        return;
    }
}

async fn join_room(
    hub: &Arc<Hub>,
    address: String,
    port: u16,
    name: String,
    token: u64,
) -> Result<(), String> {
    validate_name(&name)?;
    let address = address.trim();
    if address.is_empty() {
        return Err("请输入房主的地址".to_owned());
    }
    if port == 0 {
        return Err("端口必须在 1 到 65535 之间".to_owned());
    }
    let mut stream = time::timeout(HELLO_TIMEOUT, TcpStream::connect((address, port)))
        .await
        .map_err(|_| "连接房主超时，请确认地址、端口和局域网连接".to_owned())?
        .map_err(|error| format!("无法连接房主：{error}"))?;
    let _ = stream.set_nodelay(true);
    let handshake = time::timeout(HELLO_TIMEOUT, async {
        write_wire(&mut stream, &hello(name), MAX_HELLO).await?;
        let wire = read_wire(&mut stream, MAX_HELLO).await?;
        validate_hello(wire)?;
        Ok::<(), String>(())
    })
    .await;
    match handshake {
        Ok(Ok(())) => {}
        result => {
            let message = match result {
                Ok(Err(message)) => message,
                Err(_) => "握手超时，请确认房主已打开同版本游戏".to_owned(),
                Ok(Ok(())) => unreachable!(),
            };
            let _ = time::timeout(
                HELLO_TIMEOUT,
                write_wire(
                    &mut stream,
                    &Wire::Error {
                        message: message.clone(),
                    },
                    MAX_FRAME,
                ),
            )
            .await;
            return Err(message);
        }
    }
    if !hub.network_is_current(token) {
        return Ok(());
    }
    let peer_address = stream
        .peer_addr()
        .map(|value| value.to_string())
        .unwrap_or_else(|_| format!("{address}:{port}"));
    let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
    let weak_sender = sender.downgrade();
    if !hub.guest_connected(token, sender, peer_address) {
        return Ok(());
    }
    connected(hub, stream, token, Role::Guest, receiver, weak_sender).await
}

fn hello(name: String) -> Wire {
    Wire::Hello {
        product: PRODUCT.to_owned(),
        protocol: PROTOCOL,
        name,
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    let count = name.chars().count();
    if !(1..=24).contains(&count) || name.trim().is_empty() {
        return Err("玩家名称必须为 1 到 24 个字符".to_owned());
    }
    if name.chars().any(char::is_control) {
        return Err("玩家名称不能包含控制字符".to_owned());
    }
    Ok(())
}

fn validate_hello(wire: Wire) -> Result<String, String> {
    match wire {
        Wire::Hello {
            product,
            protocol,
            name,
        } => {
            if product != PRODUCT {
                return Err("对方运行的不是 Reversi2026".to_owned());
            }
            if protocol != PROTOCOL {
                return Err("联机协议版本不一致，请使用相同版本的游戏".to_owned());
            }
            validate_name(&name)?;
            Ok(name)
        }
        Wire::Error { message } => Err(message),
        _ => Err("握手失败：首条消息必须是 hello".to_owned()),
    }
}

async fn host_hello(stream: &mut TcpStream) -> Result<String, String> {
    let name = validate_hello(read_wire(stream, MAX_HELLO).await?)?;
    write_wire(stream, &hello("房主".to_owned()), MAX_HELLO).await?;
    Ok(name)
}

async fn reject(
    stream: &mut TcpStream,
    message: &str,
    cancel: &mut watch::Receiver<bool>,
) -> bool {
    let error = Wire::Error {
        message: message.to_owned(),
    };
    matches!(
        race(
            cancelled(cancel),
            time::timeout(HELLO_TIMEOUT, write_wire(stream, &error, MAX_FRAME)),
        )
        .await,
        Either::Second(_)
    )
}

struct ReadFailure {
    message: String,
    reply: bool,
}

impl ReadFailure {
    fn protocol(message: String) -> Self {
        Self {
            message,
            reply: true,
        }
    }
}

async fn connected(
    hub: &Arc<Hub>,
    stream: TcpStream,
    token: u64,
    role: Role,
    receiver: mpsc::Receiver<Wire>,
    sender: mpsc::WeakSender<Wire>,
) -> Result<(), String> {
    let (reader, writer) = stream.into_split();
    let (stop_sender, stop_receiver) = oneshot::channel();
    // These futures are created once. Only cancellation or a terminal failure
    // drops the reader; heartbeat ticks run solely inside the writer future.
    let mut reader = Box::pin(read_loop(hub, reader, token, role, sender.clone()));
    let mut writer = Box::pin(write_loop(writer, receiver, sender, stop_receiver));
    match race(reader.as_mut(), writer.as_mut()).await {
        Either::First(Ok(())) => Ok(()),
        Either::First(Err(failure)) => {
            if failure.reply {
                let _ = stop_sender.send(failure.message.clone());
                // Let the sole writer finish its current frame before writing
                // Error. If it is blocked, close without appending a broken frame.
                let _ = time::timeout(HELLO_TIMEOUT, writer.as_mut()).await;
            }
            Err(failure.message)
        }
        Either::Second(result) => result,
    }
}

async fn read_loop(
    hub: &Arc<Hub>,
    mut reader: OwnedReadHalf,
    token: u64,
    role: Role,
    sender: mpsc::WeakSender<Wire>,
) -> Result<(), ReadFailure> {
    let mut received_state = false;
    loop {
        if !hub.network_is_current(token) {
            return Ok(());
        }
        let wire = time::timeout(FRAME_TIMEOUT, read_wire(&mut reader, MAX_FRAME))
            .await
            .map_err(|_| ReadFailure::protocol("长时间未收到对方消息，连接已超时".to_owned()))?
            .map_err(ReadFailure::protocol)?;
        if !hub.network_is_current(token) {
            return Ok(());
        }
        match (role, wire) {
            (_, Wire::Ping) => {
                enqueue(&sender, Wire::Pong).map_err(ReadFailure::protocol)?;
            }
            (_, Wire::Pong) => {}
            (Role::Host, Wire::Action { request }) => hub.receive_peer(token, request),
            (Role::Host, Wire::Respond { request }) => hub.receive_response(token, request),
            (Role::Guest, Wire::State { snapshot }) => {
                hub.receive_snapshot(token, *snapshot)
                    .map_err(ReadFailure::protocol)?;
                received_state = true;
            }
            (Role::Guest, Wire::Error { message }) => {
                if !received_state {
                    return Err(ReadFailure {
                        message,
                        reply: false,
                    });
                }
                hub.network_notice(token, message);
            }
            _ => {
                return Err(ReadFailure::protocol(
                    "联机协议错误：收到不符合当前身份的消息".to_owned(),
                ));
            }
        }
    }
}

// Only Hub retains a strong sender. Releasing Hub's queue closes the connection;
// neither heartbeat nor Pong handling can keep an abandoned connection alive.
fn enqueue(sender: &mpsc::WeakSender<Wire>, wire: Wire) -> Result<(), String> {
    let sender = sender
        .upgrade()
        .ok_or_else(|| "连接已关闭（发送通道已释放）".to_owned())?;
    sender.try_send(wire).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => "发送队列已满，连接已断开".to_owned(),
        mpsc::error::TrySendError::Closed(_) => "连接已关闭（发送通道已关闭）".to_owned(),
    })
}

async fn write_loop(
    mut writer: OwnedWriteHalf,
    mut receiver: mpsc::Receiver<Wire>,
    sender: mpsc::WeakSender<Wire>,
    mut stop: oneshot::Receiver<String>,
) -> Result<(), String> {
    let mut heartbeat = time::interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        match race(
            &mut stop,
            race(heartbeat.tick(), receiver.recv()),
        )
        .await
        {
            Either::First(Ok(message)) => {
                let error = Wire::Error {
                    message: message.clone(),
                };
                let _ = time::timeout(FRAME_TIMEOUT, write_wire(&mut writer, &error, MAX_FRAME))
                    .await;
                return Err(message);
            }
            Either::First(Err(_)) => return Err("连接已关闭".to_owned()),
            Either::Second(Either::First(_)) => {
                if let Err(message) = enqueue(&sender, Wire::Ping) {
                    let error = Wire::Error {
                        message: message.clone(),
                    };
                    let _ = time::timeout(
                        HELLO_TIMEOUT,
                        write_wire(&mut writer, &error, MAX_FRAME),
                    )
                    .await;
                    return Err(message);
                }
            }
            Either::Second(Either::Second(Some(wire))) => {
                time::timeout(FRAME_TIMEOUT, write_wire(&mut writer, &wire, MAX_FRAME))
                    .await
                    .map_err(|_| "发送消息超时，连接已断开".to_owned())??;
            }
            Either::Second(Either::Second(None)) => {
                return Err("连接已关闭（发送通道已释放）".to_owned());
            }
        }
    }
}

async fn read_wire<R: AsyncRead + Unpin>(reader: &mut R, limit: usize) -> Result<Wire, String> {
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|error| read_error(error, "消息头"))?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > limit {
        return Err(format!("联机协议错误：消息长度必须在 1 到 {limit} 字节之间"));
    }
    let mut bytes = vec![0_u8; length];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|error| read_error(error, "消息正文"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("联机协议错误：消息不是有效的 UTF-8 JSON（{error}）"))
}

fn read_error(error: io::Error, part: &str) -> String {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        format!("对方已断开连接（{part}未完整接收）")
    } else {
        format!("接收{part}失败：{error}")
    }
}

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}

impl io::Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "消息超过长度上限"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn write_wire<W: AsyncWrite + Unpin>(
    writer: &mut W,
    wire: &Wire,
    limit: usize,
) -> Result<(), String> {
    let mut frame = BoundedJson {
        bytes: Vec::with_capacity(limit.min(1024)),
        limit,
    };
    serde_json::to_writer(&mut frame, wire)
        .map_err(|error| format!("无法编码联机消息：{error}"))?;
    let header = (frame.bytes.len() as u32).to_be_bytes();
    writer
        .write_all(&header)
        .await
        .map_err(|error| format!("发送消息头失败：{error}"))?;
    writer
        .write_all(&frame.bytes)
        .await
        .map_err(|error| format!("发送消息正文失败：{error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("发送消息失败：{error}"))
}
