# Alternate icons

Not built. Kept so a decision that was already made once doesn't have to be
re-made from scratch.

## `dark-tile.svg`

The first shipped direction: near-black squircle, the mark stroked in the orange
ramp, a 32-unit orange edge ring. The current icon is this one with the colour
roles swapped — same two gradients, exchanged.

Three details in it are load-bearing, and are the reason this file exists rather
than just the PNG:

- **One gradient in user space.** All five strokes reference a single
  `linearGradient` with `gradientUnits="userSpaceOnUse"`. Anchored to the icon's
  coordinates rather than each path's own bounding box, which is what stops a
  seam appearing where the arms leave the trunk.
- **The halo is a blurred copy of the graphic, not a flood.** That is what makes
  it self-coloured — amber alongside the tips, ember alongside the root — and
  lets it fade with distance instead of stopping at an edge. A second, wider
  glow is masked to the lower half, because the dim ember under-glows the bright
  amber at a shared sigma.
- **The ring is inset by half its stroke width**, so its outer edge lands on the
  squircle boundary and the icon keeps its footprint on the macOS grid.

## Swapping one in

Rasterize to 1024 first — these use filters and clip paths, and tools that
render SVG filters loosely will not reproduce them:

```sh
chrome --headless=new --default-background-color=00000000 \
  --screenshot=icon-1024.png --window-size=1024,1024 file://$PWD/dark-tile.svg
cp dark-tile.svg ../icon.svg
npx tauri icon icon-1024.png
rm -rf ../ios ../android          # no mobile targets in this project
```
