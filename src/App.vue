<script setup lang="ts">
import { computed, provide, ref, shallowRef, watch } from 'vue'
import AppDialog from './components/AppDialog.vue'
import AppIcon from './components/AppIcon.vue'
import ReversiBoard from './components/ReversiBoard.vue'
import GameSidebar from './components/GameSidebar.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import NetworkDialog from './components/NetworkDialog.vue'
import { useGame } from './composables/useGame'
import { useSound } from './composables/useSound'
import type { Settings } from './types'

const game = useGame()
const { snapshot, hint, hintLoading, connecting, working, waitingRemote, remainingMs, error, notice, requestHint, initialize } = game
useSound(snapshot)
provide('game-error', error)
type Dialog = 'settings' | 'network' | 'rules' | 'result' | 'request' | 'confirm' | null
const dialog = ref<Dialog>(null)
const confirmation = shallowRef<{ title: string; body: string; label: string; danger?: boolean; back: Dialog; run: () => Promise<boolean> } | null>(null)
const isLan = computed(() => snapshot.value?.settings.mode === 'lan')
const pending = computed(() => snapshot.value?.pendingRequest)
const ownRequest = computed(() => !!pending.value && pending.value.from === snapshot.value?.network.localColor)
const hasProgress = computed(() => !!snapshot.value && (snapshot.value.history.length > 0 || snapshot.value.phase !== 'ready' || !!snapshot.value.result))
const modeText = computed(() => snapshot.value ? ({ computer: '人机对弈', local: '双人同屏', lan: '局域网对战' }[snapshot.value.settings.mode]) : '桌面黑白棋')
const phaseLabel = computed(() => {
  const state = snapshot.value
  if (!state) return '开始对局'
  if (state.result) return '本局结束'
  if (state.phase === 'ready') return state.settings.mode === 'lan' && state.network.role === 'guest' ? '等待房主' : '开始对局'
  if (state.settings.mode === 'lan' && state.network.role === 'guest') return state.phase === 'paused' ? '等待房主继续' : '房主控制暂停'
  return state.phase === 'running' ? '暂停对局' : '继续对局'
})
const phaseEnabled = computed(() => {
  const state = snapshot.value
  if (!state || state.result || working.value || state.pendingRequest) return false
  if (state.phase === 'ready') return state.canStart && (state.settings.mode !== 'lan' || (state.network.status === 'connected' && state.network.localReady && state.network.peerReady))
  return state.canControl && (state.settings.mode !== 'lan' || state.network.status === 'connected')
})
const hintEnabled = computed(() => !!snapshot.value?.canMove && !snapshot.value?.thinking && !snapshot.value?.result && !pending.value && !working.value && !hintLoading.value)
const canNew = computed(() => !!snapshot.value && !working.value && !pending.value && (!isLan.value || snapshot.value.network.status === 'connected'))
const canResign = computed(() => !!snapshot.value && !snapshot.value.result && snapshot.value.phase !== 'ready' && !working.value && !pending.value && (!isLan.value || snapshot.value.network.status === 'connected'))
const boardTitle = computed(() => {
  const state = snapshot.value
  if (!state) return ''
  if (state.result) return state.result.winner ? `${state.result.winner === 'black' ? '黑棋' : '白棋'}获胜` : '握手言和'
  if (state.phase === 'ready') return '下一局，从这里开始'
  if (state.phase === 'paused') return '稍作停留，再想一步'
  return state.thinking ? '电脑正在思考' : `轮到${state.turn === 'black' ? '黑棋' : '白棋'}`
})
const resultDescription = computed(() => snapshot.value?.result ? ({ noMoves: '双方均无合法落点，按最终棋子数结算。', timeout: '一方超过每步限时，本局结束。', resign: '一方选择认输，本局结束。' }[snapshot.value.result.reason]) : '')
const liveSummary = computed(() => {
  const state = snapshot.value
  if (!state) return ''
  return `${state.result ? boardTitle.value : ({ ready: '等待开始', running: `${state.turn === 'black' ? '黑棋' : '白棋'}回合`, paused: '对局暂停' }[state.phase])}。黑棋 ${state.blackCount} 枚，白棋 ${state.whiteCount} 枚。${state.thinking ? '电脑正在思考。' : ''}`
})

function closeDialog() {
  if (dialog.value === 'confirm') { dialog.value = confirmation.value?.back ?? null; confirmation.value = null }
  else dialog.value = null
}
function confirm(title: string, body: string, label: string, run: () => Promise<boolean>, danger = false, back: Dialog = dialog.value) {
  confirmation.value = { title, body, label, run, danger, back }
  dialog.value = 'confirm'
}
async function executeConfirmation() {
  const current = confirmation.value
  if (!current || working.value) return
  if (await current.run()) {
    if (confirmation.value === current) { if (dialog.value === 'confirm') dialog.value = null; confirmation.value = null }
  }
}
async function phaseAction() {
  if (!phaseEnabled.value || !snapshot.value) return
  await game.act({ type: snapshot.value.phase === 'ready' ? 'start' : snapshot.value.phase === 'running' ? 'pause' : 'resume' })
}
async function startNew() {
  const state = snapshot.value
  if (!state || !canNew.value) return
  if (state.settings.mode === 'lan') {
    confirm('申请重新开局', '新的一局需要对方同意。同意后棋盘将重置，双方重新准备，再由房主开始。等待回应时，当前回合仍按原状态计时。', '发送请求', () => game.act({ type: 'restart' }))
  } else if (hasProgress.value) {
    confirm('开始新的一局？', '当前棋盘与落子记录将被替换。新棋局会等待你点击“开始对局”。', '新开一局', () => game.newGame({ ...state.settings }))
  } else {
    if (await game.newGame({ ...state.settings })) dialog.value = null
  }
}
async function undo() {
  if (!snapshot.value?.canUndo || working.value || pending.value) return
  if (await game.act({ type: 'undo' })) {
    if (dialog.value === 'result') dialog.value = null
  }
}
function resign() {
  if (!canResign.value) return
  confirm('确定认输吗？', '认输后本局将结束，对方获胜。', '确认认输', () => game.act({ type: 'resign' }), true)
}
async function saveSettings(settings: Settings) {
  const state = snapshot.value
  if (!state || working.value) return
  const rulesChanged = (['mode', 'humanColor', 'difficulty', 'stepSeconds', 'playerName'] as const).some(key => settings[key] !== state.settings[key])
  if (state.settings.mode === 'lan' || !rulesChanged) {
    if (settings.sound === state.settings.sound && settings.showHints === state.settings.showHints) { dialog.value = null; return }
    if (await game.preferences(settings.sound, settings.showHints)) dialog.value = null
  } else if (hasProgress.value) {
    confirm('应用设置并新开一局？', '修改规则将替换当前棋盘与记录，新棋局等待手动开始。', '应用并新开一局', () => game.newGame(settings), false, 'settings')
  } else if (await game.newGame(settings)) dialog.value = null
}
function hostRoom(port: number, settings: Settings) {
  if (hasProgress.value) {
    confirm('创建房间并替换当前棋局？', '房间会使用新的棋盘。等待朋友加入、双方准备后，由你开始。', '创建房间', () => game.host(port, settings), false, 'network')
  } else void game.host(port, settings)
}
function joinRoom(address: string, port: number, playerName: string) {
  if (hasProgress.value) {
    confirm('加入房间并替换当前棋局？', '将同步房主的棋局。加入后点击准备，等待房主开始。', '加入房间', () => game.join(address, port, playerName), false, 'network')
  } else void game.join(address, port, playerName)
}
function leaveRoom() {
  confirm('离开局域网房间？', '棋盘、落子记录与剩余时间会保留，转为暂停的双人同屏对局。尚未开始的棋局仍等待开始。', '离开房间', () => game.leave(), false, 'network')
}
async function respond(accept: boolean) {
  if (!pending.value || ownRequest.value || working.value) return
  const id = pending.value.id
  if (await game.respond(id, accept)) { if (dialog.value === 'request') dialog.value = null }
}
function placeMove(index: number) { void game.act({ type: 'move', index }) }
function setReady(value: boolean) { void game.act({ type: 'ready', value }) }
async function toggleSound() {
  const state = snapshot.value
  if (state && !working.value) await game.preferences(!state.settings.sound, state.settings.showHints)
}

let lastResult = ''
watch(snapshot, state => {
  if (!state) return
  if (!state.result) {
    lastResult = ''
    if (dialog.value === 'result') dialog.value = null
  } else {
    const key = `${state.sessionId}:${state.gameId}:${state.result.winner}:${state.result.reason}:${state.result.blackCount}:${state.result.whiteCount}`
    if (key !== lastResult) { lastResult = key; dialog.value = 'result' }
  }
}, { immediate: true })
watch(() => pending.value ? `${snapshot.value?.sessionId}:${pending.value.id}` : '', (key, previous) => {
  if (key && key !== previous && !ownRequest.value) dialog.value = 'request'
  else if (!key && dialog.value === 'request') dialog.value = null
})
</script>

<template>
  <div class="app-shell">
    <header class="app-header">
      <div class="brand" aria-label="黑白棋 Reversi"><span class="brand-mark" aria-hidden="true"><i /><i /></span><span><strong>黑白棋 <span class="brand-divider">·</span> <span class="brand-english">Reversi</span></strong><small>黑白之间，自有新局</small></span></div>
      <nav class="header-actions" aria-label="对局操作">
        <button type="button" class="button button-quiet" :disabled="!canNew" @click="startNew"><AppIcon name="plus" />新一局</button>
        <button type="button" class="button button-primary phase-button" :disabled="!phaseEnabled" @click="phaseAction"><AppIcon :name="snapshot?.phase === 'running' && !snapshot?.result ? 'pause' : 'play'" />{{ phaseLabel }}</button>
        <button type="button" class="button button-quiet" :disabled="!snapshot?.canUndo || working || !!pending" :title="isLan ? '向对方申请悔棋' : '退回上一手；人机模式退回到你上次落子前'" @click="undo"><AppIcon name="undo" />悔棋</button>
        <span class="toolbar-divider" aria-hidden="true" />
        <button type="button" class="button button-quiet" :disabled="!snapshot" @click="dialog = 'settings'"><AppIcon name="settings" />设置</button>
        <button type="button" class="button button-quiet" :class="{ 'network-active': isLan }" :disabled="!snapshot" @click="dialog = 'network'"><AppIcon name="network" />联机</button>
        <button type="button" class="icon-button" :disabled="!snapshot || working" :aria-label="snapshot?.settings.sound ? '关闭音效' : '开启音效'" :title="snapshot?.settings.sound ? '关闭音效' : '开启音效'" :aria-pressed="!!snapshot?.settings.sound" @click="toggleSound"><AppIcon :name="snapshot?.settings.sound ? 'sound' : 'mute'" /></button>
        <button type="button" class="icon-button" aria-label="游戏规则" title="游戏规则" @click="dialog = 'rules'"><AppIcon name="help" /></button>
      </nav>
    </header>

    <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{{ liveSummary }}</div>
    <div v-if="error || notice" class="toast-region">
      <div v-if="error" class="toast toast-error" role="alert"><span>{{ error }}</span><button type="button" class="icon-button" aria-label="关闭错误提示" @click="error = ''"><AppIcon name="close" :size="16" /></button></div>
      <div v-else-if="notice" class="toast" role="status"><span>{{ notice }}</span><button type="button" class="icon-button" aria-label="关闭通知" @click="notice = ''"><AppIcon name="close" :size="16" /></button></div>
    </div>

    <main v-if="snapshot" class="app-main">
      <section class="board-panel surface" aria-label="对弈区">
        <div class="board-panel-heading"><div><span class="eyebrow"><span class="mini-disc" />{{ modeText }}</span><h1>{{ boardTitle }}</h1></div><span v-if="snapshot.result" class="phase-badge">已结束</span><span v-else class="phase-badge" :class="{ 'badge-running': snapshot.phase === 'running' }">{{ { ready: '等待开始', running: '对弈中', paused: '已暂停' }[snapshot.phase] }}</span></div>
        <p v-if="snapshot.network.message && isLan" class="network-inline"><AppIcon name="network" :size="15" />{{ snapshot.network.message }}</p>
        <ReversiBoard :snapshot="snapshot" :busy="working" :hint="hint" @move="placeMove" />
        <div class="board-bottom-bar">
          <button type="button" class="button button-soft hint-button" :disabled="!hintEnabled" @click="requestHint"><AppIcon name="bulb" />{{ hintLoading ? '正在找好棋…' : hint ? '再想一步' : '给我提示' }}</button>
          <span class="board-bottom-note">{{ snapshot.settings.stepSeconds === 0 ? '不限时，慢慢思考' : `每步 ${snapshot.settings.stepSeconds} 秒` }}</span>
          <button type="button" class="text-button resign-button" :disabled="!canResign" @click="resign"><AppIcon name="flag" :size="15" />认输</button>
        </div>
        <div v-if="working || pending" class="pending-banner" role="status">
          <span v-if="pending">{{ ownRequest ? '已发送' : '收到' }}{{ pending.kind === 'undo' ? '悔棋' : '重新开局' }}请求，等待{{ ownRequest ? '对方' : '你的' }}回应。</span>
          <span v-else>{{ waitingRemote ? '请求已发送，等待房主同步…' : '正在处理…' }}</span>
          <button v-if="pending" type="button" class="text-button" @click="dialog = 'request'">{{ ownRequest ? '查看请求' : '回应请求' }}<AppIcon name="arrow" :size="15" /></button>
        </div>
      </section>
      <GameSidebar :snapshot="snapshot" :remaining-ms="remainingMs" :busy="working" @ready="setReady" @request="dialog = 'request'" />
    </main>
    <main v-else class="connection-page"><div class="connection-card surface"><span class="large-brand-mark brand-mark" aria-hidden="true"><i /><i /></span><h1>{{ connecting ? '正在摆好棋盘…' : '还未连接桌面应用' }}</h1><p>{{ connecting ? '即将开始一场黑白之间的较量。' : '请通过 npm run tauri dev 启动桌面应用，完整的规则与联机功能由桌面后端提供。' }}</p><button v-if="!connecting" type="button" class="button button-primary" @click="initialize"><AppIcon name="refresh" />重新连接</button></div></main>
    <footer class="app-footer"><span>64 格天地，无限种可能。</span><button type="button" class="text-button" @click="dialog = 'rules'">初次见面？了解规则<AppIcon name="arrow" :size="14" /></button></footer>

    <template v-if="snapshot">
      <SettingsDialog :open="dialog === 'settings'" :settings="snapshot.settings" :busy="working" @close="closeDialog" @save="saveSettings" />
      <NetworkDialog :open="dialog === 'network'" :snapshot="snapshot" :busy="working" @close="closeDialog" @host="hostRoom" @join="joinRoom" @leave="leaveRoom" @ready="setReady" />
      <AppDialog :open="dialog === 'result'" :title="boardTitle" :subtitle="resultDescription" @close="closeDialog">
        <template v-if="snapshot.result">
          <div class="result-emblem" aria-hidden="true"><span v-if="snapshot.result.winner" class="result-disc" :class="`disc-${snapshot.result.winner}`" /><span v-else class="draw-discs"><i class="disc-black" /><i class="disc-white" /></span><AppIcon name="spark" :size="25" /></div>
          <div class="result-scoreline"><div><span><i class="history-disc disc-black" />黑棋</span><strong>{{ snapshot.result.blackCount }}</strong></div><span class="result-score-divider">:</span><div><span><i class="history-disc disc-white" />白棋</span><strong>{{ snapshot.result.whiteCount }}</strong></div></div>
          <p class="result-note">{{ isLan ? '新的一局需要双方同意，并重新准备。' : '再来一局，试试另一种走法。' }}</p>
        </template>
        <template #footer><button type="button" class="button button-quiet" @click="closeDialog">查看棋盘</button><button v-if="snapshot.canUndo" type="button" class="button button-soft" :disabled="working || !!pending" @click="undo"><AppIcon name="undo" />{{ isLan ? '申请悔棋' : '悔棋' }}</button><button type="button" class="button button-primary" data-autofocus :disabled="!canNew" @click="startNew"><AppIcon name="plus" />再来一局</button></template>
      </AppDialog>
      <AppDialog :open="dialog === 'request' && !!pending" :title="pending?.kind === 'undo' ? '悔棋请求' : '重新开局请求'" :subtitle="ownRequest ? '请求已送出，等待对方回应。' : `${snapshot.network.peerName || '对方'}希望${pending?.kind === 'undo' ? '退回上一手' : '重新开始一局'}。`" @close="closeDialog">
        <p class="request-description">{{ pending?.kind === 'undo' ? '同意后由房主恢复棋局。已结束的棋局悔棋后会暂停，等待继续。' : '同意后将重置当前棋盘与记录，双方重新准备，再由房主开始。' }}</p>
        <p class="info-note">请在 30 秒内回应，以房主计时为准。等待请求期间，正在进行的回合继续计时。</p>
        <p v-if="working" class="field-help">正在等待房主同步结果；你可以关闭此窗口查看棋盘。</p>
        <template #footer><button type="button" class="button button-quiet" @click="closeDialog">暂时收起</button><template v-if="!ownRequest"><button type="button" class="button button-soft" :disabled="working || !pending" @click="respond(false)">拒绝</button><button type="button" class="button button-primary" :disabled="working || !pending" @click="respond(true)"><AppIcon name="check" />同意</button></template></template>
      </AppDialog>
    </template>

    <AppDialog :open="dialog === 'confirm'" :title="confirmation?.title ?? '确认操作'" @close="closeDialog"><p class="confirmation-copy">{{ confirmation?.body }}</p><template #footer><button type="button" class="button button-quiet" data-autofocus @click="closeDialog">取消</button><button type="button" class="button" :class="confirmation?.danger ? 'button-danger' : 'button-primary'" :disabled="working" @click="executeConfirmation">{{ working ? '正在处理…' : confirmation?.label }}</button></template></AppDialog>
    <AppDialog :open="dialog === 'rules'" title="黑白棋，落子有方" subtitle="规则简单，变化无穷。一分钟，认识这张棋盘。" @close="closeDialog">
      <ol class="rules-list"><li><span>01</span><div><h3>黑棋先行，轮流落子</h3><p>棋盘开始时，中央各有两枚黑白棋子。点击“开始对局”，再选择合法落点。</p></div></li><li><span>02</span><div><h3>夹住对手，翻为己色</h3><p>新落棋子必须在横、竖或斜线方向，与已有己方棋子夹住至少一枚连续的对方棋子。所有被夹住的棋子都会翻转。</p></div></li><li><span>03</span><div><h3>无棋可下，自动让行</h3><p>没有合法落点时自动跳过回合；双方都无法落子时结算，棋子多的一方获胜，数量相同则平局。</p></div></li><li><span>04</span><div><h3>慢慢想，也可以限时</h3><p>默认不限时。设置每步限时后，超时判负。暂停会停住时钟；联机的悔棋与重开请求需要对方同意，等待回应不会暂停计时。</p></div></li></ol>
      <div class="rules-keyboard"><AppIcon name="bulb" /><p>圆点标记合法落点，悬停可预览翻转；方向键移动焦点，回车或空格落子。“给我提示”只推荐落点，由你决定是否落子。</p></div>
      <template #footer><button type="button" class="button button-primary" data-autofocus @click="closeDialog">明白了，回到棋盘<AppIcon name="arrow" /></button></template>
    </AppDialog>
  </div>
</template>
