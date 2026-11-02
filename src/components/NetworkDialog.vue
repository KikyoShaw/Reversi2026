<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import type { Settings, Snapshot } from '../types'
import AppDialog from './AppDialog.vue'
import AppIcon from './AppIcon.vue'

const props = defineProps<{ open: boolean; snapshot: Snapshot; busy: boolean }>()
const emit = defineEmits<{ close: []; host: [port: number, settings: Settings]; join: [address: string, port: number, playerName: string]; leave: []; ready: [value: boolean] }>()
const id = useId()
const tab = ref<'host' | 'join'>('host')
const port = ref(9529)
const address = ref('')
const playerName = ref(props.snapshot.settings.playerName)
const humanColor = ref(props.snapshot.settings.humanColor)
const stepSeconds = ref(props.snapshot.settings.stepSeconds)
const active = computed(() => props.snapshot.settings.mode === 'lan')
const network = computed(() => props.snapshot.network)
const statusLabel = computed(() => ({ offline: '未连接', hosting: '等待对手加入', connecting: '正在连接房主', connected: '对手已连接', disconnected: '连接已断开' }[network.value.status]))
const valid = computed(() => Number.isInteger(port.value) && port.value >= 1 && port.value <= 65535 &&
  playerName.value.trim().length >= 1 && Array.from(playerName.value.trim()).length <= 24 &&
  (tab.value === 'join' ? address.value.trim().length > 0 : Number.isInteger(stepSeconds.value) && stepSeconds.value >= 0 && stepSeconds.value <= 600))
const readyEnabled = computed(() => network.value.status === 'connected' && props.snapshot.phase === 'ready' && !props.snapshot.result && !props.snapshot.pendingRequest && !props.busy)
watch(() => props.open, open => {
  if (open) {
    playerName.value = props.snapshot.settings.playerName
    humanColor.value = props.snapshot.settings.humanColor
    stepSeconds.value = props.snapshot.settings.stepSeconds
  }
})
function submit() {
  if (!valid.value || props.busy) return
  if (tab.value === 'host') emit('host', port.value, { ...props.snapshot.settings, mode: 'lan', humanColor: humanColor.value, stepSeconds: stepSeconds.value, playerName: playerName.value.trim() })
  else emit('join', address.value.trim(), port.value, playerName.value.trim())
}
</script>

<template>
  <AppDialog :open="open" :title="active ? '局域网房间' : '和朋友下一局'" :subtitle="active ? '同一个棋盘，连接两位棋友。' : '连接同一局域网，一人创建房间，一人加入。'" @close="emit('close')">
    <template v-if="active">
      <div class="network-state" :class="{ connected: network.status === 'connected', disconnected: network.status === 'disconnected' }"><span class="connection-symbol"><AppIcon name="network" :size="27" /></span><div><strong>{{ statusLabel }}</strong><p>{{ network.role === 'host' ? '你是房主' : '你是访客' }} · 执{{ network.localColor === 'white' ? '白' : '黑' }}棋</p></div></div>
      <div v-if="network.address" class="room-address"><span>{{ network.role === 'host' ? '将房间地址告诉朋友' : '房主地址' }}</span><code>{{ network.address }}</code></div>
      <div class="room-participants">
        <div><span class="history-disc" :class="`disc-${network.localColor ?? snapshot.settings.humanColor}`" /><span><strong>{{ snapshot.settings.playerName }}</strong><small>你 · {{ network.role === 'host' ? '房主' : '访客' }}</small></span><span class="ready-badge" :class="{ ready: network.localReady }">{{ network.localReady ? '已准备' : '未准备' }}</span></div>
        <div><span class="history-disc" :class="network.localColor === 'white' ? 'disc-black' : 'disc-white'" /><span><strong>{{ network.peerName || '等待棋友加入' }}</strong><small>{{ network.status === 'connected' ? '已连接' : '未连接' }}</small></span><span class="ready-badge" :class="{ ready: network.peerReady }">{{ network.peerReady ? '已准备' : '未准备' }}</span></div>
      </div>
      <p v-if="network.message" class="info-note" role="status">{{ network.message }}</p>
      <template v-if="snapshot.phase === 'ready' && !snapshot.result">
        <button type="button" class="button full-width" :class="network.localReady ? 'button-soft' : 'button-primary'" :disabled="!readyEnabled" @click="emit('ready', !network.localReady)"><AppIcon :name="network.localReady ? 'check' : 'play'" />{{ network.localReady ? '已准备 · 点击取消' : '我准备好了' }}</button>
        <p class="field-help centered">{{ network.role === 'host' ? '双方连接并准备后，可在主界面开始对局。' : '准备就绪后，由房主开始对局。' }}</p>
      </template>
      <p v-if="network.status === 'disconnected'" class="field-help">连接不会自动重建。可离开房间后重新创建或加入。</p>
      <p class="field-help">离开房间会保留棋盘、记录与剩余时间，转为暂停的同屏对局；尚未开始的棋局仍等待开始。</p>
    </template>
    <form v-else :id="`${id}-form`" class="settings-form" @submit.prevent="submit">
      <div class="segmented-control"><button type="button" :class="{ selected: tab === 'host' }" :aria-pressed="tab === 'host'" :disabled="busy" @click="tab = 'host'">创建房间</button><button type="button" :class="{ selected: tab === 'join' }" :aria-pressed="tab === 'join'" :disabled="busy" @click="tab = 'join'">加入房间</button></div>
      <label class="field" :for="`${id}-name`"><span>你的称呼</span><input :id="`${id}-name`" v-model="playerName" type="text" maxlength="24" placeholder="让朋友认出你" autocomplete="nickname" required :disabled="busy" /><small>1–24 个字符</small></label>
      <label v-if="tab === 'join'" class="field" :for="`${id}-address`"><span>房主地址</span><input :id="`${id}-address`" v-model="address" type="text" placeholder="例如 192.168.1.8" spellcheck="false" autocomplete="off" required :disabled="busy" /><small>填写房主显示的 IP 地址，端口在下方填写。</small></label>
      <div class="form-grid">
        <label class="field" :for="`${id}-port`"><span>房间端口</span><input :id="`${id}-port`" v-model.number="port" type="number" min="1" max="65535" step="1" required :disabled="busy" /><small>双方填写相同端口，默认 9529</small></label>
        <label v-if="tab === 'host'" class="field" :for="`${id}-timer`"><span>每步限时 <span class="optional-label">秒</span></span><input :id="`${id}-timer`" v-model.number="stepSeconds" type="number" min="0" max="600" step="1" required :disabled="busy" /><small>0 为不限时，最多 600 秒</small></label>
      </div>
      <fieldset v-if="tab === 'host'" class="form-fieldset" :disabled="busy"><legend>你的执棋</legend><div class="color-options"><label :class="{ selected: humanColor === 'black' }"><input v-model="humanColor" type="radio" value="black" :name="`${id}-color`" /><span class="choice-disc disc-black" />黑棋 · 先行</label><label :class="{ selected: humanColor === 'white' }"><input v-model="humanColor" type="radio" value="white" :name="`${id}-color`" /><span class="choice-disc disc-white" />白棋 · 后手</label></div></fieldset>
      <p class="info-note">{{ tab === 'host' ? '房间创建后等待朋友加入。双方准备，由房主点击开始。' : '加入后点击准备，等待房主开始。无需账号，也不会自动重连。' }}</p>
      <p v-if="!valid" class="field-help">请填写称呼{{ tab === 'join' ? '与房主地址' : '' }}，并确认端口{{ tab === 'host' ? '和限时' : '' }}有效。</p>
    </form>
    <template #footer>
      <template v-if="active"><button type="button" class="button button-danger-quiet" :disabled="busy" @click="emit('leave')"><AppIcon name="leave" />离开房间</button><button type="button" class="button button-primary" @click="emit('close')">回到棋盘</button></template>
      <template v-else><button type="button" class="button button-quiet" @click="emit('close')">取消</button><button type="submit" :form="`${id}-form`" class="button button-primary" :disabled="!valid || busy"><AppIcon name="network" />{{ busy ? '正在连接…' : tab === 'host' ? '创建房间' : '加入房间' }}</button></template>
    </template>
  </AppDialog>
</template>
