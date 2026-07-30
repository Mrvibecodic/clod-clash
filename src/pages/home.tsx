import { useSimpleMode } from '@/hooks/use-simple-mode'

import HomeAdvancedPage from './home-advanced'
import HomeSimplePage from './home-simple'

/**
 * clod: both home screens are ours. The simple one is a single column around
 * the Connect button; the advanced one is the same functions on a wider
 * canvas plus quick tiles into the deeper sections. The upstream dashboard
 * (cards grid) was removed with the redesign — the deep pages it linked to
 * are all still reachable from the sidebar and the tiles.
 */
const HomePage = () => {
  const { simpleMode } = useSimpleMode()
  return simpleMode ? <HomeSimplePage /> : <HomeAdvancedPage />
}

export default HomePage
