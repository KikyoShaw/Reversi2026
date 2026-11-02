import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { GameAction, Hint, Settings, Snapshot } from '../types'

const sameTurn = (a: Pick<Snapshot, 'revision' | 'sessionId' | 'gameId' | 'turnToken'>, b: Pick<Snapshot, 'revision' | 'sessionId' | 'gameId' | 'turnToken'>) =>
  a.revision === b.revision && a.sessionId === b.sessionId && a.gameId === b.gameId && a.turnToken === b.turnToken

export function useGame() {
  const snapshot = shallowRef<Snapshot | null>(null)
  const hint = shallowRef<Hint | null>(null)
  const hintLoading = ref(false)
  const connecting = ref(true)
  const error = ref('')
  const notice = ref('')
  const waiting = shallowRef<{ id: number; base: Snapshot; remote: boolean } | null>(null)
  const now = ref(performance.now())
  let anchorTime = now.value
  let sequence = 0
  let hintSequence = 0
  let unlisten: UnlistenFn | undefined
  let disposed = false
  let initialized = false
  let initialization: Promise<void> | null = null
  let ticker: ReturnType<typeof setInterval> | undefined
  let noticeTimer: ReturnType<typeof setTimeout> | undefined

  const working = computed(() => waiting.value !== null)
  const waitingRemote = computed(() => waiting.value?.remote ?? false)
  const clockRunning = computed(() => {
    const state = snapshot.value
    return !!state && state.phase === 'running' && !state.result &&
      (state.settings.mode !== 'lan' || state.network.status === 'connected')
  })
  const remainingMs = computed(() => {
    const state = snapshot.value
    if (!state || state.remainingMs === null) return null
    return Math.max(0, state.remainingMs - (clockRunning.value ? now.value - anchorTime : 0))
  })

  function notify(message: string) {
    notice.value = message
    if (noticeTimer) clearTimeout(noticeTimer)
    noticeTimer = setTimeout(() => { notice.value = '' }, 6500)
  }

  function accept(next: Snapshot) {
    if (disposed) return false
    const previous = snapshot.value
    // Both event and command responses share this gate; late responses cannot roll back the board.
    if (previous && (next.sessionId < previous.sessionId || next.revision <= previous.revision)) return false
    snapshot.value = next
    anchorTime = performance.now()
    now.value = anchorTime
    if (hint.value && !sameTurn(next, hint.value)) hint.value = null
    if (previous && !sameTurn(previous, next)) {
      hintSequence += 1
      hintLoading.value = false
    }
    if (waiting.value && (next.revision > waiting.value.base.revision ||
      next.sessionId !== waiting.value.base.sessionId || next.network.status === 'disconnected' ||
      next.network.status === 'offline')) waiting.value = null
    if (previous && next.network.message && next.network.message !== previous.network.message) notify(next.network.message)
    return true
  }

  function failure(reason: unknown) {
    error.value = reason instanceof Error ? reason.message : String(reason)
  }

  async function initialize() {
    if (initialization) return initialization
    if (disposed) return
    connecting.value = true
    error.value = ''
    initialization = (async () => {
      try {
        if (!('__TAURI_INTERNALS__' in window)) {
          throw new Error('当前页面未连接桌面后端。请在项目目录使用 npm run tauri dev 启动桌面应用，然后重试。')
        }
        if (!initialized) {
          const stop = await listen<Snapshot>('game:snapshot', event => accept(event.payload))
          if (disposed) { stop(); return }
          unlisten = stop
          initialized = true
        }
        const state = await invoke<Snapshot>('get_snapshot')
        accept(state)
      } catch (reason) {
        if (!disposed) failure(reason)
      } finally {
        if (!disposed) connecting.value = false
        initialization = null
      }
    })()
    return initialization
  }

  async function mutate(command: string, args: Record<string, unknown> = {}, remote = false) {
    if (working.value || disposed) return false
    const base = snapshot.value
    const id = ++sequence
    if (base) waiting.value = { id, base, remote }
    error.value = ''
    try {
      const next = await invoke<Snapshot>(command, args)
      if (disposed) return false
      accept(next)
      // A guest acknowledgement may contain its old snapshot. Only a later revision releases it.
      if (waiting.value?.id === id && (!remote || next.network.status === 'disconnected' || next.network.status === 'offline')) {
        waiting.value = null
      }
      return true
    } catch (reason) {
      if (!disposed) {
        failure(reason)
        if (waiting.value?.id === id) waiting.value = null
      }
      return false
    }
  }

  function tokens(state: Snapshot) {
    return { revision: state.revision, sessionId: state.sessionId, gameId: state.gameId, turnToken: state.turnToken }
  }

  async function act(action: GameAction) {
    const state = snapshot.value
    if (!state || working.value) return false
    if (action.type !== 'ready' && state.pendingRequest) return false
    if (action.type === 'start' && (state.result || state.phase !== 'ready' || !state.canStart)) return false
    if ((action.type === 'pause' || action.type === 'resume') && (state.result || !state.canControl)) return false
    if (action.type === 'move' && (!state.canMove || state.result || !state.legalMoves.some(move => move.index === action.index))) return false
    if (action.type === 'undo' && !state.canUndo) return false
    return mutate('game_action', { action, ...tokens(state) }, state.settings.mode === 'lan' && state.network.role === 'guest')
  }

  async function requestHint() {
    const state = snapshot.value
    if (!state || !state.canMove || state.thinking || state.result || working.value || hintLoading.value || state.pendingRequest) return
    const ticket = ++hintSequence
    hintLoading.value = true
    hint.value = null
    error.value = ''
    try {
      const answer = await invoke<Hint>('get_hint', tokens(state))
      const current = snapshot.value
      if (!disposed && ticket === hintSequence && current && sameTurn(current, answer) && current.canMove && !current.thinking && !current.result) hint.value = answer
    } catch (reason) {
      if (!disposed && ticket === hintSequence) failure(reason)
    } finally {
      if (!disposed && ticket === hintSequence) hintLoading.value = false
    }
  }

  function newGame(settings: Settings) { return mutate('new_game', { settings }) }
  function preferences(sound: boolean, showHints: boolean) { return mutate('update_preferences', { sound, showHints }) }
  function host(port: number, settings: Settings) { return mutate('host_lan', { port, settings }) }
  function join(address: string, port: number, playerName: string) { return mutate('join_lan', { address, port, playerName }) }
  function leave() { return mutate('leave_lan') }
  function respond(id: number, acceptRequest: boolean) {
    const state = snapshot.value
    if (!state) return Promise.resolve(false)
    return mutate('respond_request', { id, accept: acceptRequest, ...tokens(state) }, state.network.role === 'guest')
  }

  watch(clockRunning, running => {
    if (ticker) clearInterval(ticker)
    ticker = undefined
    if (running) ticker = setInterval(() => { now.value = performance.now() }, 200)
  })
  onMounted(initialize)
  onBeforeUnmount(() => {
    disposed = true
    hintSequence += 1
    unlisten?.()
    if (ticker) clearInterval(ticker)
    if (noticeTimer) clearTimeout(noticeTimer)
  })

  return { snapshot, hint, hintLoading, connecting, error, notice, working, waitingRemote, remainingMs,
    initialize, act, requestHint, newGame, preferences, host, join, leave, respond }
}
