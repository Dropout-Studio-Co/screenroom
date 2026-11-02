import Blits from '@lightningjs/blits'

import { describeSearch, SEARCH_KEYS, searchResults, withKey } from '../core/search.ts'
import { gridWindow, moveNav, RAIL_ENTRIES, railIndexFor } from '../core/nav.ts'
import { moveFocus } from '../core/focus.ts'
import { subtitleFor } from '../design/cards.ts'
import { keyCells, keyGeometry, resultRowHeight, SEARCH_COLUMNS, SEARCH_RESULT_ROWS, type KeyCell } from '../design/search.ts'
import { railRows, RAIL_WIDTH, type RailRow } from '../design/rail.ts'
import Rail from '../components/Rail.ts'
import { focusMotion } from '../core/motion.ts'
import { hex, TOKENS } from '../design/tokens.ts'
import { items, rememberSearch, searchSession } from '../app/runtime.ts'

const ROUTE = '/search'

type SearchZone = 'rail' | 'keys' | 'results'

const CONTENT_LEFT = RAIL_WIDTH + Math.round(TOKENS.spacing.shellPad * 0.6)

interface ResultRow {
  readonly id: string
  readonly title: string
  readonly subtitle: string
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly fill: string
  readonly titleColor: string
  readonly subtitleColor: string
  readonly focused: boolean
  readonly scale: number
}

export default Blits.Component('Search', {
  components: { Rail },
  template: `
    <Element w="1920" h="1080" :color="$background">
      <Rail :rows="$railRows" />

      <Text :x="$contentLeft" :y="$pad" content="Search" :size="$headingSize" :color="$foreground" />
      <Text
        :x="$contentLeft"
        :y="$subY"
        :content="$queryLabel"
        :size="$bodySize"
        :color="$foreground"
        maxlines="1"
        textoverflow="ellipsis"
        :maxwidth="$queryWidth"
      />
      <Text
        :x="$contentLeft"
        :y="$statusY"
        :content="$status"
        :size="$captionSize"
        :color="$muted"
        maxlines="2"
        wordwrap="true"
        :maxwidth="$statusWidth"
      />

      <Element
        :for="cap in $keys"
        key="$cap.id"
        :x="$cap.x"
        :y="$cap.y"
        :w="$cap.width"
        :h="$cap.height"
        :color="$cap.fill"
        :rounded="$radius"
      >
        <Text
          :x="0"
          :y="$keyLabelY"
          :w="$cap.width"
          :content="$cap.label"
          :size="$keyLabelSize"
          :color="$cap.labelColor"
          align="center"
          maxlines="1"
        />
      </Element>

      <Text
        :x="$resultsX"
        :y="$keysTop"
        content="Matches"
        :size="$captionSize"
        :color="$muted"
      />
      <Element
        :for="result in $results"
        key="$result.id"
        :x="$result.x"
        :y="$result.y"
        :w="$result.width"
        :h="$result.height"
        :color="$result.fill"
        :scale.transition="{value: $result.scale, duration: $focus.duration, easing: $focus.ease}"
        :rounded="$radius"
      >
        <Text
          :x="$resultPad"
          :y="$resultTitleY"
          :content="$result.title"
          :size="$titleSize"
          :color="$result.titleColor"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$resultTextWidth"
        />
        <Text
          :x="$resultPad"
          :y="$resultSubtitleY"
          :content="$result.subtitle"
          :size="$captionSize"
          :color="$result.subtitleColor"
          maxlines="1"
          textoverflow="ellipsis"
          :maxwidth="$resultTextWidth"
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
      headingSize: Math.round(TOKENS.type.title),
      titleSize: Math.round(TOKENS.type.bodyLg),
      bodySize: Math.round(TOKENS.type.body),
      captionSize: Math.round(TOKENS.type.caption),
      radius: Math.round(TOKENS.radius.surface),
      subY: Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 1.4),
      statusY: Math.round(TOKENS.spacing.shellPad + TOKENS.type.title * 1.4 + TOKENS.type.body * 1.4),
      queryWidth: 1500,
      statusWidth: 1500,
      keysTop: 0,
      keyLabelSize: Math.round(TOKENS.type.body),
      keyLabelY: 0,
      resultsX: 0,
      resultPad: 0,
      resultTitleY: 0,
      resultSubtitleY: 0,
      resultTextWidth: 0,
      resultWidth: 0,
      resultHeight: 0,
      resultsTop: 0,
      footY: 0,
      query: '',
      queryLabel: '',
      status: '',
      footer: '',
      keys: [] as KeyCell[],
      results: [] as ResultRow[],
      matchCount: 0,
      zone: 'keys' as SearchZone,
      railIndex: 0,
      keyIndex: 0,
      resultIndex: 0,
      focus: focusMotion(),
    }
  },
  computed: {
    keyboard() {
      return keyGeometry(TOKENS)
    },
  },
  hooks: {
    ready() {
      this.$focus()
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
      const keyboard = this.keyboard
      const keysTop = Math.round(
        TOKENS.spacing.shellPad + TOKENS.type.title * 1.4 + TOKENS.type.body * 3.4,
      )
      const resultsX = CONTENT_LEFT + keyboard.panelWidth + Math.round(TOKENS.spacing.shellPad * 1.2)
      this.keysTop = keysTop
      this.resultsTop = keysTop + Math.round(TOKENS.type.caption * 2)
      this.resultsX = resultsX
      this.resultWidth = 1920 - Math.round(TOKENS.spacing.shellPad) - resultsX
      this.resultHeight = resultRowHeight(TOKENS)
      this.resultPad = Math.round(TOKENS.spacing.fieldX)
      this.resultTitleY = Math.round(TOKENS.type.caption * 0.5)
      this.resultSubtitleY = Math.round(TOKENS.type.caption * 0.5 + TOKENS.type.bodyLg * 1.2)
      this.resultTextWidth = this.resultWidth - this.resultPad * 2
      this.keyLabelY = Math.round((keyboard.height - this.keyLabelSize * 1.25) / 2)
      this.footY = 1080 - Math.round(TOKENS.spacing.shellPad + TOKENS.type.body * 2.4)
    },

    reload(): void {
      const session = searchSession()
      this.query = session.query
      this.keyIndex = session.keyIndex
      this.resultIndex = session.resultIndex
      this.railIndex = session.railIndex
      this.zone = session.zone
      this.repaint()
      this.footer = 'OK types · ▲▼◄► move · Left from the first column opens the menu · Back returns'
    },

    repaint(): void {
      const matches = searchResults(items(), this.query)
      this.matchCount = matches.length
      if (this.resultIndex > matches.length - 1) {
        this.resultIndex = 0
      }
      const window = gridWindow(matches.length, 1, SEARCH_RESULT_ROWS, this.resultIndex)
      this.results = matches
        .slice(window.first, window.first + window.count)
        .map((item, index) => {
          const focused = this.zone === 'results' && window.first + index === this.resultIndex
          return {
            id: item.id,
            title: item.title,
            subtitle: subtitleFor(item),
            x: this.resultsX,
            y: this.resultsTop + index * (this.resultHeight + 8),
            width: this.resultWidth,
            height: this.resultHeight,
            fill: focused ? hex(TOKENS.palette.focusSurface) : hex(TOKENS.palette.surface1),
            titleColor: focused
              ? hex(TOKENS.palette.focusForeground)
              : hex(TOKENS.palette.foreground),
            subtitleColor: focused
              ? hex(TOKENS.palette.focusForeground)
              : hex(TOKENS.palette.inkMuted),
            focused,
            scale: focused ? this.focus.lift : 1,
          }
        })

      this.keys = keyCells(this.zone === 'keys' ? this.keyIndex : -1, CONTENT_LEFT, this.keysTop, TOKENS)
      this.railRows = railRows(railIndexFor(ROUTE), this.railIndex, this.zone === 'rail')
      this.queryLabel = `Search: ${this.query === '' ? '—' : this.query}`
      this.status = describeSearch(this.query, matches.length)
      rememberSearch({
        query: this.query,
        keyIndex: this.keyIndex,
        resultIndex: this.resultIndex,
        railIndex: this.railIndex,
        zone: this.zone === 'rail' ? 'rail' : this.zone === 'results' ? 'results' : 'keys',
      })
    },

    press(direction: 'left' | 'right' | 'up' | 'down'): void {
      if (this.zone === 'rail') {
        const next = moveNav(
          { zone: 'rail', rail: this.railIndex, content: this.keyIndex },
          direction,
          {
            railCount: RAIL_ENTRIES.length,
            contentCount: SEARCH_KEYS.length,
            columns: SEARCH_COLUMNS,
          },
        )
        this.railIndex = next.rail
        this.keyIndex = Math.max(0, next.content)
        this.zone = next.zone === 'content' ? 'keys' : 'rail'
        this.repaint()
        return
      }

      if (this.zone === 'keys') {
        const columns = this.keyboard.columns
        if (direction === 'left' && this.keyIndex % columns === 0) {
          this.zone = 'rail'
          this.repaint()
          return
        }
        const next = moveFocus(this.keyIndex, SEARCH_KEYS.length, columns, direction)
        if (next !== this.keyIndex) {
          this.keyIndex = next
          this.repaint()
          return
        }

        if (direction === 'down' && this.matchCount > 0) {
          this.zone = 'results'
          this.resultIndex = 0
          this.repaint()
        }
        return
      }

      if (direction === 'left') {
        this.zone = 'rail'
        this.repaint()
        return
      }
      if (direction === 'up' && this.resultIndex === 0) {
        this.zone = 'keys'
        this.repaint()
        return
      }
      if (direction === 'up' || direction === 'down') {
        const next = moveFocus(this.resultIndex, this.matchCount, 1, direction)
        if (next !== this.resultIndex) {
          this.resultIndex = next
          this.repaint()
        }
      }
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

        this.zone = 'keys'
        this.repaint()
        return
      }
      if (this.zone === 'keys') {
        const key = SEARCH_KEYS[this.keyIndex]
        if (key === undefined) {
          return
        }
        this.query = withKey(this.query, key)
        this.resultIndex = 0
        this.repaint()
        return
      }
      const row = this.results.find((candidate) => candidate.focused)
      if (row === undefined) {
        return
      }
      this.$router.to('/detail', { id: row.id })
    },
  },
})
