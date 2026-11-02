import Blits from '@lightningjs/blits'

import { hex, TOKENS } from './design/tokens.ts'
import { prepare } from './app/runtime.ts'
import { pageTransition } from './app/page-transition.ts'
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
      { path: '/', component: Home, transition: pageTransition },
      { path: '/movies', component: Browse, data: { kind: 'movies' }, transition: pageTransition },
      { path: '/series', component: Browse, data: { kind: 'series' }, transition: pageTransition },
      { path: '/search', component: Search, transition: pageTransition },
      { path: '/sources', component: Sources, transition: pageTransition },
      { path: '/detail', component: Detail, transition: pageTransition },
      { path: '/settings', component: Settings, transition: pageTransition },
      { path: '/player', component: Player, transition: pageTransition },
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
