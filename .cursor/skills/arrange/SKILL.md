---
name: arrange
description: Improve layout, spacing, and visual rhythm for ImgForge (egui). Fixes crowded settings, inconsistent gaps, weak hierarchy, and clipped bottom rows. Use when the user mentions layout feeling off, spacing issues, crowded UI, alignment problems, squeezed sections, or better composition — especially convert settings and toolbars.
license: Apache-2.0 (upstream layout guidance from pbakaus/impeccable); project egui adaptations local.
---

# Arrange (egui)

Upstream reference (CSS-oriented layout command formerly called `arrange`): [reference/layout.md](reference/layout.md) from [pbakaus/impeccable](https://github.com/pbakaus/impeccable).

**This project is Rust + egui**, not web CSS. Apply the *principles* below; map CSS tools to egui helpers. Do not emit Flex/Grid/Tailwind/`clamp()` unless the user is editing web assets.

## Register

Use the **Product** register from the upstream doc: predictable structure, consistent density, structural responsive behavior. ImgForge convert/settings UIs are data-dense tools — tighter spacing than marketing pages, clear label columns, no decorative card nesting.

## When invoked

1. Read [reference/layout.md](reference/layout.md) for the full assess → plan → improve → verify checklist.
2. Run the **egui mechanical scan** below (Node detector is optional; this repo often has no `node` — do not block on it).
3. Apply fixes using existing helpers in `src/gui/widgets.rs` / `src/gui/theme.rs`. Prefer tokens over magic numbers.

## egui mapping

| Upstream idea | ImgForge / egui |
|---|---|
| Spacing scale | `theme::SECTION_GAP` (12), `PAGE_HEADER_GAP` (16), row gaps 4/6/8; control height `widgets::TOOLBAR_ROW_HEIGHT` (32) |
| Label column | `theme::SETTINGS_LABEL_WIDTH` + `widgets::settings_labeled_row` / `settings_indented` |
| 1D row | `equal_height_row` / `toolbar_row` — lock height before laying out controls |
| 2D form | `checkbox_grid`; avoid `horizontal_wrapped` for single-line toolbars |
| Narrow stack | `SETTINGS_NARROW_BREAKPOINT` (360) for settings forms; do **not** use page `NARROW_BREAKPOINT` (520) inside convert half-columns (~450–480) |
| Section rhythm | Tight gaps inside a fieldset (2–6); `inset_separator` + `settings_subheading` between groups; `section_gap` between cards |
| Hierarchy | One job per grouped section; short controls share a row; long hints go to `settings_indented` weak labels — never cram hints into a fixed-height control row |
| Depth | Existing `grouped_section` frames only; no nested cards inside cards |

## egui mechanical scan (substitute for Node detector)

On the target files, check:

- Magic gaps outside `{2,4,6,8,12,16}` (or documented theme constants)
- `settings_labeled_row` / `equal_height_row` with long weak text or multi-line content inside (clip/`set_height` squeeze)
- Half-column UIs using `NARROW_BREAKPOINT` instead of `SETTINGS_NARROW_BREAKPOINT`
- Consecutive short dropdown/drag rows that could share one labeled row
- Bottom-of-panel content using `available_height()` to fill leftover space inside an outer scroll (empty tall frames / clipped last rows)

## Assess (required)

Work the five dimensions from upstream: Spacing, Visual hierarchy, Grid & structure, Rhythm & variety, Density — citing concrete widgets/lines in the target UI.

## Improve (egui rules of thumb)

- **Related tight, groups open**: sibling controls 6px; after a subheading 2–4px; between major blocks `inset_separator` or `section_gap`.
- **Fixed label lane**: every settings row shares `SETTINGS_LABEL_WIDTH`; secondary inline labels ("类型", "认证") are muted and sit *inside* the control column.
- **Share rows for short widgets**: format stays alone if needed; quality slider+presets; JIRA API+auth; attach+concurrency; tags+priority.
- **Hints below**: concurrency/auth/priority explanations on indented lines — never beside a DragValue in a 32px row.
- **Control height**: `add_sized(..., TOOLBAR_ROW_HEIGHT)` for TextEdit/DragValue/combos in settings rows.
- **Don't** reintroduce `allocate_exact_size` + `scope_builder` for toolbar buttons (known staircase bug).

## Verify

- Squint test: section headings still readable as groups; primary convert fields above advanced (RAW / brightness / remote / JIRA).
- Last visible row in a scrollable column is not vertically clipped; empty states are content-sized.
- Wide convert half-column keeps label-left layout; only &lt;360 stacks labels above.
- `cargo check --features gui` after UI edits.
