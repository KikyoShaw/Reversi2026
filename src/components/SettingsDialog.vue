<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import type { Settings } from '../types'
import AppDialog from './AppDialog.vue'
import AppIcon from './AppIcon.vue'

const props = defineProps<{ open: boolean; settings: Settings; busy: boolean }>()
const emit = defineEmits<{ close: []; save: [settings: Settings] }>()
const id = useId()
const draft = ref<Settings>({ ...props.settings })
watch(() => props.open, open => { if (open) draft.value = { ...props.settings } })
const lan = computed(() => props.settings.mode === 'lan')
const changedRules = computed(() => ['mode', 'humanColor', 'difficulty', 'stepSeconds', 'playerName'].some(key => draft.value[key as keyof Settings] !== props.settings[key as keyof Settings]))
const valid = computed(() => draft.value.playerName.trim().length >= 1 && Array.from(draft.value.playerName.trim()).length <= 24 &&
  Number.isInteger(draft.value.stepSeconds) && draft.value.stepSeconds >= 0 && draft.value.stepSeconds <= 600)
function save() {
  if (!valid.value || props.busy) return
  emit('save', { ...draft.value, playerName: draft.value.playerName.trim(), difficulty: Number(draft.value.difficulty), stepSeconds: Number(draft.value.stepSeconds) })
}
</script>

<template>
  <AppDialog :open="open" title="对局设置" subtitle="找到适合自己的节奏，认真享受每一步。" @close="emit('close')">
    <form :id="`${id}-form`" class="settings-form" @submit.prevent="save">
      <p v-if="lan" class="info-note">联机房间的规则在创建时确定。音效与落点提示可独立调整。</p>
      <fieldset :disabled="lan || busy" class="form-fieldset">
        <legend>对局方式</legend>
        <div class="segmented-control">
          <button type="button" :aria-pressed="draft.mode === 'computer'" :class="{ selected: draft.mode === 'computer' }" @click="draft.mode = 'computer'">与电脑对弈</button>
          <button type="button" :aria-pressed="draft.mode === 'local'" :class="{ selected: draft.mode === 'local' }" @click="draft.mode = 'local'">双人同屏</button>
        </div>
        <p v-if="lan" class="field-help">当前为局域网对战，离开房间后可切换。</p>
      </fieldset>
      <div class="form-grid">
        <label class="field" :for="`${id}-name`"><span>你的称呼</span><input :id="`${id}-name`" v-model="draft.playerName" :disabled="lan || busy" type="text" maxlength="24" placeholder="棋友" autocomplete="nickname" required /><small>1–24 个字符</small></label>
        <label class="field" :for="`${id}-timer`"><span>每步限时 <span class="optional-label">秒</span></span><input :id="`${id}-timer`" v-model.number="draft.stepSeconds" :disabled="lan || busy" type="number" min="0" max="600" step="1" required /><small>0 为不限时，最多 600 秒</small></label>
      </div>
      <fieldset v-if="draft.mode === 'computer' || lan" :disabled="lan || busy" class="form-fieldset">
        <legend>你的执棋</legend>
        <div class="color-options">
          <label :class="{ selected: draft.humanColor === 'black' }"><input v-model="draft.humanColor" type="radio" value="black" :name="`${id}-color`" /><span class="choice-disc disc-black" />黑棋 · 先行</label>
          <label :class="{ selected: draft.humanColor === 'white' }"><input v-model="draft.humanColor" type="radio" value="white" :name="`${id}-color`" /><span class="choice-disc disc-white" />白棋 · 后手</label>
        </div>
      </fieldset>
      <fieldset v-if="draft.mode === 'computer'" :disabled="lan || busy" class="form-fieldset">
        <legend>电脑棋力</legend>
        <div class="difficulty-options">
          <label v-for="(level, index) in ['轻松', '进阶', '挑战']" :key="level" :class="{ selected: draft.difficulty === index + 1 }"><input v-model="draft.difficulty" type="radio" :value="index + 1" :name="`${id}-difficulty`" /><strong>{{ level }}</strong><small>{{ ['熟悉棋盘', '认真较量', '深思熟虑'][index] }}</small></label>
        </div>
      </fieldset>
      <div class="preferences-list">
        <label class="switch-field"><span><strong>落子音效</strong><small>轻柔的落子与翻棋声</small></span><input v-model="draft.sound" :disabled="busy" type="checkbox" role="switch" /><span class="switch-track" aria-hidden="true" /></label>
        <label class="switch-field"><span><strong>合法落点提示</strong><small>用圆点标记当前可落子的位置</small></span><input v-model="draft.showHints" :disabled="busy" type="checkbox" role="switch" /><span class="switch-track" aria-hidden="true" /></label>
      </div>
      <p v-if="!valid" class="field-error">请填写称呼，并将每步限时设为 0–600 的整数。</p>
      <p v-else-if="changedRules && !lan" class="field-help">修改对局规则将创建一局等待开始的新棋局。</p>
    </form>
    <template #footer><button type="button" class="button button-quiet" @click="emit('close')">取消</button><button type="submit" :form="`${id}-form`" class="button button-primary" :disabled="!valid || busy"><AppIcon name="check" />{{ busy ? '正在保存…' : changedRules && !lan ? '保存并新开一局' : '保存设置' }}</button></template>
  </AppDialog>
</template>
