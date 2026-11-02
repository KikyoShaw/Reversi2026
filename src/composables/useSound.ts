import { onBeforeUnmount, onMounted, watch, type ShallowRef } from 'vue'
import type { Snapshot } from '../types'

export function useSound(snapshot: ShallowRef<Snapshot | null>) {
  let context: AudioContext | null = null
  let lastSignal = ''
  let active = false

  function unlock() {
    active = true
    if (!snapshot.value?.settings.sound) return
    try {
      context ??= new AudioContext()
      if (context.state === 'suspended') void context.resume().catch(() => {})
    } catch { /* Sound is optional when the device has no audio output. */ }
  }

  function tone(frequency: number, start: number, duration: number, gain: number, type: OscillatorType = 'sine') {
    if (!context || context.state !== 'running') return
    const oscillator = context.createOscillator()
    const volume = context.createGain()
    oscillator.type = type
    oscillator.frequency.setValueAtTime(frequency, start)
    oscillator.frequency.exponentialRampToValueAtTime(Math.max(80, frequency * 0.72), start + duration)
    volume.gain.setValueAtTime(0.0001, start)
    volume.gain.exponentialRampToValueAtTime(gain, start + 0.008)
    volume.gain.exponentialRampToValueAtTime(0.0001, start + duration)
    oscillator.connect(volume)
    volume.connect(context.destination)
    oscillator.start(start)
    oscillator.stop(start + duration + 0.02)
    oscillator.onended = () => { oscillator.disconnect(); volume.disconnect() }
  }

  watch(snapshot, (state, previous) => {
    if (!state) return
    const identity = `${state.sessionId}:${state.gameId}:${state.action.id}`
    if (identity === lastSignal) return
    lastSignal = identity
    if (!previous || !active || !state.settings.sound || state.sessionId !== previous.sessionId || state.gameId !== previous.gameId) return
    unlock()
    if (!context) return
    const start = context.currentTime
    if (state.result && !previous.result) {
      tone(440, start, 0.19, 0.055)
      tone(554.37, start + 0.12, 0.22, 0.045)
      tone(659.25, start + 0.25, 0.30, 0.035)
    } else if (state.action.kind === 'move') {
      tone(420, start, 0.075, 0.075, 'triangle')
      if (state.action.flips.length) tone(640, start + 0.055, 0.11, 0.025)
    }
  })

  onMounted(() => {
    document.addEventListener('pointerdown', unlock, { passive: true })
    document.addEventListener('keydown', unlock)
  })
  onBeforeUnmount(() => {
    document.removeEventListener('pointerdown', unlock)
    document.removeEventListener('keydown', unlock)
    if (context) void context.close().catch(() => {})
    context = null
  })
}
