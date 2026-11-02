<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch, type ComponentPublicInstance } from 'vue'
import type { Color, Hint, Snapshot } from '../types'

const props = defineProps<{ snapshot: Snapshot; busy: boolean; hint: Hint | null }>()
const emit = defineEmits<{ move: [index: number] }>()
const letters = 'ABCDEFGH'.split('')
const cells = Array.from({ length: 8 }, (_, row) => Array.from({ length: 8 }, (_, col) => row * 8 + col))
const hovered = ref<number | null>(null)
const cursor = ref(19)
const keyboardPreview = ref(false)
const buttons: (HTMLButtonElement | null)[] = []
const animation = shallowRef<{ placed: number | null; flips: Map<number, number>; alternate: boolean }>({ placed: null, flips: new Map(), alternate: false })
let animationTimer: ReturnType<typeof setTimeout> | undefined
const interactive = computed(() => props.snapshot.canMove && props.snapshot.phase === 'running' && !props.snapshot.result && !props.snapshot.pendingRequest && !props.busy)
const legal = computed(() => new Map(props.snapshot.legalMoves.map(move => [move.index, move])))
const preview = computed(() => {
  if (!interactive.value) return null
  if (hovered.value !== null && legal.value.has(hovered.value)) return legal.value.get(hovered.value)!
  if (keyboardPreview.value && legal.value.has(cursor.value)) return legal.value.get(cursor.value)!
  if (props.hint && props.hint.revision === props.snapshot.revision) return props.hint
  return null
})
const previewFlips = computed(() => new Set(preview.value?.flips ?? []))
const lastMove = computed(() => props.snapshot.history[props.snapshot.history.length - 1]?.index ?? null)
const coordinate = (index: number) => `${letters[index % 8]}${Math.floor(index / 8) + 1}`
const colorName = (color: Color) => color === 'black' ? '黑棋' : '白棋'

function label(index: number) {
  const color = props.snapshot.board[index]
  const move = legal.value.get(index)
  return `${coordinate(index)}，${color ? colorName(color) : '空位'}${move ? `，合法落点，可翻转 ${move.flips.length} 枚` : ''}${lastMove.value === index ? '，上一手' : ''}`
}
function setButton(element: Element | ComponentPublicInstance | null, index: number) {
  buttons[index] = element instanceof HTMLButtonElement ? element : null
}
function place(index: number) {
  cursor.value = index
  if (interactive.value && legal.value.has(index)) emit('move', index)
}
function keydown(event: KeyboardEvent, index: number) {
  const row = Math.floor(index / 8)
  const col = index % 8
  let target = index
  if (event.key === 'ArrowRight') target = row * 8 + (col + 1) % 8
  else if (event.key === 'ArrowLeft') target = row * 8 + (col + 7) % 8
  else if (event.key === 'ArrowDown') target = ((row + 1) % 8) * 8 + col
  else if (event.key === 'ArrowUp') target = ((row + 7) % 8) * 8 + col
  else if (event.key === 'Home') target = event.ctrlKey ? 0 : row * 8
  else if (event.key === 'End') target = event.ctrlKey ? 63 : row * 8 + 7
  else if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); place(index); return }
  else return
  event.preventDefault()
  cursor.value = target
  hovered.value = null
  keyboardPreview.value = true
  void nextTick(() => { buttons[target]?.focus(); keyboardPreview.value = true })
}

watch(() => props.snapshot, (state, previous) => {
  hovered.value = null
  keyboardPreview.value = false
  const changedAction = previous && (state.action.id !== previous.action.id || state.gameId !== previous.gameId || state.sessionId !== previous.sessionId)
  if ((!previous || changedAction) && animationTimer) clearTimeout(animationTimer)
  if (changedAction && state.action.kind === 'move' && previous.sessionId === state.sessionId && previous.gameId === state.gameId) {
    const flips = state.action.flips.filter(index => previous.board[index] !== state.board[index])
    const placed = state.action.index !== null && previous.board[state.action.index] === null ? state.action.index : null
    animation.value = { placed, flips: new Map(flips.map((index, order) => [index, order])), alternate: !animation.value.alternate }
    animationTimer = setTimeout(() => { animation.value = { placed: null, flips: new Map(), alternate: animation.value.alternate } }, 1100)
  } else if (!previous || state.action.id !== previous.action.id || state.gameId !== previous.gameId || state.sessionId !== previous.sessionId) {
    animation.value = { placed: null, flips: new Map(), alternate: animation.value.alternate }
  }
}, { immediate: true })
watch(interactive, value => { if (!value) { hovered.value = null; keyboardPreview.value = false } })
onBeforeUnmount(() => { if (animationTimer) clearTimeout(animationTimer) })
</script>

<template>
  <div class="reversi-board-area">
    <div class="board-coordinate-frame">
      <div class="coordinate-top" aria-hidden="true"><span v-for="letter in letters" :key="letter">{{ letter }}</span></div>
      <div class="coordinate-left" aria-hidden="true"><span v-for="number in 8" :key="number">{{ number }}</span></div>
      <div class="board-rim">
        <div class="board-grid" role="grid" aria-label="黑白棋棋盘，方向键移动，回车或空格落子" :aria-rowcount="8" :aria-colcount="8" @mouseleave="hovered = null">
          <div v-for="(row, rowIndex) in cells" :key="rowIndex" class="board-row" role="row" :aria-rowindex="rowIndex + 1">
            <button v-for="index in row" :key="index" :ref="element => setButton(element, index)" type="button" role="gridcell"
              class="board-cell" :class="{ 'cell-light': (Math.floor(index / 8) + index % 8) % 2 === 0, 'cell-legal': interactive && legal.has(index), 'cell-hint': interactive && hint?.index === index, 'cell-preview': preview?.index === index, 'cell-flip-preview': previewFlips.has(index) }"
              :tabindex="cursor === index ? 0 : -1" :aria-label="label(index)" :aria-colindex="index % 8 + 1" :aria-disabled="!interactive || !legal.has(index)"
              @click="place(index)" @keydown="keydown($event, index)" @pointerdown="keyboardPreview = false" @mouseenter="hovered = interactive && legal.has(index) ? index : null" @focus="cursor = index" @blur="keyboardPreview = false">
              <span v-if="snapshot.board[index]" class="disc" :class="{ 'disc-placed': animation.placed === index, 'disc-flipped-a': animation.flips.has(index) && !animation.alternate, 'disc-flipped-b': animation.flips.has(index) && animation.alternate }"
                :style="{ '--flip-delay': `${Math.min(animation.flips.get(index) ?? 0, 9) * 28}ms` }" aria-hidden="true">
                <span class="disc-face disc-front" :class="`disc-${snapshot.board[index]}`" />
                <span class="disc-face disc-back" :class="snapshot.board[index] === 'black' ? 'disc-white' : 'disc-black'" />
                <span v-if="lastMove === index" class="last-move-ring" :class="snapshot.board[index] === 'white' ? 'ring-dark' : 'ring-light'" />
              </span>
              <span v-else-if="preview?.index === index" class="ghost-disc" :class="`ghost-${snapshot.turn}`" aria-hidden="true" />
              <span v-else-if="snapshot.settings.showHints && snapshot.phase === 'running' && !snapshot.result && legal.has(index)" class="legal-dot" aria-hidden="true" />
              <span v-if="interactive && hint?.index === index" class="hint-corner" aria-hidden="true" />
            </button>
          </div>
        </div>
      </div>
    </div>
    <div class="board-caption">
      <span v-if="preview"><span class="caption-dot" />{{ coordinate(preview.index) }} · 翻转 {{ preview.flips.length }} 枚</span>
      <span v-else-if="snapshot.result">棋局已结束，每一步都值得回味。</span>
      <span v-else-if="snapshot.phase === 'ready'">黑棋先行，开始后落下第一枚棋子。</span>
      <span v-else-if="snapshot.phase === 'paused'">棋局已暂停，继续后即可落子。</span>
      <span v-else-if="snapshot.thinking">电脑正在思考，你可以随时暂停。</span>
      <span v-else-if="interactive">{{ snapshot.settings.showHints ? '圆点为可落子位置，悬停预览翻转。' : '选择落点，悬停可预览翻转。' }}</span>
      <span v-else>静候对手落子。</span>
      <span class="keyboard-note">方向键移动 · 空格落子</span>
    </div>
  </div>
</template>