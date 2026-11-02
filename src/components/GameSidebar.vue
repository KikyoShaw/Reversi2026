<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import type { Color, Snapshot } from '../types'
import AppIcon from './AppIcon.vue'

const props = defineProps<{ snapshot: Snapshot; remainingMs: number | null; busy: boolean }>()
const emit = defineEmits<{ ready: [value: boolean]; request: [] }>()
const historyList = ref<HTMLOListElement | null>(null)
const colors: Color[] = ['black', 'white']
const difficultyNames = ['轻松', '进阶', '挑战']
const isLan = computed(() => props.snapshot.settings.mode === 'lan')
const timeText = computed(() => {
  if (props.remainingMs === null) return '不限时'
  const seconds = Math.ceil(props.remainingMs / 1000)
  return `${Math.floor(seconds / 60).toString().padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`
})
const phaseText = computed(() => props.snapshot.result ? '本局结束' : ({ ready: '准备开局', running: '对局进行中', paused: '已暂停' }[props.snapshot.phase]))
const lastPassed = computed(() => props.snapshot.action.kind === 'move' ? props.snapshot.history[props.snapshot.history.length - 1]?.passed : null)
const networkText = computed(() => ({ offline: '未联机', hosting: '等待对手加入', connecting: '正在连接', connected: '已连接', disconnected: '连接已断开' }[props.snapshot.network.status]))
const readyEnabled = computed(() => isLan.value && props.snapshot.network.status === 'connected' && props.snapshot.phase === 'ready' && !props.snapshot.result && !props.snapshot.pendingRequest && !props.busy)
function playerName(color: Color) {
  const state = props.snapshot
  if (state.settings.mode === 'computer') return state.settings.humanColor === color ? state.settings.playerName : `电脑 · ${difficultyNames[state.settings.difficulty - 1] ?? '进阶'}`
  if (state.settings.mode === 'lan') return state.network.localColor === color ? state.settings.playerName : state.network.peerName || '等待对手'
  return color === 'black' ? '黑棋玩家' : '白棋玩家'
}
function playerDetail(color: Color) {
  const state = props.snapshot
  if (state.settings.mode === 'computer') return state.settings.humanColor === color ? '你' : '电脑对手'
  if (isLan.value) {
    const local = state.network.localColor === color
    const ready = local ? state.network.localReady : state.network.peerReady
    return `${local ? '你' : '对手'}${state.phase === 'ready' ? ready ? ' · 已准备' : ' · 未准备' : ''}`
  }
  return color === 'black' ? '执黑先行' : '执白后手'
}
watch(() => `${props.snapshot.sessionId}:${props.snapshot.gameId}:${props.snapshot.history.length}:${props.snapshot.history[props.snapshot.history.length - 1]?.coordinate ?? ''}`, async () => {
  await nextTick()
  if (historyList.value) historyList.value.scrollTop = historyList.value.scrollHeight
}, { immediate: true })
</script>

<template>
  <aside class="game-sidebar" aria-label="对局信息">
    <section class="players-panel surface">
      <div class="section-heading"><h2>本局对阵</h2><span class="small-muted">{{ snapshot.history.length }} 手</span></div>
      <div v-for="color in colors" :key="color" class="player-card" :class="{ 'player-active': snapshot.turn === color && snapshot.phase !== 'ready' && !snapshot.result, 'player-winner': snapshot.result?.winner === color }">
        <div class="player-piece" :class="`disc-${color}`" aria-hidden="true" />
        <div class="player-info"><strong>{{ playerName(color) }}</strong><span>{{ playerDetail(color) }}</span></div>
        <div class="player-score"><strong>{{ color === 'black' ? snapshot.blackCount : snapshot.whiteCount }}</strong><span>枚</span></div>
        <span v-if="snapshot.turn === color && snapshot.phase !== 'ready' && !snapshot.result" class="player-turn-dot" aria-label="当前回合" />
      </div>
      <div class="turn-summary">
        <span class="status-text"><span class="status-dot" :class="{ 'is-paused': snapshot.phase !== 'running' || snapshot.result }" />{{ phaseText }}</span>
        <span v-if="!snapshot.result && snapshot.phase !== 'ready'">{{ snapshot.turn === 'black' ? '黑棋' : '白棋' }}回合</span>
      </div>
      <div class="turn-clock" :class="{ 'clock-urgent': remainingMs !== null && remainingMs <= 10000 && snapshot.phase === 'running' && !snapshot.result }">
        <span><AppIcon name="clock" /> 每步用时</span><strong :aria-label="remainingMs === null ? '不限时' : `剩余 ${Math.ceil(remainingMs / 1000)} 秒`">{{ timeText }}</strong>
      </div>
      <div v-if="snapshot.thinking && !snapshot.result" class="thinking-note"><span class="thinking-dots" aria-hidden="true"><i /><i /><i /></span>电脑正在思考…</div>
      <p v-if="lastPassed && !snapshot.result" class="pass-note">{{ lastPassed === 'black' ? '黑棋' : '白棋' }}暂无合法落点，已自动跳过。</p>
      <div v-if="isLan" class="room-summary">
        <div><AppIcon name="network" /><span>{{ networkText }}</span><span class="room-role">{{ snapshot.network.role === 'host' ? '房主' : '访客' }}</span></div>
        <button v-if="snapshot.phase === 'ready' && !snapshot.result" type="button" class="button full-width" :class="snapshot.network.localReady ? 'button-soft' : 'button-primary'" :disabled="!readyEnabled" @click="emit('ready', !snapshot.network.localReady)"><AppIcon :name="snapshot.network.localReady ? 'check' : 'play'" />{{ snapshot.network.localReady ? '已准备 · 取消' : '我准备好了' }}</button>
        <p v-if="snapshot.phase === 'ready'" class="small-muted">{{ snapshot.network.role === 'host' ? '双方准备后，由你开始对局。' : '准备后，等待房主开始对局。' }}</p>
      </div>
      <button v-if="snapshot.pendingRequest" type="button" class="request-link" @click="emit('request')">{{ snapshot.pendingRequest.kind === 'undo' ? '悔棋' : '重新开局' }}请求等待回应 <AppIcon name="arrow" :size="15" /></button>
    </section>

    <section class="history-panel surface" aria-label="落子记录">
      <div class="section-heading"><h2>落子记录</h2><span class="small-muted">{{ snapshot.history.length ? '从第一手到此刻' : '故事即将开始' }}</span></div>
      <ol v-if="snapshot.history.length" ref="historyList" class="move-history">
        <li v-for="record in snapshot.history" :key="`${snapshot.gameId}-${record.ply}`">
          <span class="move-number">{{ String(record.ply).padStart(2, '0') }}</span>
          <span class="history-disc" :class="`disc-${record.color}`" :aria-label="record.color === 'black' ? '黑棋' : '白棋'" />
          <strong class="move-coordinate">{{ record.coordinate }}</strong><span class="move-flips">翻转 {{ record.flips.length }} 枚</span>
          <span v-if="record.passed" class="history-pass">{{ record.passed === 'black' ? '黑棋' : '白棋' }}无棋可下，自动跳过</span>
        </li>
      </ol>
      <div v-else class="history-empty"><span class="empty-board-mark" aria-hidden="true"><i /><i /><i /><i /></span><p>每一步，都会留在这里</p><span>开始对局后查看落点与翻转记录</span></div>
    </section>
  </aside>
</template>
