// Portuguese (Brazil) translations, keyed by the English UI text. One file per
// area of the app; `pnpm i18n` checks every tr() string has an entry here.

import common from './common.json'
import compat from './compat.json'
import discover from './discover.json'
import explore from './explore.json'
import games from './games.json'
import install from './install.json'
import settings from './settings.json'
import store from './store.json'
import transfers from './transfers.json'

const PT: Record<string, string> = { ...common, ...discover, ...store, ...explore, ...transfers, ...settings, ...compat, ...install, ...games }

export default PT
