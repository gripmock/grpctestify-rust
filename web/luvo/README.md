# luvo

The parts of the workbench that are not about gRPC.

`grpctestify play` grew a design system, a set of primitives and a handful of
interaction rules that have nothing to do with `.gctf` files: a palette in two
modes with a contrast grader, a splitter, a context menu that knows how to fit
on a screen, a keyboard layer that matches physical keys, a toast, a modal, a
ranking search. They were spread through `src/lib` and `src/components/ui`,
where each was one import away from learning about endpoints and assertions.

They live here instead. The rule for what belongs in luvo is one line: **it must
not know what a `.gctf` file is.** Nothing here imports from `src/`, and
`luvo.test.ts` fails the suite the day something does.

luvo is a CSS framework first: three stylesheets that a screen is written in,
and the small amount of TypeScript those styles need to behave.

## The stylesheets

- `tokens.css` — the palettes and the scales. Two palettes, each in two modes,
  written in `rgb()` so a contrast grader can read them without a browser.
- `base.css` — the document: the reset, the viewport an app owns rather than
  scrolls, the scrollbars, the focus ring.
- `controls.css` — the vocabulary: `.btn`, `.field`, `.badge`, `.chip`, `.seg`,
  `.menu`, `.row`, `.kv`, `.panel`, `.modal`, `.toast`, `.tabs`, `.split`.

Every rule reads a token. None names a colour, radius, shadow, duration or font
of its own — `theme-reach.test.ts` fails the suite if one does.

## The behaviour those styles need

- `theme/` — the palette registry, light/dark/system resolution, the grader
- `input/` — hotkeys by physical key, dismissal, menu placement, tab-strip keys
- `data/` — storage, clipboard, debounced posts, ranking, diffing
- `ui/` — Splitter, Tabs, ContextMenu, Toast, Modal: the markup the classes
  expect, for the cases where a class alone cannot carry the behaviour.

Imported as `luvo/...` from the app; the stylesheets by `@import`.
