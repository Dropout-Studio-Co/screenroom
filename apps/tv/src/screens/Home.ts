import Blits from '@lightningjs/blits'

import { COLUMNS, ROWS, emptyState, homeShelf } from '../core/browse.ts'
import {
  gridWindow,
  moveNav,
  RAIL_ENTRIES,
  railIndexFor,
  restoreNav,
  type NavPosition,
  type NavZone,
} from '../core/nav.ts'
import { APP_WIDTH, buildCards, gridFor } from '../design/cards.ts'
import { railRows, RAIL_WIDTH, type RailRow } from '../design/rail.ts'
import Rail from '../components/Rail.ts'
import { hex, TOKENS } from '../design/tokens.ts'
import {
  items,
  mediaRoots,
  nav,
  onTv,
  scanInProgress,
  sources,
} from '../app/runtime.ts'

const ROUTE = '/'

const CONTENT_LEFT = RAIL_WIDTH + Math.round(TOKENS.spacing.shellPad * 0.6)

export default Blits.Component('Home', {

  components: { Rail },
  template: `
    <Element w="1920" h="1080" :color="$background">
      <Rail :rows="$railRows" />

      <Text
        :x="$contentLeft"
        :y="$pad"
        :content="$heading"
        :size="$headingSize"
        :color="$foreground"
      />
      <Text
        :x="$contentLeft"
        :y="$subY"
        :content="$subheading"
        :size="$bodySize"
        :color="$muted"
        maxlines="1"
        textoverflow="ellipsis"
        :maxwidth="$noteWidth"
      />

      <Element
        :for="card in $cards"
        key="$card.id"
        :x="$card.x"
        :y="$card.y"
        :w="$card.width"
        :h="$card.height"
        :color="$card.fill"
        :scale="$card.scale"
        :rounded="$radius"
      >
        <Element
          x="0"
          y="0"
          :w="$card.width"
          :h="$artHeight"
          :color="$surface2"
          :rounded="$radius"
        />
        <Text
          :x="$fieldX"
          :y="$titleY"
          :content="$card.title"
          :size="$titleSize"
          :color="$card.titleColor"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$cardTextWidth"
        />
        <Text
          :x="$fieldX"
          :y="$subtitleY"
          :content="$card.subtitle"
          :size="$captionSize"
          :color="$card.subtitleColor"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$cardTextWidth"
        />
        <Element
          x="0"
          :y="$progressY"
          :w="$card.progressWidth"
          :h="$barHeight"
          :color="$card.progressColor"
          :show="$card.progressVisible"
        />
      </Element>

      <Element :show="$emptyTitle !== ''">
        <Text
          :x="$contentLeft"
          :y="$emptyY"
          :content="$emptyTitle"
          :size="$titleSize"
          :color="$foreground"
        />
        <Text
          :x="$contentLeft"
          :y="$emptyDetailY"
          :content="$emptyDetail"
          :size="$bodySize"
          :color="$muted"
          maxlines="3"
          wordwrap="true"
          :maxwidth="$emptyWidth"
        />
      </Element>

      <Text :x="$contentLeft" :y="$footY" :content="$footer" :size="$captionSize" :color="$disabled" />
      <Text
        :x="$rowX"
        :y="$footY"
        :content="$positionLabel"
        :size="$captionSize"
        :color="$disabled"
      />
    </Element>
  `,
  state() {
    return {
      railRows: [] as RailRow[],
      background: hex(TOKENS.palette.background),
      foreground: hex(TOKENS.palette.foreground),
      muted: hex(TOKENS.palette.inkMuted),
      disabled: hex(TOKENS.palette.disabledForeground),
      surface2: hex(TOKENS.palette.surface2),
      pad: Math.round(TOKENS.spacing.shellPad),
      contentLeft: CONTENT_LEFT,
      noteWidth: APP_WIDTH - CONTENT_LEFT - Math.round(TOKENS.spacing.shellPad),
      rowX: APP_WIDTH - Math.round(TOKENS.spacing.shellPad) - 300,
      radius: Math.round(TOKENS.radius.surface),
      headingSize: Math.round(TOKENS.type.title),
      titleSize: Math.round(TOKENS.type.bodyLg),
      bodySize: Math.round(TOKENS.type.body),
      captionSize: Math.round(TOKENS.type.caption),
      subY: Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 1.4),

      artHeight: 0,
      fieldX: 0,
      titleY: 0,
      subtitleY: 0,
      progressY: 0,
      barHeight: 6,
      cardTextWidth: 0,
      emptyY: 0,
      emptyDetailY: 0,
      emptyWidth: 0,
      footY: 0,
      heading: 'Your library',
      subheading: '',
      footer: '',
      positionLabel: '',
      emptyTitle: '',
      emptyDetail: '',
      cards: [] as ReturnType<typeof buildCards>,

      shelfFirst: 0,
      count: 0,
      zone: 'rail' as NavZone,
      railIndex: 0,
      contentIndex: 0,
    }
  },
  computed: {

    grid() {
      const top = Math.round(
        TOKENS.spacing.shellPad + TOKENS.type.title * 1.4 + TOKENS.type.body * 2.6,
      )
      return gridFor(COLUMNS, ROWS, TOKENS, APP_WIDTH, {
        top,
        bottom: 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 2.4),
        left: CONTENT_LEFT,
      })
    },
  },
  hooks: {
    ready() {
      this.applyLayout()
      this.reload()
    },
    focus() {
      this.reload()
    },
  },

  input: {
    left() {
      this.press('left')
    },
    right() {
      this.press('right')
    },
    up() {
      this.press('up')
    },
    down() {
      this.press('down')
    },
    enter() {
      this.enter()
    },
  },
  methods: {

    applyLayout(): void {
      const grid = this.grid
      this.artHeight = grid.artHeight
      this.fieldX = Math.round(TOKENS.spacing.fieldX)
      this.titleY = grid.titleY
      this.subtitleY = grid.subtitleY
      this.progressY = grid.artHeight - 6
      this.barHeight = 6
      this.cardTextWidth = grid.cardWidth - this.fieldX * 2
      this.emptyY = Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 2.6)
      this.emptyDetailY = Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 3.6)
      this.emptyWidth = APP_WIDTH - CONTENT_LEFT - Math.round(TOKENS.spacing.shellPad)
      this.footY = 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 2.4)
    },

    bounds(): { railCount: number; contentCount: number; columns: number } {
      return {
        railCount: RAIL_ENTRIES.length,
        contentCount: homeShelf(items()).items.length,
        columns: COLUMNS,
      }
    },

    reload(): void {
      const all = items()
      const situation = emptyState({
        sourceCount: sources().length,
        catalogCount: all.length,
        scanning: scanInProgress(),
        unavailableSources: sources().filter((source) => source.state !== 'available').length,
        filterText: '',
      })
      this.emptyTitle = situation?.title ?? ''
      this.emptyDetail = situation?.detail ?? ''

      const position = restoreNav(nav(), ROUTE, this.bounds(), 'rail')
      this.zone = position.zone
      this.railIndex = position.rail
      this.contentIndex = position.content
      this.repaint()

      const drives = mediaRoots().length
      const driveLine =
        drives === 0 ? 'no drive found' : `${drives} ${drives === 1 ? 'drive' : 'drives'} found`
      this.footer = onTv()
        ? `OK opens · ◄ ► ▲ ▼ move · ${driveLine}`
        : `Browser preview — nothing here proves TV playback · ${driveLine}`
    },

    repaint(): void {
      const shelf = homeShelf(items())
      this.heading = shelf.heading
      this.subheading = shelf.note
      this.count = shelf.items.length
      if (this.count === 0) {
        this.contentIndex = -1
      } else {
        this.contentIndex = Math.min(this.contentIndex, this.count - 1)
      }

      const window = gridWindow(this.count, COLUMNS, ROWS, this.contentIndex)
      this.shelfFirst = window.first
      this.cards = buildCards(
        shelf.items.slice(window.first, window.first + window.count),

        this.zone === 'content' ? this.contentIndex : -1,
        this.grid,
        TOKENS,
        { first: window.first, columns: COLUMNS },
      )
      this.railRows = railRows(railIndexFor(ROUTE), this.railIndex, this.zone === 'rail')
      this.positionLabel =
        this.count === 0
          ? ''
          : `Row ${Math.floor(Math.max(0, this.contentIndex) / COLUMNS) + 1} of ${window.totalRows}`
      nav().set(
        ROUTE,
        { zone: this.zone, rail: this.railIndex, content: this.contentIndex },
        this.bounds(),
      )
    },

    press(direction: 'left' | 'right' | 'up' | 'down'): void {
      const current: NavPosition = {
        zone: this.zone,
        rail: this.railIndex,
        content: this.contentIndex,
      }
      const next = moveNav(current, direction, this.bounds())
      if (
        next.zone === this.zone &&
        next.rail === this.railIndex &&
        next.content === this.contentIndex
      ) {
        return
      }
      this.zone = next.zone
      this.railIndex = next.rail
      this.contentIndex = next.content
      this.repaint()
    },

    enter(): void {
      if (this.zone === 'rail') {
        const entry = RAIL_ENTRIES[this.railIndex]
        if (entry === undefined) {
          return
        }
        if (entry.route !== ROUTE) {
          this.$router.to(entry.route)
          return
        }

        if (this.count > 0) {
          this.zone = 'content'
          this.contentIndex = Math.max(0, this.contentIndex)
          this.repaint()
        }
        return
      }
      const card = this.cards.find((candidate) => candidate.focused)
      if (card === undefined) {
        return
      }
      this.$router.to('/detail', { id: card.id })
    },
  },
})
