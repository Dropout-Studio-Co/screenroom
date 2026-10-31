import Blits from '@lightningjs/blits'

import { hex, TOKENS } from './design/tokens.ts'
import { prepare } from './app/runtime.ts'
import { exitApp } from './platform/tizen.ts'
import Browse from './screens/Browse.ts'
import Detail from './screens/Detail.ts'
import Home from './screens/Home.ts'
import Player from './screens/Player.ts'
import Search from './screens/Search.ts'
import Settings from './screens/Settings.ts'
import Sources from './screens/Sources.ts'

function isBackKey(keyCode: number): boolean {

  return keyCode === 10009 || keyCode === 8
}

export default Blits.Application({
  state() {
    return {
      background: hex(TOKENS.palette.background),
    }
  },
  template: `
    <Element w="1920" h="1080" :color="$background">
      <RouterView w="1920" h="1080" />
    </Element>
  `,
  router: {

    routes: [
      { path: '/', component: Home },
      { path: '/movies', component: Browse, data: { kind: 'movies' } },
      { path: '/series', component: Browse, data: { kind: 'series' } },
      { path: '/search', component: Search },
      { path: '/sources', component: Sources },
      { path: '/detail', component: Detail },
      { path: '/settings', component: Settings },
      { path: '/player', component: Player },
    ],

    backNavigation: true,
  },
  hooks: {
    ready() {

      prepare()
    },
  },
  input: {

    intercept(event) {
      if (!isBackKey(event.keyCode)) {
        return event
      }
      const path = this.$router.currentRoute.path
      if (path !== '/') {
        return event
      }

      if (!exitApp()) {
        this.$log.info('Screenroom: at the home screen with nowhere to go back to.')
      }
      return undefined
    },
  },
})
