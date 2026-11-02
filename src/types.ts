export type Color = 'black' | 'white'
export type GameMode = 'computer' | 'local' | 'lan'
export type Phase = 'ready' | 'running' | 'paused'
export interface Settings {
  mode: GameMode; humanColor: Color; difficulty: number; stepSeconds: number
  sound: boolean; showHints: boolean; playerName: string
}
export interface LegalMove { index: number; flips: number[] }
export interface MoveRecord {
  ply: number; color: Color; index: number; coordinate: string; flips: number[]
  passed: Color | null; blackCount: number; whiteCount: number
}
export interface GameResult {
  winner: Color | null; reason: 'noMoves' | 'timeout' | 'resign'
  blackCount: number; whiteCount: number
}
export interface NetworkView {
  status: 'offline' | 'hosting' | 'connecting' | 'connected' | 'disconnected'
  role: 'host' | 'guest' | null; localColor: Color | null; address: string
  message: string; peerName: string; localReady: boolean; peerReady: boolean
}
export interface PendingRequest { id: number; kind: 'undo' | 'restart'; from: Color }
export interface ActionSignal { id: number; kind: string; index: number | null; flips: number[] }
export interface Snapshot {
  revision: number; sessionId: number; gameId: number; turnToken: number; phase: Phase
  board: (Color | null)[]; turn: Color; legalMoves: LegalMove[]; history: MoveRecord[]
  blackCount: number; whiteCount: number; result: GameResult | null; thinking: boolean
  remainingMs: number | null; settings: Settings; network: NetworkView
  pendingRequest: PendingRequest | null; action: ActionSignal
  canStart: boolean; canControl: boolean; canMove: boolean; canUndo: boolean
}
export type GameAction =
  | { type: 'start' } | { type: 'pause' } | { type: 'resume' }
  | { type: 'move'; index: number } | { type: 'undo' } | { type: 'restart' }
  | { type: 'ready'; value: boolean } | { type: 'resign' }
export interface Hint {
  revision: number; sessionId: number; gameId: number; turnToken: number
  index: number; flips: number[]; depth: number; exact: boolean
}