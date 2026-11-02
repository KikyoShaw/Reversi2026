<script setup lang="ts">
defineProps<{ name: string; size?: number }>()
</script>

<template>
  <svg :width="size ?? 18" :height="size ?? 18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
    <path v-if="name === 'play'" d="m9 5 11 7-11 7Z" />
    <path v-else-if="name === 'pause'" d="M8 5v14M16 5v14" stroke-width="3" />
    <path v-else-if="name === 'plus'" d="M12 5v14M5 12h14" />
    <path v-else-if="name === 'undo'" d="m8 4-5 5 5 5M3 9h11a6 6 0 0 1 0 12h-2" />
    <template v-else-if="name === 'bulb'"><path d="M8 15a6 6 0 1 1 8 0l-1 3H9l-1-3ZM9 21h6M12 1v1M3 5l1 1M20 6l1-1" /></template>
    <template v-else-if="name === 'settings'"><path d="m9 3-1 3-3 1 1 3-2 2 2 2-1 3 3 1 1 3h6l1-3 3-1-1-3 2-2-2-2 1-3-3-1-1-3Z" /><circle cx="12" cy="12" r="3" /></template>
    <template v-else-if="name === 'network'"><rect x="9" y="2" width="6" height="6" rx="1" /><rect x="2" y="16" width="6" height="6" rx="1" /><rect x="16" y="16" width="6" height="6" rx="1" /><path d="M12 8v4M5 16v-4h14v4" /></template>
    <template v-else-if="name === 'sound' || name === 'mute'"><path d="M4 9h4l5-4v14l-5-4H4Z" /><path v-if="name === 'sound'" d="M17 8a6 6 0 0 1 0 8M20 5a10 10 0 0 1 0 14" /><path v-else d="m17 9 5 6m0-6-5 6" /></template>
    <template v-else-if="name === 'help'"><circle cx="12" cy="12" r="9" /><path d="M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-1 .5-1 1.2-1 2.2M12 17h.01" /></template>
    <path v-else-if="name === 'close'" d="m6 6 12 12M6 18 18 6" />
    <path v-else-if="name === 'check'" d="m5 12 4 4L19 6" />
    <path v-else-if="name === 'leave'" d="M10 4H5v16h5m5-12 4 4-4 4M9 12h10" />
    <path v-else-if="name === 'flag'" d="M5 21V3c5-4 9 4 14 0v10c-5 4-9-4-14 0" />
    <path v-else-if="name === 'arrow'" d="M4 12h15m-6-6 6 6-6 6" />
    <path v-else-if="name === 'refresh'" d="M20 9a8 8 0 1 0 0 6M20 3v6h-6" />
    <template v-else-if="name === 'clock'"><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></template>
    <template v-else-if="name === 'spark'"><path d="m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5Z" /></template>
  </svg>
</template>