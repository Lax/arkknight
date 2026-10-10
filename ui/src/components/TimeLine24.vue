<script setup lang="ts">
// 24 小时时间轴：拖拽画时间窗 / 拖边调整 / 整块平移 / 删除；
// 叠加显示游戏日界（默认 04:00，账号调度语义的日期归属线）与本地定时事项
// （如每晚网络闪断），供选窗时直观避让。分钟网格 5min，最短窗口 15min。
import { computed, ref } from 'vue'

export interface TlWindow {
  start: string
  end: string
  executor: string
  task?: string | null
}
export interface TlEvent {
  name: string
  start: string
  end: string
}

const props = defineProps<{
  windows: TlWindow[]
  boundary?: string
  events?: TlEvent[]
  readonly?: boolean
}>()

const emit = defineEmits<{
  'update:windows': [windows: TlWindow[]]
}>()

const SNAP = 5
const MIN_LEN = 15
const DAY = 24 * 60

function hm2m(s: string): number {
  const [h, m] = s.split(':').map(Number)
  return (h || 0) * 60 + (m || 0)
}
function m2hm(m: number): string {
  const t = Math.max(0, Math.min(DAY, m))
  return `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`
}
function snap(m: number): number {
  return Math.round(m / SNAP) * SNAP
}
function pct(m: number): string {
  return `${(m / DAY) * 100}%`
}

const track = ref<HTMLElement | null>(null)
const hours = Array.from({ length: 13 }, (_, i) => i * 2) // 0,2,...,24

const boundaryMin = computed(() => (props.boundary ? hm2m(props.boundary) : 240))
const eventsMin = computed(() =>
  (props.events ?? []).map((e) => ({ name: e.name, a: hm2m(e.start), b: hm2m(e.end) })),
)
const winMin = computed(() =>
  props.windows.map((w) => ({ ...w, a: hm2m(w.start), b: hm2m(w.end) })),
)

// 拖拽态：draft=画新窗；move/resize 作用于 winMin[i]，视觉用 dragShift 偏移
type Drag =
  | { mode: 'draft'; a: number; b: number }
  | { mode: 'move' | 'resize-l' | 'resize-r'; index: number; origA: number; origB: number; grabMin: number }

const drag = ref<Drag | null>(null)

function posToMin(e: PointerEvent): number {
  const rect = track.value!.getBoundingClientRect()
  const x = Math.max(0, Math.min(rect.width, e.clientX - rect.left))
  return snap((x / rect.width) * DAY)
}

function onTrackDown(e: PointerEvent): void {
  if (props.readonly || drag.value) return
  const target = e.target as HTMLElement
  if (target.closest('.tl-block') || target.closest('.tl-handle')) return // 块交互另行处理
  const m = posToMin(e)
  drag.value = { mode: 'draft', a: m, b: m }
  track.value!.setPointerCapture(e.pointerId)
}

function onBlockDown(e: PointerEvent, index: number, mode: 'move' | 'resize-l' | 'resize-r'): void {
  if (props.readonly || drag.value) return
  e.stopPropagation()
  const w = winMin.value[index]
  drag.value = {
    mode,
    index,
    origA: w.a,
    origB: w.b,
    grabMin: posToMin(e),
  }
  track.value!.setPointerCapture(e.pointerId)
}

function onPointerMove(e: PointerEvent): void {
  const d = drag.value
  if (!d) return
  const m = posToMin(e)
  if (d.mode === 'draft') {
    drag.value = { mode: 'draft', a: d.a, b: m }
  } else if (d.mode === 'resize-l') {
    d.origA = Math.max(0, Math.min(d.origB - MIN_LEN, m))
  } else if (d.mode === 'resize-r') {
    d.origB = Math.min(DAY, Math.max(d.origA + MIN_LEN, m))
  } else {
    // 整块平移：保时长，钳制在 0-24h
    const delta = m - d.grabMin
    const len = d.origB - d.origA
    let na = d.origA + delta
    na = Math.max(0, Math.min(DAY - len, na))
    d.origA = na
    d.origB = na + len
  }
}

function onPointerUp(): void {
  const d = drag.value
  if (!d) return
  if (d.mode === 'draft') {
    const a = Math.min(d.a, d.b)
    const b = Math.max(d.a, d.b)
    if (b - a >= MIN_LEN) {
      emit('update:windows', [
        ...props.windows,
        { start: m2hm(a), end: m2hm(b), executor: 'mower', task: null },
      ])
    }
  } else {
    const next = props.windows.map((w, i) =>
      i === d.index
        ? { ...w, start: m2hm(d.origA), end: m2hm(d.origB) }
        : w,
    )
    emit('update:windows', next)
  }
  drag.value = null
}

// 块的最终显示位置（拖拽中的窗口用拖拽态坐标，未拖拽用 props）
function displayRange(i: number): { a: number; b: number } {
  const d = drag.value
  if (d && d.mode !== 'draft' && d.index === i) return { a: d.origA, b: d.origB }
  return { a: winMin.value[i].a, b: winMin.value[i].b }
}

function removeAt(i: number): void {
  emit(
    'update:windows',
    props.windows.filter((_, j) => j !== i),
  )
}

function executorColor(executor: string): string {
  return executor === 'maa' ? '#2080f0' : '#18a058'
}
</script>

<template>
  <div class="tl-wrap">
    <div
      ref="track"
      class="tl-track"
      :class="{ readonly }"
      @pointerdown="onTrackDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
    >
      <!-- 游戏日两段底色：0-界（上一游戏日尾）与 界-24 -->
      <div
        class="tl-seg tl-seg-prev"
        :style="{ width: pct(boundaryMin) }"
        :title="`上一游戏日尾（${boundary} 前）`"
      />
      <!-- 小时刻度 -->
      <div v-for="h in hours" :key="h" class="tl-tick" :style="{ left: pct(h * 60) }">
        <div class="tl-tick-line" />
        <div class="tl-tick-label">{{ String(h).padStart(2, '0') }}</div>
      </div>

      <!-- 游戏日界虚线 -->
      <div class="tl-boundary" :style="{ left: pct(boundaryMin) }">
        <div class="tl-boundary-line" />
        <div class="tl-boundary-label">游戏日界 {{ boundary }}</div>
      </div>

      <!-- 本地定时事项（只读红纹） -->
      <div
        v-for="(ev, i) in eventsMin"
        :key="`ev${i}`"
        class="tl-event"
        :style="{ left: pct(ev.a), width: pct(Math.max(ev.b - ev.a, 4)) }"
        :title="`${ev.name} ${m2hm(ev.a)}-${m2hm(ev.b)}`"
      >
        <span class="tl-event-label">{{ ev.name }}</span>
      </div>

      <!-- 时间窗块 -->
      <div
        v-for="(w, i) in winMin"
        :key="`w${i}`"
        class="tl-block"
        :style="(() => {
          const r = displayRange(i)
          return { left: pct(r.a), width: pct(Math.max(r.b - r.a, 8)), background: executorColor(w.executor) }
        })()"
        :title="`${w.start}-${w.end} ${w.executor}${w.task ? ':' + w.task : ''}（拖动平移，拖边调整）`"
        @pointerdown="onBlockDown($event, i, 'move')"
      >
        <span class="tl-block-label">{{ w.start }}-{{ w.end }}</span>
        <span
          v-if="!readonly"
          class="tl-block-del"
          title="删除此时间窗"
          @pointerdown.stop
          @click.stop="removeAt(i)"
        >×</span>
        <span class="tl-handle tl-handle-l" @pointerdown="onBlockDown($event, i, 'resize-l')" />
        <span class="tl-handle tl-handle-r" @pointerdown="onBlockDown($event, i, 'resize-r')" />
      </div>

      <!-- 拖拽中的新窗草稿 -->
      <div
        v-if="drag && drag.mode === 'draft' && Math.abs(drag.b - drag.a) >= 5"
        class="tl-block tl-draft"
        :style="{ left: pct(Math.min(drag.a, drag.b)), width: pct(Math.abs(drag.b - drag.a)) }"
      >
        <span class="tl-block-label">{{ m2hm(Math.min(drag.a, drag.b)) }}-{{ m2hm(Math.max(drag.a, drag.b)) }}</span>
      </div>
    </div>
    <div class="tl-hint">
      在空白处拖拽画新窗（{{ Math.floor(MIN_LEN / 60) ? `${MIN_LEN / 60}h` : `${MIN_LEN}m` }} 起，5min 吸附）；
      拖动块平移、拖左右边调整；点 × 删除。改动保存前不落盘。
    </div>
  </div>
</template>

<style scoped>
.tl-wrap {
  min-width: 0;
}
.tl-track {
  position: relative;
  height: 64px;
  min-width: 720px;
  border: 1px solid rgba(128, 128, 128, 0.35);
  border-radius: 6px;
  background: rgba(128, 128, 128, 0.06);
  touch-action: none;
  user-select: none;
  overflow: hidden;
}
.tl-track.readonly {
  pointer-events: none;
}
.tl-seg {
  position: absolute;
  top: 0;
  bottom: 0;
  background: rgba(24, 160, 88, 0.07);
}
.tl-seg-prev {
  left: 0;
  background: repeating-linear-gradient(
    -45deg,
    rgba(128, 128, 128, 0.08),
    rgba(128, 128, 128, 0.08) 6px,
    transparent 6px,
    transparent 12px
  );
}
.tl-tick {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0;
}
.tl-tick-line {
  position: absolute;
  top: 0;
  bottom: 16px;
  width: 1px;
  background: rgba(128, 128, 128, 0.25);
}
.tl-tick-label {
  position: absolute;
  bottom: 0;
  transform: translateX(-50%);
  font-size: 10px;
  color: gray;
  line-height: 14px;
  white-space: nowrap;
}
.tl-boundary {
  position: absolute;
  top: 0;
  bottom: 16px;
  width: 0;
  z-index: 3;
}
.tl-boundary-line {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0;
  border-left: 2px dashed #d03050;
}
.tl-boundary-label {
  position: absolute;
  top: 1px;
  left: 4px;
  font-size: 10px;
  color: #d03050;
  white-space: nowrap;
  opacity: 0.85;
}
.tl-event {
  position: absolute;
  top: 4px;
  bottom: 18px;
  z-index: 2;
  background: repeating-linear-gradient(
    -45deg,
    rgba(240, 160, 32, 0.45),
    rgba(240, 160, 32, 0.45) 4px,
    rgba(240, 160, 32, 0.18) 4px,
    rgba(240, 160, 32, 0.18) 8px
  );
  border: 1px solid rgba(240, 160, 32, 0.7);
  border-radius: 3px;
  overflow: hidden;
}
.tl-event-label {
  font-size: 10px;
  color: #7a4b00;
  padding: 0 2px;
  white-space: nowrap;
}
.tl-block {
  position: absolute;
  top: 22px;
  height: 26px;
  z-index: 4;
  border-radius: 4px;
  color: #fff;
  cursor: grab;
  display: flex;
  align-items: center;
  min-width: 8px;
}
.tl-block:active {
  cursor: grabbing;
}
.tl-block-label {
  font-size: 11px;
  padding: 0 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
}
.tl-block-del {
  position: absolute;
  right: 2px;
  top: -8px;
  width: 14px;
  height: 14px;
  line-height: 12px;
  text-align: center;
  font-size: 11px;
  border-radius: 50%;
  background: rgba(208, 48, 80, 0.9);
  color: #fff;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}
.tl-block:hover .tl-block-del {
  opacity: 1;
}
.tl-handle {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 7px;
  cursor: ew-resize;
  background: rgba(255, 255, 255, 0.25);
}
.tl-handle-l {
  left: 0;
  border-radius: 4px 0 0 4px;
}
.tl-handle-r {
  right: 0;
  border-radius: 0 4px 4px 0;
}
.tl-draft {
  background: rgba(128, 128, 128, 0.55);
  border: 1px dashed rgba(128, 128, 128, 0.8);
  pointer-events: none;
}
.tl-hint {
  font-size: 11px;
  color: gray;
  margin-top: 4px;
}
</style>
