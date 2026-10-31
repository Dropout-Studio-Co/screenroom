import Blits from '@lightningjs/blits'

import { formatPosition, progressFraction } from '../core/browse.ts'
import { resolveLocator, subtitleStem } from '../core/locator.ts'
import { rememberChoice, type DeviceSettings } from '../core/settings.ts'
import {
  buildPlayerMenu,
  MENU_SEEK_MS,
  type PlayerMenuRow,
} from '../core/player-menu.ts'
import { chooseTracks, nextEpisode, sidecarFileNames, type TrackChoice } from '../core/tracks.ts'
import type { LibraryItem } from '../core/contract.ts'
import { hex, TOKENS } from '../design/tokens.ts'
import { routeBoolean, routeString } from '../app/route-props.ts'
import { items, library, saveSettings, settings } from '../app/runtime.ts'
import * as player from '../app/player.ts'
import { pathExists, fileUri } from '../platform/media-roots.ts'

const SEEK_STEP_MS = 10_000

const JUMP_MS = 60_000

const OVERLAY_MS = 4_000

const SCRIM_ALPHA = 0.55

const SCRIM_ALPHA_STUCK = 0.8

interface MenuRow extends PlayerMenuRow {
  readonly y: number
  readonly fill: string
  readonly labelColor: string
  readonly detailColor: string
  readonly marker: string
  readonly detailY: number
}

function findSidecar(absolute: string, relativePath: string): string | null {
  const stem = subtitleStem(relativePath)
  if (stem === null) {
    return null
  }
  const slash = absolute.lastIndexOf('/')
  const directory = slash < 0 ? '' : absolute.slice(0, slash + 1)
  for (const name of sidecarFileNames(stem)) {
    const candidate = `${directory}${name}`
    if (pathExists(candidate) === true) {
      return candidate
    }
  }
  return null
}

function fileName(location: string): string {
  const slash = location.lastIndexOf('/')
  return slash < 0 ? location : location.slice(slash + 1)
}

export default Blits.Component('Player', {
  props: {
    id: { required: true },

    fromStart: { default: false },
  },
  template: `
    <Element w="1920" h="1080">
      <!--
        The scrim is its own layer, and the screen itself is never transparent. Tying the
        whole template's alpha to the overlay meant that a file which failed to open drew
        its own explanation into an invisible node: the owner got a black screen and no
        reason, on the one path where the reason is the only thing worth showing.
      -->
      <Element w="1920" h="1080" :color="$background" :alpha="$scrimAlpha" />
      <Text
        :x="$pad"
        :y="$titleY"
        :content="$title"
        :size="$titleSize"
        :color="$foreground"
        maxlines="1"
        textoverflow="ellipsis"
        maxwidth="1660"
        :alpha="$overlayAlpha"
      />
      <Text
        :x="$pad"
        :y="$subY"
        :content="$status"
        :size="$bodySize"
        :color="$muted"
        maxlines="2"
        wordwrap="true"
        maxwidth="1660"
        :alpha="$overlayAlpha"
      />

      <Element
        :x="$pad"
        :y="$barY"
        :w="$barWidth"
        :h="$barHeight"
        :color="$trackColor"
        :alpha="$overlayAlpha"
      />
      <Element
        :x="$pad"
        :y="$barY"
        :w="$barFill"
        :h="$barHeight"
        :color="$signal"
        :alpha="$overlayAlpha"
      />

      <Text
        :x="$pad"
        :y="$footY"
        :content="$footer"
        :size="$captionSize"
        :color="$disabled"
        :alpha="$overlayAlpha"
      />

      <Text
        :x="$subtitleX"
        :y="$subtitleY"
        :content="$subtitleLine"
        :size="$subtitleSize"
        :color="$foreground"
        align="center"
        maxlines="2"
        wordwrap="true"
        :maxwidth="$subtitleWidth"
      />

      <Element
        :show="$menuOpen"
        :x="$menuX"
        :y="$menuY"
        :w="$menuWidth"
        :h="$menuHeight"
        :color="$surface1"
        :rounded="$radius"
      >
        <Text
          :x="$menuPad"
          y="0"
          :content="$menuTitle"
          :size="$menuTitleSize"
          :color="$foreground"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$menuTextWidth"
        />
        <Element
          :for="row in $menuRows"
          key="$row.key"
          x="0"
          :y="$row.y"
          :w="$menuWidth"
          :h="$menuRowHeight"
          :color="$row.fill"
          :rounded="$radius"
        >
          <Text
            :x="$menuPad"
            :y="$menuLabelOffset"
            :content="$row.label"
            :size="$bodySize"
            :color="$row.labelColor"
            maxlines="1"
            textoverflow="ellipsis"
            :maxwidth="$menuTextWidth"
          />
          <Text
            :x="$menuPad"
            :y="$row.detailY"
            :content="$row.detail"
            :size="$captionSize"
            :color="$row.detailColor"
            maxlines="1"
            textoverflow="ellipsis"
            :maxwidth="$menuTextWidth"
          />
          <Text
            :x="$menuMarkX"
            :y="$menuLabelOffset"
            :content="$row.marker"
            :size="$bodySize"
            :color="$row.labelColor"
          />
        </Element>
      </Element>
    </Element>
  `,
  state() {
    return {
      background: hex(TOKENS.palette.background),
      foreground: hex(TOKENS.palette.foreground),
      muted: hex(TOKENS.palette.inkMuted),
      disabled: hex(TOKENS.palette.disabledForeground),
      signal: hex(TOKENS.palette.signal),
      trackColor: hex(TOKENS.palette.surface3),
      surface1: hex(TOKENS.palette.surface1),
      focusSurface: hex(TOKENS.palette.focusSurface),
      focusForeground: hex(TOKENS.palette.focusForeground),
      pad: Math.round(TOKENS.spacing.shellPad),
      radius: Math.round(TOKENS.radius.surface),
      titleSize: Math.round(TOKENS.type.title),
      bodySize: Math.round(TOKENS.type.body),
      captionSize: Math.round(TOKENS.type.caption),
      subtitleSize: Math.round(TOKENS.type.bodyLg),
      titleY: 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 12),
      subY: 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 9.4),
      barY: 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 6.4),
      barHeight: 8,
      barWidth: 1660,
      barFill: 0,
      footY: 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 3),
      title: '',
      status: '',
      footer: '▲▼ controls · OK selects · ◄ ► ten seconds · Back stops',

      durationMs: 0,
      overlayAlpha: 0,
      scrimAlpha: 0,
      overlayTimer: null as ReturnType<typeof setTimeout> | null,

      subtitleLine: '',
      subtitleX: 160,
      subtitleY: 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 16),
      subtitleWidth: 1600,

      menuOpen: false,
      menuRows: [] as MenuRow[],
      menuIndex: 0,
      menuTitle: '',
      menuX: 0,
      menuY: 0,
      menuWidth: 0,
      menuHeight: 0,
      menuPad: 0,
      menuTitleSize: Math.round(TOKENS.type.bodyLg),
      menuTextWidth: 0,
      menuMarkX: 0,
      menuRowHeight: 0,
      menuLabelOffset: 0,

      audioIndex: null as number | null,
      textIndex: null as number | null,
      silent: false,

      currentId: '',

      nextId: '',
      nextLabel: '',
      finished: false,

      failed: false,

      sidecarNote: '',
    }
  },
  computed: {
    layout() {
      const rowHeight = Math.round(TOKENS.type.body * 3)
      const gap = Math.round(TOKENS.type.caption * 0.5)
      const menuWidth = 820
      return {
        menuWidth,
        menuX: 1920 - menuWidth - Math.round(TOKENS.spacing.shellPad),
        menuY: Math.round(TOKENS.spacing.shellPad * 1.5),
        menuPad: Math.round(TOKENS.spacing.fieldX),
        menuTitleSize: Math.round(TOKENS.type.bodyLg),
        menuTextWidth: menuWidth - Math.round(TOKENS.spacing.fieldX) * 2 - 60,
        menuMarkX: menuWidth - Math.round(TOKENS.spacing.fieldX) - 40,
        menuRowHeight: rowHeight,
        rowGap: rowHeight + gap,
        menuLabelOffset: Math.round(rowHeight * 0.16),
        detailY: Math.round(rowHeight * 0.16 + TOKENS.type.body * 1.25),
        titleBlock: Math.round(TOKENS.type.bodyLg * 2),
      }
    },
  },
  hooks: {
    ready() {
      this.$focus()
      this.open()
    },
    destroy() {

      player.end()
    },
  },

  input: {
    enter() {
      this.enter()
    },
    space() {
      this.playPause()
    },
    play() {
      this.playPause()
    },
    pause() {
      this.playPause()
    },
    playPause() {
      this.playPause()
    },
    left() {
      this.left()
    },
    right() {
      this.right()
    },
    up() {
      this.up()
    },
    down() {
      this.down()
    },
    rewind() {
      this.rewind()
    },
    fastForward() {
      this.fastForward()
    },
    info() {
      this.info()
    },
    back() {
      if (this.menuOpen) {
        this.closeMenu()
        return
      }
      this.$router.back()
    },
  },
  methods: {

    currentItem(): LibraryItem | null {
      const id = this.currentId === '' ? routeString(this.id) : this.currentId
      return items().find((candidate) => candidate.id === id) ?? null
    },

    events() {
      return {
        onReady: () => {
          this.applyPreferences()
          this.status = this.sidecarNote === '' ? 'Playing.' : this.sidecarNote
          this.repaint()
        },
        onPosition: () => {
          this.repaint()
          player.checkpointIfDue()
        },
        onDuration: (milliseconds: number) => {
          this.durationMs = milliseconds
          this.repaint()
        },
        onBuffering: (active: boolean) => {
          this.status = active ? 'Buffering…' : this.playingLabel()
        },
        onEnded: () => {
          this.onEnded()
        },
        onError: (message: string) => {
          this.reportFailure(`This file could not be played. ${message}`)
        },
        onSubtitleFile: (result: { offered: string | null; accepted: boolean }) => {
          if (result.offered === null) {
            this.sidecarNote = ''
            return
          }
          this.sidecarNote = result.accepted
            ? `Playing with subtitles from ${fileName(result.offered)}.`
            : 'The decoder would not take the subtitle file beside this video.'
        },
        onSubtitleText: (text: string) => {
          this.subtitleLine = text
        },
      }
    },

    open() {
      const wantedId = routeString(this.id)
      const item = items().find((candidate) => candidate.id === wantedId)
      if (item === undefined) {
        this.title = 'That file is no longer in the library.'
        this.reportFailure('Nothing was played. Go back and choose again.')
        return
      }
      const startMs = routeBoolean(this.fromStart, false) ? 0 : item.positionMs
      this.start(item, startMs)
    },

    start(item: LibraryItem, startMs: number) {
      this.title = item.title
      this.currentId = item.id
      this.subtitleLine = ''
      this.menuOpen = false
      this.finished = false
      this.failed = false
      this.nextId = ''
      this.nextLabel = ''
      this.sidecarNote = ''
      this.audioIndex = null
      this.textIndex = null
      this.silent = false
      this.durationMs = item.durationMs

      const source = library().source(item.sourceId)
      if (source === null) {
        this.reportFailure(
          'The drive this file came from is no longer granted. Scan again from Media Sources.',
        )
        return
      }
      if (source.state !== 'available' || !item.available) {
        this.reportFailure(
          'That drive is not readable right now. Plug it back in and scan again from Media Sources.',
        )
        return
      }

      const absolute = resolveLocator(source.rootPath, item.relativePath)
      if (absolute === null) {
        this.reportFailure("This file's stored path is not safe to open, so it was not opened.")
        return
      }
      const resolved = fileUri(absolute)
      if (!resolved.ok) {
        this.reportFailure(resolved.reason)
        return
      }

      const sidecar = findSidecar(absolute, item.relativePath)
      this.status = startMs > 0 ? `Resuming from ${formatPosition(startMs)}…` : 'Starting…'
      player.begin(item, resolved.uri, startMs, this.events(), { subtitleFile: sidecar })
      this.repaint()
    },

    applyPreferences() {
      const prefs: DeviceSettings = settings()
      const choice = chooseTracks(player.tracks(), {
        subtitles: prefs.subtitles,
        subtitleLanguage: prefs.subtitleLanguage,
        audioLanguage: prefs.audioLanguage,
      })
      if (choice.selectAudio && choice.audioIndex !== null) {
        if (player.selectTrack('AUDIO', choice.audioIndex)) {
          this.audioIndex = choice.audioIndex
        }
      }
      if (choice.selectText && choice.textIndex !== null) {
        if (player.selectTrack('TEXT', choice.textIndex)) {
          this.textIndex = choice.textIndex
          this.silent = false
          return
        }
      }

      if (player.setSubtitlesSilent(true)) {
        this.silent = true
      }
    },

    repaint() {
      const current = player.view()
      this.durationMs = current.durationMs > 0 ? current.durationMs : this.durationMs
      this.barFill = Math.round(
        this.barWidth * progressFraction(current.positionMs, this.durationMs),
      )
      this.status =
        this.durationMs > 0
          ? `${formatPosition(current.positionMs)} of ${formatPosition(this.durationMs)}` +
            (current.paused ? ' · paused' : '')
          : this.status
    },
    playingLabel(): string {
      return player.view().paused ? 'Paused.' : 'Playing.'
    },

    showOverlay() {
      this.overlayAlpha = 1
      this.scrimAlpha = SCRIM_ALPHA
      if (this.overlayTimer !== null) {
        this.$clearTimeout(this.overlayTimer)
      }
      this.overlayTimer = this.$setTimeout(() => {
        this.overlayAlpha = 0
        this.scrimAlpha = 0
        this.overlayTimer = null
      }, OVERLAY_MS)
    },

    reportFailure(message: string) {
      if (this.overlayTimer !== null) {
        this.$clearTimeout(this.overlayTimer)
        this.overlayTimer = null
      }
      this.status = message
      this.overlayAlpha = 1
      this.scrimAlpha = SCRIM_ALPHA_STUCK

      this.failed = true
    },
    playPause() {
      if (this.menuOpen) {
        return
      }
      this.showOverlay()
      const current = player.view()
      if (!current.running) {
        return
      }
      if (current.paused) {
        player.resume()
      } else {
        player.pause()
      }
      this.status = this.playingLabel()
      this.repaint()
    },
    rewind() {
      if (this.menuOpen) {
        return
      }
      this.showOverlay()
      player.jump(-JUMP_MS)
    },
    fastForward() {
      if (this.menuOpen) {
        return
      }
      this.showOverlay()
      player.jump(JUMP_MS)
    },
    left() {
      if (this.menuOpen) {
        return
      }
      this.showOverlay()
      player.seekTo(player.view().positionMs - SEEK_STEP_MS)
    },
    right() {
      if (this.menuOpen) {
        return
      }
      this.showOverlay()
      player.seekTo(player.view().positionMs + SEEK_STEP_MS)
    },
    info() {
      if (this.menuOpen) {
        this.closeMenu()
        return
      }
      if (this.overlayAlpha > 0) {
        this.overlayAlpha = 0
        return
      }
      this.showOverlay()
    },

    up() {
      if (this.menuOpen) {
        this.moveMenu(-1)
        return
      }
      this.openMenu()
    },

    down() {
      if (this.menuOpen) {
        this.moveMenu(1)
        return
      }
      this.openMenu()
    },

    enter() {
      if (this.menuOpen) {
        this.chooseMenuRow()
        return
      }
      if (this.finished && this.nextId !== '') {
        this.playNext()
        return
      }
      this.playPause()
    },

    openMenu() {
      const tracks: readonly TrackChoice[] = player.tracks()

      const rows = buildPlayerMenu({
        tracks,
        state: {
          audioIndex: this.audioIndex,
          textIndex: this.textIndex,
          silent: this.silent || player.subtitlesSilent(),
        },
        paused: player.view().paused,
        running: !this.failed && player.view().running,
        finished: this.finished,
        nextLabel: this.nextLabel,
      })
      const layout = this.layout

      this.menuIndex = Math.min(this.menuIndex, rows.length - 1)
      this.menuTitle = this.title
      this.menuX = layout.menuX
      this.menuY = layout.menuY
      this.menuWidth = layout.menuWidth
      this.menuPad = layout.menuPad
      this.menuTitleSize = layout.menuTitleSize
      this.menuTextWidth = layout.menuTextWidth
      this.menuMarkX = layout.menuMarkX
      this.menuRowHeight = layout.menuRowHeight
      this.menuLabelOffset = layout.menuLabelOffset
      this.menuRows = rows.map((row, index) => {
        const focused = index === this.menuIndex
        return {
          ...row,
          y: layout.titleBlock + index * layout.rowGap,
          fill: focused ? hex(TOKENS.palette.focusSurface) : hex(TOKENS.palette.surface1),
          labelColor: focused ? hex(TOKENS.palette.focusForeground) : hex(TOKENS.palette.foreground),
          detailColor: focused
            ? hex(TOKENS.palette.focusForeground)
            : hex(TOKENS.palette.inkMuted),
          marker: row.selected ? '●' : '',
          detailY: layout.detailY,
        }
      })
      this.menuHeight =
        layout.titleBlock + rows.length * layout.rowGap + Math.round(TOKENS.spacing.shellPad * 0.5)

      this.showOverlay()
      this.menuOpen = true
    },

    moveMenu(delta: number) {
      if (this.menuRows.length === 0) {
        return
      }
      const next = Math.min(this.menuRows.length - 1, Math.max(0, this.menuIndex + delta))
      if (next === this.menuIndex) {
        return
      }
      this.menuIndex = next
      this.menuRows = this.menuRows.map((row, index) => {
        const focused = index === this.menuIndex
        return {
          ...row,
          fill: focused ? hex(TOKENS.palette.focusSurface) : hex(TOKENS.palette.surface1),
          labelColor: focused ? hex(TOKENS.palette.focusForeground) : hex(TOKENS.palette.foreground),
          detailColor: focused
            ? hex(TOKENS.palette.focusForeground)
            : hex(TOKENS.palette.inkMuted),
        }
      })
    },
    closeMenu() {
      this.menuOpen = false
      this.showOverlay()
    },

    chooseMenuRow() {
      const row = this.menuRows[this.menuIndex]
      if (row === undefined) {
        this.closeMenu()
        return
      }
      switch (row.kind) {
        case 'playpause': {
          this.closeMenu()
          this.playPause()
          return
        }
        case 'seekBack':
        case 'seekForward': {
          const delta = row.kind === 'seekBack' ? -MENU_SEEK_MS : MENU_SEEK_MS
          this.closeMenu()
          player.seekTo(player.view().positionMs + delta)
          this.repaint()
          return
        }
        case 'next':
          this.closeMenu()
          this.playNext()
          return
        case 'exit':
          this.closeMenu()
          this.$router.back()
          return
        case 'off':
          break
        case 'audio':
        case 'text':
          break
      }

      if (row.kind === 'off') {
        const accepted = player.setSubtitlesSilent(true)
        this.silent = accepted
        this.subtitleLine = ''
        saveSettings(rememberChoice(settings(), { kind: 'off' }))
        this.closeMenu()
        this.status = accepted
          ? 'Subtitles off.'
          : 'The decoder would not hide its subtitles for this file.'
        return
      }
      const track = row.track
      if (track === null) {
        this.closeMenu()
        return
      }
      const type = row.kind === 'audio' ? 'AUDIO' : 'TEXT'
      if (!player.selectTrack(type, track.index)) {
        this.closeMenu()
        this.status = 'The decoder refused that track for this file.'
        return
      }
      if (row.kind === 'audio') {
        this.audioIndex = track.index
        saveSettings(rememberChoice(settings(), { kind: 'audio', language: track.language }))
        this.closeMenu()
        this.status = `Audio: ${track.label}.`
        return
      }
      this.textIndex = track.index
      this.silent = false
      saveSettings(rememberChoice(settings(), { kind: 'text', language: track.language }))
      this.closeMenu()
      this.status = `Subtitles: ${track.label}.`
    },

    onEnded() {
      player.checkpoint(true)
      this.finished = true
      this.subtitleLine = ''
      const current = this.currentItem()
      const next = current === null ? null : nextEpisode(items(), current)
      if (next === null) {
        this.nextId = ''
        this.nextLabel = ''
        this.status = 'Finished.'
        this.repaint()
        return
      }
      this.nextId = next.id
      const episode =
        next.season !== null && next.episode !== null
          ? `S${String(next.season)} E${String(next.episode)} · `
          : ''
      this.nextLabel = `${episode}${next.title}`
      this.status = `Finished. Enter plays ${this.nextLabel}.`
      this.showOverlay()
      this.repaint()
    },

    playNext() {
      const next = items().find((item) => item.id === this.nextId)
      if (next === undefined) {
        this.nextId = ''
        this.status = 'The next episode is no longer in the library.'
        return
      }
      this.start(next, 0)
      this.showOverlay()
    },
  },
})
