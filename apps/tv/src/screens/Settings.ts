import Blits from '@lightningjs/blits'

import { clampIndex } from '../core/focus.ts'
import { focusMotion } from '../core/motion.ts'
import { moveNav, RAIL_ENTRIES, railIndexFor, type NavZone } from '../core/nav.ts'
import { describeScan, type ScanReport } from '../core/scan.ts'
import {
  describeAudio,
  describeSubtitles,
  forgetChoices,
  type DeviceSettings,
} from '../core/settings.ts'
import { APP_WIDTH } from '../design/cards.ts'
import { railRows, RAIL_WIDTH, type RailRow } from '../design/rail.ts'
import Rail from '../components/Rail.ts'
import { hex, TOKENS } from '../design/tokens.ts'
import {
  cancelScan,
  device,
  host,
  keys,
  library,
  mediaRoots,
  onTv,
  nav,
  refreshMediaRoots,
  saveSettings,
  scanDrives,
  scanInProgress,
  scanReport,
  settings,
  storageSummary,
} from '../app/runtime.ts'
import { hostSummary } from '../platform/env.ts'

type Action = 'scan' | 'subtitles' | 'audio' | 'forget'

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

  readonly actionable: boolean

  readonly action: Action | null

  readonly visible: boolean
}

interface Entry {
  readonly key: string
  readonly label: string
  readonly value: string
  readonly action?: Action
}

const MAX_LISTED = 4

const ROUTE = '/settings'

const CONTENT_LEFT = RAIL_WIDTH + Math.round(TOKENS.spacing.shellPad * 0.6)

function sinceMs(timestampMs: number, nowMs: number): string {
  if (timestampMs <= 0) {
    return 'never'
  }
  const elapsed = Math.max(0, nowMs - timestampMs)
  const minutes = Math.floor(elapsed / 60_000)
  if (minutes < 1) {
    return 'just now'
  }
  if (minutes < 60) {
    return `${minutes} min ago`
  }
  const hours = Math.floor(minutes / 60)
  return hours < 24 ? `${hours} h ago` : `${Math.floor(hours / 24)} d ago`
}

export default Blits.Component('Settings', {
  components: { Rail },
  template: `
    <Element w="1920" h="1080" :color="$background">
      <Rail :rows="$railRows" />

      <Text :x="$contentLeft" :y="$pad" content="Settings" :size="$titleSize" :color="$foreground" />
      <Text
        :x="$contentLeft"
        :y="$subY"
        :content="$subtitle"
        :size="$bodySize"
        :color="$muted"
        maxlines="1"
        textoverflow="ellipsis"
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
        />
        <Text
          :x="$valueX"
          :y="$rowLabelY"
          :content="$row.value"
          :size="$captionSize"
          :color="$row.valueColor"
          maxlines="1"
          textoverflow="ellipsis"
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
      titleSize: Math.round(TOKENS.type.title),
      bodySize: Math.round(TOKENS.type.body),
      captionSize: Math.round(TOKENS.type.caption),
      subY: Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 1.4),
      subtitle: '',
      footer: '',
      listWidth: 0,
      rowHeight: 0,
      fieldX: 0,
      valueX: 0,
      rowLabelY: 0,
      footY: 0,
      rows: [] as Row[],
      focusIndex: 0,
      zone: 'content' as NavZone,
      railIndex: 0,
      busy: false,

      scroll: 0,

      visibleRows: 12,
      focus: focusMotion(),
    }
  },
  computed: {
    layout() {
      const rowHeight = Math.round(TOKENS.type.body * 2.1)
      const gap = Math.round(TOKENS.type.caption * 0.6)
      const top = Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 1.4 + TOKENS.type.body * 3)
      const footY = 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 2)
      return {
        top,
        rowHeight,
        rowGap: rowHeight + gap,
        fieldX: Math.round(TOKENS.spacing.fieldX),
        valueX: 620,
        rowLabelY: Math.round(rowHeight * 0.3),
        footY,
        visibleRows: Math.max(1, Math.floor((footY - top) / (rowHeight + gap))),
      }
    },
  },
  hooks: {
    ready() {
      this.$focus()
      this.applyLayout()
      this.reload()

      void this.refreshDrives()
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
    applyLayout() {
      const layout = this.layout
      this.rowHeight = layout.rowHeight
      this.fieldX = layout.fieldX
      this.valueX = layout.valueX
      this.rowLabelY = layout.rowLabelY
      this.footY = layout.footY
      this.visibleRows = layout.visibleRows
      this.listWidth = APP_WIDTH - CONTENT_LEFT - Math.round(TOKENS.spacing.shellPad)
    },

    reload() {
      const capabilities = host()
      const info = device()
      const registration = keys()
      const report = library().loadReport()
      const store = library().state()
      const itemCount = store.items.length
      const unavailable = store.items.filter((item) => !item.available).length
      const nowMs = Date.now()
      const lastScan: ScanReport = scanReport()
      const prefs: DeviceSettings = settings()

      this.subtitle = hostSummary(capabilities)
      const entries: Entry[] = [
        { key: 'device', label: 'TV model', value: info.model ?? 'not reported by this host' },
        {
          key: 'firmware',
          label: 'Software version',
          value: info.firmwareVersion ?? 'not reported by this host',
        },
        {
          key: 'platform',
          label: 'Tizen version',
          value: info.platformVersion ?? 'not reported by this host',
        },
        {

          key: 'product',
          label: 'Product version',
          value: info.productVersion ?? 'not reported by this host',
        },
        {
          key: 'playback',
          label: 'Playback path',
          value: capabilities.avplay ? 'AVPlay (the TV decoder)' : '<video> element (browser preview)',
        },
        {
          key: 'remote',
          label: 'Remote keys accepted',
          value: registration.supported
            ? `${registration.registered.length} registered, ${registration.refused.length} refused`
            : 'this host has no remote-key API',
        },
        { key: 'storage', label: 'Where your data is kept', value: report.location },
        {
          key: 'durable',
          label: 'Storage writes',
          value: capabilities.durableStorage ? 'accepted' : 'REFUSED — nothing will be saved',
        },
        {
          key: 'lost',
          label: 'State from a previous run',
          value: report.lostStoredState
            ? `not usable: ${report.issues[0]?.reason ?? 'unknown reason'}`
            : 'read without problems',
        },
        { key: 'writefail', label: 'Last write', value: library().writeFailure()?.reason ?? 'accepted' },
        { key: 'drives', label: 'Drives the TV reports', value: storageSummary() },
      ]

      const roots = mediaRoots()
      for (const root of roots.slice(0, MAX_LISTED)) {
        entries.push({
          key: `root-${root.label}`,
          label: `Drive ${root.label}`,
          value: `${root.type.toLowerCase()} · ${root.state.toLowerCase()} · ${
            root.scannable ? 'this app scans it' : 'not scanned by this app'
          }`,
        })
      }
      if (roots.length > MAX_LISTED) {
        entries.push({
          key: 'root-more',
          label: 'More drives',
          value: `${String(roots.length - MAX_LISTED)} not listed`,
        })
      }

      for (const source of store.sources.slice(0, MAX_LISTED)) {
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
      if (store.sources.length > MAX_LISTED) {
        entries.push({
          key: 'source-more',
          label: 'More library roots',
          value: `${String(store.sources.length - MAX_LISTED)} not listed`,
        })
      }

      entries.push({
        key: 'sources',
        label: 'Catalogued',
        value: `${itemCount} ${itemCount === 1 ? 'file' : 'files'}${
          unavailable > 0 ? `, ${String(unavailable)} not readable now` : ''
        }`,
      })
      entries.push({
        key: 'lastscan',
        label: 'Last scan',
        value: `${describeScan(lastScan)}${lastScan.finishedMs > 0 ? ` (${sinceMs(lastScan.finishedMs, nowMs)})` : ''}`,
      })
      if (lastScan.issues.length > 0) {
        const first = lastScan.issues[0]
        entries.push({
          key: 'scanissues',
          label: 'Entries the scan refused',
          value: first === undefined ? '' : `${String(lastScan.issues.length)} · first: ${first.code} (${first.relativePath === '' ? 'whole drive' : first.relativePath})`,
        })
      }

      entries.push({
        key: 'subtitles',
        label: 'Subtitles',
        value: describeSubtitles(prefs),
        action: 'subtitles',
      })
      entries.push({
        key: 'audio',
        label: 'Audio language',
        value: describeAudio(prefs),
        action: 'audio',
      })
      entries.push({
        key: 'forget',
        label: 'Forget learned languages',
        value: 'press Enter',
        action: 'forget',
      })
      entries.push({
        key: 'scan',
        label: 'Scan for media',
        value: this.busy
          ? 'scanning — press Enter to stop'
          : scanInProgress()
            ? 'a scan is already running'
            : 'press Enter',
        action: 'scan',
      })
      entries.push({ key: 'provenance', label: 'Design tokens', value: TOKENS.provenance })

      const layout = this.layout
      this.visibleRows = layout.visibleRows

      this.scroll = Math.min(Math.max(0, entries.length - this.visibleRows), Math.max(0, this.scroll))
      this.rows = entries.map((entry, index) => {
        const focused = this.zone === 'content' && index === this.focusIndex
        return {
          key: entry.key,
          label: entry.label,
          value: entry.value,

          y: layout.top + (index - this.scroll) * layout.rowGap,
          x: this.rowX(focused),
          fill: focused ? hex(TOKENS.palette.focusSurface) : hex(TOKENS.palette.surface1),
          labelColor: focused ? hex(TOKENS.palette.focusForeground) : hex(TOKENS.palette.foreground),
          valueColor: focused ? hex(TOKENS.palette.focusForeground) : hex(TOKENS.palette.inkMuted),
          focused,
          actionable: entry.action !== undefined,
          action: entry.action ?? null,

          visible: index >= this.scroll && index < this.scroll + this.visibleRows,
        }
      })
      this.focusIndex = Math.max(0, clampIndex(this.focusIndex, this.rows.length))
      this.railRows = railRows(railIndexFor(ROUTE), this.railIndex, this.zone === 'rail')
      this.footer = onTv()
        ? 'Enter acts on the highlighted row · Left opens the menu · Back returns'
        : 'Browser preview: none of these readings prove TV behaviour'
      nav().set(
        ROUTE,
        { zone: this.zone, rail: this.railIndex, content: this.focusIndex },
        { railCount: RAIL_ENTRIES.length, contentCount: this.rows.length, columns: 1 },
      )
    },

    rowX(focused: boolean): number {
      return this.contentLeft + (focused ? 8 : 0)
    },

    markFocus() {
      const layout = this.layout
      this.rows = this.rows.map((row, index) => {
        const focused = this.zone === 'content' && index === this.focusIndex
        return {
          ...row,
          focused,
          y: layout.top + (index - this.scroll) * layout.rowGap,
          x: this.rowX(focused),
          visible: index >= this.scroll && index < this.scroll + this.visibleRows,
          fill: focused ? hex(TOKENS.palette.focusSurface) : hex(TOKENS.palette.surface1),
          labelColor: focused ? hex(TOKENS.palette.focusForeground) : hex(TOKENS.palette.foreground),
          valueColor: focused ? hex(TOKENS.palette.focusForeground) : hex(TOKENS.palette.inkMuted),
        }
      })
    },

    press(direction: 'up' | 'down' | 'left' | 'right') {
      const next = moveNav(
        { zone: this.zone, rail: this.railIndex, content: this.focusIndex },
        direction,
        { railCount: RAIL_ENTRIES.length, contentCount: this.rows.length, columns: 1 },
      )
      if (
        next.zone === this.zone &&
        next.rail === this.railIndex &&
        Math.max(0, next.content) === this.focusIndex
      ) {
        return
      }
      this.zone = next.zone
      this.railIndex = next.rail
      this.focusIndex = Math.max(0, next.content)
      if (this.zone === 'content') {
        const first = this.scroll
        const last = this.scroll + this.visibleRows - 1
        if (this.focusIndex < first) {
          this.scroll = this.focusIndex
        } else if (this.focusIndex > last) {
          this.scroll = this.focusIndex - this.visibleRows + 1
        }
      }
      this.markFocus()
    },

    async enter() {
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
        this.markFocus()
        return
      }
      const row = this.rows[this.focusIndex]
      if (row === undefined || row.action === null) {
        return
      }
      switch (row.action) {
        case 'scan':
          if (this.busy) {

            cancelScan()
            this.footer = 'Stopping after the folder it is in…'
            return
          }
          await this.runScan()
          return
        case 'subtitles': {
          const flipped = settings().subtitles === 'off' ? 'auto' : 'off'
          const next: DeviceSettings = { ...settings(), subtitles: flipped }
          saveSettings(next)
          this.reload()
          this.footer = `Subtitles: ${describeSubtitles(settings())}`
          return
        }
        case 'audio': {

          saveSettings({ ...settings(), audioLanguage: '' })
          this.reload()
          this.footer = 'Audio is back to whatever each file defaults to.'
          return
        }
        case 'forget': {
          saveSettings(forgetChoices())
          this.reload()
          this.footer = 'Both preferences are back to this television’s defaults.'
          return
        }
      }
    },

    async runScan() {
      this.busy = true
      this.reload()

      const report = await scanDrives()
      this.busy = false
      this.reload()
      this.footer = describeScan(report)
    },

    async refreshDrives() {
      await refreshMediaRoots()
      this.reload()
    },
  },
})
