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
 *
 * The scanner mirrors `colour_marker_at`/`truncate_banner` on the Rust side
 * character for character: every marker is zero-width formatting, chained
 * markers (`#AAAAAA#BBBBBBword`) are all consumed with the last one winning,
 * and "whitespace" means Unicode White_Space (what `char::is_whitespace`
 * uses), not the JS `\s` class. If the two sides ever disagree on what counts
 * as a marker, the 500-visible-character limit enforced in Rust drifts from
 * what the user actually sees.
 */
export interface BannerFragment {
  text: string
  /** `#RRGGBB` when the provider asked for a colour. */
  color?: string
  /** Offset in the original text — a stable React key. */
  start: number
}

const HEX_DIGIT = /[0-9a-fA-F]/

/** Unicode White_Space — same set as Rust's `char::is_whitespace`. */
const isWhitespace = (ch: string) =>
  (/\s/.test(ch) && ch !== '\uFEFF') || ch === '\u0085'

/** `#RRGGBB` glued to a non-space character, or `null`. */
const markerAt = (text: string, index: number): string | null => {
  if (text[index] !== '#' || index + 7 > text.length) return null
  const next = text[index + 7]
  if (next === undefined || isWhitespace(next)) return null
  const code = text.slice(index + 1, index + 7)
  for (const ch of code) {
    if (!HEX_DIGIT.test(ch)) return null
  }
  return code
}

/** Split banner text into plain and coloured fragments, in order. */
export const parseBannerText = (text: string): BannerFragment[] => {
  const fragments: BannerFragment[] = []
  let buf = ''
  let bufStart = 0
  let bufColor: string | undefined

  const flush = () => {
    if (!buf) return
    fragments.push(
      bufColor
        ? { text: buf, color: bufColor, start: bufStart }
        : { text: buf, start: bufStart },
    )
    buf = ''
  }

  let color: string | undefined
  let index = 0
  while (index < text.length) {
    const code = markerAt(text, index)
    if (code) {
      // Zero-width formatting, exactly like the Rust budget counter: chained
      // markers are all consumed and the last one paints the word.
      color = `#${code}`
      index += 7
      continue
    }
    const ch = text[index]
    if (isWhitespace(ch)) {
      color = undefined
    }
    if (!buf || bufColor !== color) {
      flush()
      bufStart = index
      bufColor = color
    }
    buf += ch
    index += 1
  }
  flush()

  return fragments
}
