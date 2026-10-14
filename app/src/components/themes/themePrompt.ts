import { notify } from '../../lib/notify'
import { BUILTIN_FONTS, GROUPS, SCENES, TOKENS } from '../../lib/packFormat'
import { STYLES } from '../../lib/themes'

/**
 * A prompt for making a theme with an AI assistant outside Venuta: the format, every value and
 * the rules, so what comes back can be imported (or saved as theme.json) as it is. Nothing is
 * sent anywhere: it's copied for you to paste.
 */
export function themePrompt(mood = '<describe the mood, colours, era or album you want>'): string {
  const values = GROUPS.map(
    (g) =>
      `${g}:\n` +
      TOKENS.filter((t) => t.group === g)
        .map((t) => `  ${t.name} (${t.type}): ${t.label}`)
        .join('\n'),
  ).join('\n')
  return `Make a theme for Venuta, a desktop music player. Reply with one theme.json file only.

The theme I want: ${mood}

Format (JSON):
{
  "format": 1,
  "id": "lowercase-with-dashes",
  "name": "Its name",
  "author": "",
  "description": "One line about it.",
  "extends": one of ${STYLES.map((s) => `"${s.id}" (${s.name})`).join(', ')},
  "modes": ["dark"] or ["light"] or ["dark", "light"],
  "accent": {"color": "#rrggbb"} or {"from": "album"} (follows the cover of the song playing) or {"from": "base"},
  "tokens": { value name: CSS value, ... },          // for every mode
  "dark": { "tokens": { ... } }, "light": { "tokens": { ... } },   // per mode, only if it has both
  "fonts": { "ui": "builtin:<font>", "display": "builtin:<font>", "readout": "builtin:<font>" },
  "effects": { "scene": "base" | ${SCENES.map((s) => `"${s}"`).join(' | ')}, "grain": 0-1, "scanlines": 0-1, "halftone": 0-1, "hex": 0-1,
               "wires": bool, "rain": bool, "trails": bool, "split": bool, "wipe": bool, "lcd": bool },
  "mark": { "color": "accent" | "ink" | "#rrggbb", "glow": 0-1 }
}

The theme starts as the one it extends and changes only what it writes; write the values the mood needs, not all of them.

Values (name, kind, what it changes):
${values}

Fonts that can be named as builtin:<font>: ${BUILTIN_FONTS.join(', ')}.

Rules:
- Values are plain CSS: colours (#hex, rgb(), oklch()), gradients, filters (blur(24px) saturate(1.5)), shadows, sizes. A value may use var(--another-value).
- No url(), no @import, no semicolons or braces inside values. Effects are only the ones listed.
- Text must read: ink on app-bg, glass, np-bg, rail-bg and popover at 4.5:1 or more; on-accent on accent at 3:1 or more.
- Translucent panels (rgb(r g b / 0.5)) let the album background show through; keep them readable over a dark or light picture.
`
}

export async function copyThemePrompt() {
  try {
    await navigator.clipboard.writeText(themePrompt())
    notify('Copied. Paste it into an AI assistant, describe your theme, then import the theme.json it makes (or save it in a theme folder).')
  } catch {
    notify("Couldn't copy to the clipboard.")
  }
}
