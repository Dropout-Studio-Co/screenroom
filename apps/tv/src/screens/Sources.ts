import Blits from '@lightningjs/blits'

import { clampIndex } from '../core/focus.ts'
import { focusMotion } from '../core/motion.ts'
import { moveNav, RAIL_ENTRIES, railIndexFor, type NavZone } from '../core/nav.ts'
import { describeScan } from '../core/scan.ts'
import { railRows, RAIL_WIDTH, type RailRow } from '../design/rail.ts'
import Rail from '../components/Rail.ts'
import { APP_WIDTH } from '../design/cards.ts'
import { hex, TOKENS } from '../design/tokens.ts'
import {
  cancelScan,
  library,
  mediaRoots,
  nav,
  onTv,
  refreshMediaRoots,
  scanDrives,
  scanInProgress,
  scanProgress,
  storageSummary,
} from '../app/runtime.ts'

const ROUTE = '/sources'

const CONTENT_LEFT = RAIL_WIDTH + Math.round(TOKENS.spacing.shellPad * 0.6)

type Action = 'scan' | 'refresh'

interface Row {
  readonly key: string
  readonly label: string
  readonly value: string
  readonly y: number
  readonly x: number
  readonly fill: string
  readonly labelColor: string
  readonly valueColor: string
  readonly focused: boolean
  readonly visible: boolean
}

interface Entry {
  readonly key: string
  readonly label: string
  readonly value: string
  readonly action?: Action
}

function sinceMs(timestampMs: number, nowMs: number): string {
  if (timestampMs <= 0) {
    return 'never'
  }
  const minutes = Math.floor(Math.max(0, nowMs - timestampMs) / 60_000)
  if (minutes < 1) {
    return 'just now'
  }
  if (minutes < 60) {
    return `${minutes} min ago`
  }
  const hours = Math.floor(minutes / 60)
  return hours < 24 ? `${hours} h ago` : `${Math.floor(hours / 24)} d ago`
}

export default Blits.Component('Sources', {
  components: { Rail },
  template: `
    <Element w="1920" h="1080" :color="$background">
      <Rail :rows="$railRows" />

      <Text :x="$contentLeft" :y="$pad" content="Media Sources" :size="$headingSize" :color="$foreground" />
      <Text
        :x="$contentLeft"
        :y="$subY"
        :content="$subtitle"
        :size="$bodySize"
        :color="$muted"
        maxlines="2"
        wordwrap="true"
        :maxwidth="$noteWidth"
      />

      <Element
        :for="row in $rows"
        key="$row.key"
        :show="$row.visible"
        :x.transition="{value: $row.x, duration: $focus.duration, easing: $focus.ease}"
        :y="$row.y"
        :w="$listWidth"
        :h="$rowHeight"
        :color="$row.fill"
        :rounded="$radius"
      >
        <Text
          :x="$fieldX"
          :y="$rowLabelY"
          :content="$row.label"
          :size="$bodySize"
          :color="$row.labelColor"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$labelWidth"
        />
        <Text
          :x="$valueX"
          :y="$rowLabelY"
          :content="$row.value"
          :size="$captionSize"
          :color="$row.valueColor"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$valueWidth"
        />
      </Element>

      <Text :x="$contentLeft" :y="$footY" :content="$footer" :size="$captionSize" :color="$disabled" />
    </Element>
  `,
  state() {
    return {
      railRows: [] as RailRow[],
      background: hex(TOKENS.palette.background),
      foreground: hex(TOKENS.palette.foreground),
      muted: hex(TOKENS.palette.inkMuted),
      disabled: hex(TOKENS.palette.disabledForeground),
      pad: Math.round(TOKENS.spacing.shellPad),
      contentLeft: CONTENT_LEFT,
      noteWidth: APP_WIDTH - CONTENT_LEFT - Math.round(TOKENS.spacing.shellPad),
      radius: Math.round(TOKENS.radius.surface),
      headingSize: Math.round(TOKENS.type.title),
      bodySize: Math.round(TOKENS.type.body),
      captionSize: Math.round(TOKENS.type.caption),
      subY: Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 1.4),
      listWidth: 0,
      rowHeight: 0,
      fieldX: 0,
      valueX: 0,
      rowLabelY: 0,
      labelWidth: 0,
      valueWidth: 0,
      footY: 0,
      visibleRows: 8,
      scroll: 0,
      rows: [] as Row[],
      focusIndex: 0,
      zone: 'content' as NavZone,
      railIndex: 0,
      busy: false,

      pollTimer: null as ReturnType<typeof setTimeout> | null,
      subtitle: '',
      footer: '',
      focus: focusMotion(),
    }
  },
  computed: {
    layout() {
      const rowHeight = Math.round(TOKENS.type.body * 2.2)
      const gap = Math.round(TOKENS.type.caption * 0.6)
      const top = Math.round(
        TOKENS.spacing.shellPad + TOKENS.type.title * 1.4 + TOKENS.type.body * 3.4,
      )
      const footY = 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 2.4)
      return {
        top,
        rowHeight,
        rowGap: rowHeight + gap,
        fieldX: Math.round(TOKENS.spacing.fieldX),
        valueX: 760,
        rowLabelY: Math.round(rowHeight * 0.3),
        footY,
        visibleRows: Math.max(2, Math.floor((footY - top) / (rowHeight + gap))),
      }
    },
  },
  hooks: {
    ready() {
      this.$focus()
      this.applyLayout()
      this.reload()
      void this.refresh()
    },
    focus() {
      this.reload()
    },
    destroy() {
      this.stopPolling()
    },
  },

  input: {
    up() {
      this.press('up')
    },
    down() {
      this.press('down')
    },
    left() {
      this.press('left')
    },
    right() {
      this.press('right')
    },
    enter() {
      void this.enter()
    },
  },
  methods: {
    applyLayout(): void {
      const layout = this.layout
      this.rowHeight = layout.rowHeight
      this.fieldX = layout.fieldX
      this.valueX = layout.valueX
      this.rowLabelY = layout.rowLabelY
      this.labelWidth = layout.valueX - layout.fieldX - 20
      this.listWidth = APP_WIDTH - CONTENT_LEFT - Math.round(TOKENS.spacing.shellPad)
      this.valueWidth = this.listWidth - layout.valueX - layout.fieldX
      this.footY = layout.footY
      this.visibleRows = layout.visibleRows
    },

    rowX(focused: boolean): number {
      return this.contentLeft + (focused ? 8 : 0)
    },

    entries(): Entry[] {
      const store = library().state()
      const nowMs = Date.now()
      const report = library().lastScan()
      const progress = scanProgress()
      const roots = mediaRoots()
      const entries: Entry[] = [
        {
          key: 'scan',
          label: 'Scan for media',
          value: this.busy
            ? progress === null
              ? 'scanning — Enter stops it'
              : `scanning · ${progress.folders} folders · ${progress.files} files found — Enter stops it`
            : scanInProgress()
              ? 'a scan is already running'
              : 'press Enter to scan every readable drive',
          action: 'scan',
        },
        {
          key: 'refresh',
          label: 'Refresh drives',
          value: 'press Enter to ask the television again',
          action: 'refresh',
        },
        { key: 'reported', label: 'Drives the TV reports', value: storageSummary() },
      ]

      for (const root of roots.slice(0, 4)) {
        entries.push({
          key: `root-${root.label}`,
          label: `Drive ${root.label}`,
          value: `${root.type.toLowerCase()} · ${root.state.toLowerCase()} · ${
            root.scannable ? 'this app scans it' : 'not scanned by this app'
          }`,
        })
      }

      for (const source of store.sources.slice(0, 4)) {
        const count = store.items.filter((item) => item.sourceId === source.id).length
        entries.push({
          key: `source-${source.id}`,
          label: `Library ${source.label}`,
          value: `${source.state} · ${count} ${count === 1 ? 'file' : 'files'} · scanned ${sinceMs(
            source.lastScanMs,
            nowMs,
          )}`,
        })
      }

      const unavailable = store.items.filter((item) => !item.available).length
      entries.push({
        key: 'catalogued',
        label: 'In the library',
        value: `${store.items.length} ${store.items.length === 1 ? 'file' : 'files'}${
          unavailable > 0 ? ` · ${String(unavailable)} not readable now` : ''
        }`,
      })
      entries.push({
        key: 'lastscan',
        label: 'Last scan',
        value: describeScan(report),
      })
      entries.push({
        key: 'write',
        label: 'Last write to storage',
        value: library().writeFailure()?.reason ?? 'accepted',
      })
      return entries
    },

    reload(): void {
      const entries = this.entries()
      const layout = this.layout
      this.visibleRows = layout.visibleRows
      this.scroll = Math.min(
        Math.max(0, entries.length - this.visibleRows),
        Math.max(0, this.scroll),
      )
      this.focusIndex = Math.max(0, clampIndex(this.focusIndex, entries.length))

      if (this.zone === 'content') {
        if (this.focusIndex < this.scroll) {
          this.scroll = this.focusIndex
        } else if (this.focusIndex >= this.scroll + this.visibleRows) {
          this.scroll = this.focusIndex - this.visibleRows + 1
        }
      }
      this.rows = entries.map((entry, index) => {
        const focused = this.zone === 'content' && index === this.focusIndex
        return {
          key: entry.key,
          label: entry.label,
          value: entry.value,
          y: layout.top + (index - this.scroll) * layout.rowGap,
          x: this.rowX(focused),
          fill: focused ? hex(TOKENS.palette.focusSurface) : hex(TOKENS.palette.surface1),
          labelColor: focused
            ? hex(TOKENS.palette.focusForeground)
            : hex(TOKENS.palette.foreground),
          valueColor: focused
            ? hex(TOKENS.palette.focusForeground)
            : hex(TOKENS.palette.inkMuted),
          focused,
          visible: index >= this.scroll && index < this.scroll + this.visibleRows,
        }
      })
      this.railRows = railRows(railIndexFor(ROUTE), this.railIndex, this.zone === 'rail')
      this.subtitle = `${library().state().items.length} files catalogued from ${library().state().sources.length} ${
        library().state().sources.length === 1 ? 'drive' : 'drives'
      }.`
      this.footer = onTv()
        ? 'OK acts on the highlighted row · Left opens the menu · Back returns'
        : 'Browser preview: no drive on this host, so nothing here proves a television'
      nav().set(
        ROUTE,
        { zone: this.zone, rail: this.railIndex, content: this.focusIndex },
        { railCount: RAIL_ENTRIES.length, contentCount: entries.length, columns: 1 },
      )
    },

    press(direction: 'left' | 'right' | 'up' | 'down'): void {
      const entries = this.entries()
      const current = { zone: this.zone, rail: this.railIndex, content: this.focusIndex }

      const next = moveNav(current, direction, {
        railCount: RAIL_ENTRIES.length,
        contentCount: entries.length,
        columns: 1,
      })
      if (
        next.zone === this.zone &&
        next.rail === this.railIndex &&
        next.content === this.focusIndex
      ) {
        return
      }
      this.zone = next.zone
      this.railIndex = next.rail
      this.focusIndex = Math.max(0, next.content)
      if (this.zone === 'content') {
        if (this.focusIndex < this.scroll) {
          this.scroll = this.focusIndex
        } else if (this.focusIndex >= this.scroll + this.visibleRows) {
          this.scroll = this.focusIndex - this.visibleRows + 1
        }
      }
      this.reload()
    },

    async enter(): Promise<void> {
      if (this.zone === 'rail') {
        const entry = RAIL_ENTRIES[this.railIndex]
        if (entry === undefined) {
          return
        }
        if (entry.route !== ROUTE) {
          this.$router.to(entry.route)
          return
        }

        this.zone = 'content'
        this.focusIndex = Math.max(0, this.focusIndex)
        this.reload()
        return
      }
      const row = this.rows[this.focusIndex]
      if (row === undefined) {
        return
      }
      if (row.key === 'scan') {
        if (this.busy) {
          cancelScan()
          this.footer = 'Stopping after the folder it is in…'
          return
        }
        await this.runScan()
        return
      }
      if (row.key === 'refresh') {
        await this.refresh()
        return
      }
    },

    async refresh(): Promise<void> {
      await refreshMediaRoots()
      this.reload()
    },

    async runScan(): Promise<void> {
      this.busy = true
      this.reload()
      this.startPolling()
      const report = await scanDrives()
      this.busy = false
      this.stopPolling()
      this.reload()
      this.footer = describeScan(report)
    },

    startPolling(): void {
      this.stopPolling()
      const tick = (): void => {
        if (!this.busy) {
          return
        }
        this.reload()
        this.pollTimer = this.$setTimeout(tick, 500)
      }
      this.pollTimer = this.$setTimeout(tick, 500)
    },
    stopPolling(): void {
      if (this.pollTimer !== null) {
        this.$clearTimeout(this.pollTimer)
        this.pollTimer = null
      }
    },
  },
})
