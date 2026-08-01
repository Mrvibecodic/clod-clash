/**
 * clod: colour markers inside the provider banners (`announce`, `clod-promo`).
 *
 * A panel may paint single words by gluing a `#RRGGBB` code to them —
 * `#EF4444ВАЖНО:` shows `ВАЖНО:` in red. The syntax is Prizrak-Box's, so a
 * panel already configured for that client works here unchanged:
 *
 * * the marker binds to the word right after it and ends at the next space;
 * * a marker followed by a space is ordinary text, not a marker;
 * * anything that is not six hex digits (`#XYZ`, `#12`, a lone `#`) stays text.
 *
 * The colour is used exactly as the provider sent it, in both themes — the app
 * does not second-guess a brand colour.
 */
export interface BannerFragment {
  text: string
  /** `#RRGGBB` when the provider asked for a colour. */
  color?: string
  /** Offset in the original text — a stable React key. */
  start: number
}

const COLOUR_MARKER = /#([0-9a-fA-F]{6})(\S+)/g

/** Split banner text into plain and coloured fragments, in order. */
export const parseBannerText = (text: string): BannerFragment[] => {
  const fragments: BannerFragment[] = []
  let cursor = 0

  for (const match of text.matchAll(COLOUR_MARKER)) {
    const start = match.index ?? 0
    if (start > cursor) {
      fragments.push({ text: text.slice(cursor, start), start: cursor })
    }
    fragments.push({
      text: match[2],
      color: `#${match[1]}`,
      start: start + 7,
    })
    cursor = start + match[0].length
  }

  if (cursor < text.length) {
    fragments.push({ text: text.slice(cursor), start: cursor })
  }

  return fragments
}
