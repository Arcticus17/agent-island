# Agent Island UI Compatibility Design

## Goal

Keep Agent Island's existing features and visual direction while making its Chinese interface reliable and usable across Windows DPI settings, display sizes, multi-monitor setups, narrow overview windows, keyboard use, touch input, and reduced-motion preferences.

## Scope

- Preserve Chinese as the default interface language and validate that source files are UTF-8.
- Make the island's compact and expanded layouts fit the available viewport instead of relying on fixed dimensions.
- Keep popovers, notification cards, paths, controls, and action groups inside a narrow island without obscuring essential actions.
- Make the overview window reflow safely on small sizes and retain usable scrolling for its list, detail, action, and message areas.
- Improve focus visibility, pointer/touch target sizing, dark/light system colors, and reduced-motion behavior.
- Clamp restored island positions and size changes to the active monitor's work area in the Tauri layer so a display/DPI change cannot leave it inaccessible.

## Non-goals

- No new Agent integrations, remote-control semantics, analytics, or visual redesign.
- No change to the existing persistence keys or user configuration meanings.
- No network/API changes.

## Architecture

CSS owns responsive layout: custom properties set safe widths/heights, container/viewport media queries collapse grids and wrap controls, and all text-bearing rows retain ellipsis or scroll affordances. JavaScript derives user-selected island width from a viewport-safe clamp and recalculates it when the viewport changes. Rust validates and clamps saved/restored window geometry against available monitor work areas before applying it.

## User Flow

1. On a high-DPI, small, or narrow display, the compact island stays fully visible and its label truncates rather than pushing controls off-screen.
2. When expanded, the island uses the available height; its body scrolls while controls remain reachable.
3. Menus anchor within the visible island and long labels/paths truncate gracefully.
4. The overview window switches from a horizontal header/action layout to wrapping, stacked controls on narrow dimensions.
5. After monitor removal, DPI scaling changes, or a restored off-screen position, the main window is moved into a visible work area.

## Error Handling

- Viewport calculations fall back to a conservative minimum width when browser dimensions are temporarily unavailable.
- Geometry clamping is best-effort: when monitor information cannot be resolved, existing position behavior is retained rather than failing application startup.
- UI controls remain keyboard accessible and disabled states continue to reflect current Agent capabilities.

## Verification

- Build the Vite frontend and run Rust unit tests.
- Inspect responsive CSS breakpoints and the emitted frontend bundle for valid Chinese strings and malformed markup.
- Run the Tauri build/check to compile geometry changes.
- Manually exercise compact/expanded demo mode at narrow and wide viewport sizes, including light theme and reduced-motion settings.
