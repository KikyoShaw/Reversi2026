<script setup lang="ts">
import { inject, nextTick, onBeforeUnmount, onMounted, ref, useId, watch, type Ref } from 'vue'
import AppIcon from './AppIcon.vue'

const props = withDefaults(defineProps<{ open: boolean; title: string; subtitle?: string; wide?: boolean; closeable?: boolean }>(), { closeable: true })
const emit = defineEmits<{ close: [] }>()
const dialog = ref<HTMLDialogElement | null>(null)
const id = useId()
const gameError = inject<Ref<string>>('game-error')
let previousFocus: HTMLElement | null = null
let alive = true

async function synchronize() {
  await nextTick()
  if (!alive || !dialog.value) return
  if (props.open && !dialog.value.open) {
    previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    dialog.value.showModal()
    const target = dialog.value.querySelector<HTMLElement>('[autofocus], [data-autofocus]') ??
      dialog.value.querySelector<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]')
    ;(target ?? dialog.value).focus()
  } else if (!props.open && dialog.value.open) {
    dialog.value.close()
    if (previousFocus?.isConnected) previousFocus.focus()
    previousFocus = null
  }
}

function keydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    if (props.closeable) emit('close')
  }
  if (event.key !== 'Tab' || !dialog.value) return
  const elements = Array.from(dialog.value.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex="0"]'))
    .filter(element => element.getClientRects().length > 0)
  const first = elements[0]
  const last = elements[elements.length - 1]
  if (!first || !last) { event.preventDefault(); dialog.value.focus(); return }
  if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.value)) { event.preventDefault(); last.focus() }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus() }
}

function backdrop(event: MouseEvent) {
  if (!props.closeable || event.target !== dialog.value || !dialog.value) return
  const bounds = dialog.value.getBoundingClientRect()
  if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) emit('close')
}

watch(() => props.open, synchronize)
onMounted(synchronize)
onBeforeUnmount(() => {
  alive = false
  dialog.value?.close()
  if (previousFocus?.isConnected) previousFocus.focus()
})
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog" class="app-dialog" :class="{ 'dialog--wide': wide }" :aria-labelledby="`${id}-title`" :aria-describedby="subtitle ? `${id}-subtitle` : undefined" tabindex="-1" @keydown="keydown" @cancel.prevent="closeable && emit('close')" @click="backdrop">
      <header class="dialog-header">
        <div><h2 :id="`${id}-title`">{{ title }}</h2><p v-if="subtitle" :id="`${id}-subtitle`">{{ subtitle }}</p></div>
        <button v-if="closeable" type="button" class="icon-button dialog-close" aria-label="关闭弹窗" @click="emit('close')"><AppIcon name="close" /></button>
      </header>
      <div class="dialog-content"><p v-if="gameError" class="dialog-error" role="alert">{{ gameError }}</p><slot /></div>
      <footer v-if="$slots.footer" class="dialog-footer"><slot name="footer" /></footer>
    </dialog>
  </Teleport>
</template>