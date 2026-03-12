# Laires GUI Visual Refresh — Implementation Plan

## Context

The egui/eframe GUI (`laires gui`) is functionally complete but visually plain. Three React mockup designs were created to explore a more polished direction. This plan brings the egui GUI up to that standard **without switching to React** — all work stays in Rust with egui/eframe 0.33.

The mockups show three views:
1. **Story Dashboard** — metrics cards, embedded graph, quick insights, branded sidebar
2. **Editor View** — clean prose canvas with right-side analysis panel
3. **Editor View (active highlighting)** — entity detection overlays on prose text

## Design Language (from mockups)

| Element | Design Direction |
|---------|-----------------|
| Layout | Card-based with shadows and generous padding |
| Typography | Sans-serif (Inter) for all UI chrome; serif (Source Serif 4) only for prose |
| Top bar | Branded header: logo + title + search + primary action button |
| Sidebar | Icon-rich navigation with section headers and active indicators |
| Depth | Card shadows, layered background tones, subtle borders |
| Canvas | Clean prose (no line numbers by default), larger font, breadcrumbs |
| Right panel | Analysis sidebar with entity legend, detection status, insights |
| Dashboard | Metric cards with large numbers, embedded graph, quick insights |
| Spacing | 12-16px margins, 8px gaps, breathable layout |

## File Changes Overview

### Modified Files
| File | Changes |
|------|---------|
| `src/gui/theme.rs` | Add Inter font, card frame helpers, shadow constants, spacing updates |
| `src/gui/mod.rs` | Swap bottom status bar → top nav bar, add Dashboard tab, wire right analysis panel |
| `src/gui/state.rs` | Add `RightTab::Dashboard`, `show_line_numbers` toggle, entity highlight state |
| `src/gui/panels/status_bar.rs` | Rewrite as top navigation bar (rename to `top_bar.rs` or keep name) |
| `src/gui/panels/sidebar.rs` | Icons, section styling, active indicator, count badges |
| `src/gui/panels/canvas.rs` | Remove line numbers, larger prose font, breadcrumbs, entity highlights |
| `src/gui/panels/chat.rs` | Message bubbles, avatar circles, styled input area |
| `src/gui/panels/graph_view.rs` | Graph legend overlay, softer edges, node glow on hover |

### New Files
| File | Purpose |
|------|---------|
| `src/gui/panels/dashboard.rs` | Story Dashboard view: metrics cards + embedded graph + quick insights |
| `src/gui/panels/analysis_sidebar.rs` | Right-side entity legend, detection status, insight cards |
| `assets/fonts/Inter-Regular.ttf` | Already present in repo (untracked) |

## Implementation Phases

---

### Phase R1: Typography + Theme Foundation

**Goal:** Inter font for UI, Source Serif for prose only, updated spacing and card helpers.

**Changes in `src/gui/theme.rs`:**

1. Load `Inter-Regular.ttf` (already in `assets/fonts/`) as a third embedded font
2. Create a custom font family `"ui"` mapped to Inter, used as the **default proportional** font
3. Push Source Serif 4 to a named family `"prose"` — used explicitly only in canvas panel
4. Keep JetBrains Mono as monospace

```rust
// Font family setup:
// FontFamily::Proportional → Inter (UI text)
// FontFamily::Name("prose") → Source Serif 4 (canvas/story text)
// FontFamily::Monospace → JetBrains Mono (code/tool output)
```

5. Add a `card_frame()` helper method to `LairesTheme`:

```rust
pub fn card_frame(&self) -> egui::Frame {
    egui::Frame {
        inner_margin: egui::Margin::same(16),
        outer_margin: egui::Margin::same(4),
        corner_radius: CornerRadius::same(12),
        shadow: egui::epaint::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 8.0,
            spread: 0.0,
            color: Color32::from_black_alpha(15),
        },
        fill: Color32::WHITE,
        stroke: Stroke::new(1.0, self.border),
    }
}
```

6. Update global style spacing:

```rust
style.spacing.item_spacing = egui::vec2(8.0, 8.0);   // was (8, 4)
style.spacing.window_margin = egui::Margin::same(16);  // was 12
visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8); // was 4
visuals.window_corner_radius = CornerRadius::same(12);  // was 8
```

7. Soften hover state:

```rust
visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xF0, 0xF4, 0xFF); // light blue tint
```

**Milestone:** All UI text renders in Inter. Prose in canvas still renders in Source Serif 4. Cards have shadows. App feels immediately more modern.

---

### Phase R2: Top Navigation Bar

**Goal:** Replace the bottom status bar with a branded top navigation bar matching the mockups.

**Rewrite `src/gui/panels/status_bar.rs`** (or rename to `top_bar.rs`):

Layout (left to right):
```
┌─────────────────────────────────────────────────────────────────────┐
│ [icon] Laires  │ Project: Title    │  [Search...]  │ [Scan] [Settings] │
│        Writing  │ provider | model  │               │  agent status     │
│        Analysis │                   │               │                   │
└─────────────────────────────────────────────────────────────────────┘
```

- Height: 52px (was 28px)
- Left section: App icon (Unicode `◈` or similar) + "Laires" in bold Inter + subtitle "Writing Analysis Tool" in small secondary text
- Center-left: Project title (clickable) + provider/model info below
- Center: Search input (`TextEdit::singleline` with placeholder "Search story elements...") — functional search can come later, but the input should exist
- Right: Primary action button ("Scan Story" when unscanned, "Write Scene" otherwise) styled as filled accent button + Settings gear + agent status spinner

**Changes in `src/gui/mod.rs`:**
- Change `TopBottomPanel::bottom("status_bar")` → `TopBottomPanel::top("top_bar")`
- Update height and frame

**Milestone:** Professional branded top bar. Bottom of window is clean.

---

### Phase R3: Sidebar Styling

**Goal:** Polished sidebar with icons, spacing, and active indicators.

**Rewrite `src/gui/panels/sidebar.rs`:**

1. **Navigation items** with icons (Unicode glyphs work well in Inter):
   - `📄` or `▸` Scenes
   - `📁` Files
   - `👥` Characters (new — pulls character list from graph)
   - `🌐` World Wiki (placeholder for future)

2. **Active indicator**: 3px indigo left border on selected tab/item

3. **Section headers**: "SCENES (32)" in small caps, secondary color, with count badge

4. **Scene items**: 36px row height, 12px left padding, hover background `bg_panel`, selected background with accent tint

5. **File items**: Show file icon + name + classification badge (Story/Context)

6. **Bottom section**: Author/project info card (optional, matches mockup 1)

**Implementation approach** — replace `selectable_label` with custom painted rows:

```rust
let item_rect = ui.allocate_space(egui::vec2(ui.available_width(), 36.0));
// Paint hover/selected background
// Paint left accent bar if selected
// Paint icon + label text
```

**Milestone:** Sidebar looks purposeful and polished. Visual hierarchy is clear.

---

### Phase R4: Canvas Cleanup

**Goal:** Clean prose reading experience matching mockups 2-3.

**Rewrite `src/gui/panels/canvas.rs`:**

1. **Remove line numbers** by default. Add `state.show_line_numbers` toggle (accessible via right-click context menu or settings).

2. **Breadcrumb header** above the text:
   ```
   PROJECT ALPHA > CHAPTER 4
   Scene 1: The Midnight Encounter
   Estimated reading time: 4 minutes · 1,240 words
   ```
   - Project name from config, chapter/scene from selected scene
   - Reading time calculated: `word_count / 250`

3. **Prose font sizing**: Use `FontFamily::Name("prose")` at 16px (was default ~14px). Set line spacing via `ui.spacing_mut().item_spacing.y = 6.0` within the canvas scope.

4. **Scene boundaries**: Instead of red text, use a subtle horizontal rule + scene title in small secondary text above the boundary line.

5. **Generous margins**: 32px left/right padding within the canvas card, 24px top.

6. **Wrap in card frame**: Use `theme.card_frame()` around the entire canvas area.

**Milestone:** Canvas reads like a proper manuscript viewer, not a code editor.

---

### Phase R5: Chat Panel Polish

**Goal:** Message bubbles and styled input replacing flat text.

**Rewrite `src/gui/panels/chat.rs`:**

1. **Message bubbles**:
   - User messages: Light accent background (`#EDF2FF`), right-aligned, rounded corners (12px top, 12px top-left, 4px bottom-right, 12px bottom-left)
   - Assistant messages: White card background, left-aligned, full-width
   - System/error: Subtle banner style (no bubble, just tinted background strip)

2. **Avatar circles**: 28px circle with initial letter — "Y" (you) in accent color for user, "L" in indigo for Laires assistant

3. **Tool calls**: Keep collapsible but style the header as a small pill/badge: `[scan_story]` in monospace on a light gray background

4. **Input area**:
   - Rounded input field with 1px border, 12px corner radius
   - Send button: Filled accent circle with arrow icon (or just "Send" in white on accent)
   - Placeholder: "Ask Laires about your story..." in secondary text

5. **Typing indicator**: Replace bare spinner with a subtle animated dots indicator inside a small bubble

**Milestone:** Chat feels conversational and polished, not like a debug log.

---

### Phase R6: Dashboard View

**Goal:** New Story Dashboard tab (default view when opening a project).

**New file `src/gui/panels/dashboard.rs`:**

**Add `RightTab::Dashboard`** to `state.rs` — make it the default when a project loads.

**Layout:**
```
┌──────────────────────────────────────────────────┐
│  Narrative Graph          │  Story Overview       │
│  [Timeline] [Connections] │  ┌──────┐ ┌────────┐ │
│                           │  │42,850│ │   14   │ │
│    (graph view embedded)  │  │words │ │ chars  │ │
│                           │  ├──────┤ ├────────┤ │
│                           │  │  32  │ │  94%   │ │
│                           │  │scenes│ │analyzed│ │
│  Legend: ● Plot ● Char    │  └──────┘ └────────┘ │
│          ● Scene ● Conf   │                       │
│                           │  Current Milestone    │
│                           │  ████████░░ 65K words │
├───────────────────────────┴───────────────────────┤
│  Quick Insights                                    │
│  ┌─────────────┐ ┌──────────────┐ ┌─────────────┐ │
│  │ Char Arcs   │ │ Pacing       │ │ Conflict    │ │
│  │ Karl: Strong│ │ ▁▂▃▅▇█▅▃    │ │ Ext: ███    │ │
│  │ Elara: Dev  │ │ peaks at Ch12│ │ Int: ██     │ │
│  └─────────────┘ └──────────────┘ └─────────────┘ │
└────────────────────────────────────────────────────┘
```

**Metric cards**: Each is a `card_frame()` containing:
- Large number in `FontId::proportional(28.0)` accent color
- Label below in small secondary text
- Computed from existing snapshot data (word_count, char_count, scene_count, analysis %)

**Embedded graph**: Reuse existing `graph_view::render()` in a sub-region (top-left quadrant)

**Quick Insights row**: Three cards at the bottom:
- Character Arcs: List top characters + arc status (from graph analysis)
- Pacing Analysis: Simple bar chart (word counts per scene, drawn with `Painter::rect_filled`)
- Conflict Density: Horizontal bars for external/internal/thematic conflicts

**Laires Insight callout**: Accent-bordered card with a key insight from the most recent analysis (stored in graph node data).

**Milestone:** Opening a project shows a rich overview instead of an empty canvas.

---

### Phase R7: Analysis Sidebar (Canvas Right Panel)

**Goal:** Entity legend + detection status panel shown alongside the canvas, matching mockups 2-3.

**New file `src/gui/panels/analysis_sidebar.rs`:**

Shown as a right panel (200-250px wide) **only when the Canvas tab is active**.

**Sections:**

1. **Token Legend** — toggleable entity type highlights:
   - `● Characters` (indigo) — toggle on/off
   - `● Locations` (green) — toggle on/off
   - `● Objects` (amber) — toggle on/off
   - Toggling highlights matching entities in the canvas text

2. **Detection Status** — horizontal progress bars:
   - Resolved: percentage bar (green)
   - Partially Known: percentage bar (amber)
   - Unknown: percentage bar (gray)
   - Data from graph analysis completeness

3. **How it works** card — small explainer:
   - "Our NLP engine scans your text for Named Entities (NER) and cross-references them with your project's Story Graph."
   - "View Full Analysis Data" link button

**Canvas entity highlighting** (pairs with this sidebar):
- When a legend toggle is active, scan visible text for known entity names (from graph nodes of that type)
- Render colored underlines beneath matched spans using `Painter::line_segment` below the text baseline
- This is computationally cheap since we only check visible lines against a small set of known entity names

**Milestone:** Canvas view has a professional analysis sidebar. Entity names are visually highlighted in the prose.

---

### Phase R8: Graph View Refinements

**Goal:** Polish the force-directed graph to match the mockup's cleaner aesthetic.

**Changes in `src/gui/panels/graph_view.rs`:**

1. **Softer edges**: Increase edge stroke width to 1.5px, use a lighter color (`#CED4DA` instead of `#DEE2E6`)

2. **Node rendering upgrade**:
   - Filled circle + thin white stroke border (2px white outline)
   - On hover: subtle glow (larger semi-transparent circle behind)
   - Selected: thicker accent ring

3. **Node labels**: White text centered inside the node circle (for short labels) OR below for long labels. Use Inter (UI font) not serif.

4. **Legend overlay**: Bottom-left corner of graph area:
   ```
   ● Core Plot   ● Character Arc
   ● Scene       ● Conflict Point
   ```
   Rendered in a small semi-transparent card

5. **Tab pills**: "Timeline" / "Connections" toggle above graph (cosmetic for now — both show same graph, but prepares for future view modes)

6. **Curved edges**: Use quadratic bezier (`Painter::add(Shape::QuadraticBezier(...))`) for edges between nodes, instead of straight lines — looks significantly more professional

**Milestone:** Graph view matches the polished aesthetic of the mockup.

---

## Implementation Priority

| Phase | Impact | Effort | Priority |
|-------|--------|--------|----------|
| R1: Typography + Theme | Very High | Small (1-2hrs) | 1st |
| R2: Top Navigation Bar | High | Medium (2-3hrs) | 2nd |
| R3: Sidebar Styling | Medium-High | Medium (2-3hrs) | 3rd |
| R4: Canvas Cleanup | High | Medium (2-3hrs) | 4th |
| R5: Chat Bubbles | Medium | Medium (2-3hrs) | 5th |
| R6: Dashboard View | High | Large (4-5hrs) | 6th |
| R7: Analysis Sidebar | Medium | Large (3-4hrs) | 7th |
| R8: Graph Refinements | Medium | Medium (2-3hrs) | 8th |

**R1-R4 are the critical path** — they transform the app's feel with the least code. R5-R8 add depth and completeness.

## Font Licensing

| Font | License | Embedded |
|------|---------|----------|
| Inter Regular | SIL OFL 1.1 | `assets/fonts/Inter-Regular.ttf` |
| Source Serif 4 Regular | SIL OFL 1.1 | `assets/fonts/SourceSerif4-Regular.otf` |
| JetBrains Mono Regular | Apache 2.0 | `assets/fonts/JetBrainsMono-Regular.ttf` |

All three licenses permit embedding in compiled binaries with no attribution requirement in the binary itself (attribution in LICENSE file is sufficient).

## Design Tokens Reference

```
// Backgrounds
bg_primary:     #FFFFFF
bg_secondary:   #F8F9FA
bg_panel:       #F1F3F5
bg_input:       #E9ECEF
bg_card:        #FFFFFF (with shadow)

// Accent
accent:         #364FC7 (indigo)
accent_light:   #EDF2FF (user bubble bg)
accent_hover:   #F0F4FF

// Text
text_primary:   #212529
text_secondary: #868E96
text_accent:    #364FC7

// Entity highlights
entity_character: #364FC7 (indigo underline)
entity_location:  #2B8A3E (green underline)
entity_object:    #E67700 (amber underline)

// Card shadow
shadow_color:   rgba(0,0,0,0.06)
shadow_offset:  0px 2px
shadow_blur:    8px

// Corner radii
radius_card:    12px
radius_button:  8px
radius_input:   8px
radius_bubble:  12px

// Spacing
margin_card:    16px inner, 4px outer
margin_panel:   12px
gap_items:      8px
sidebar_row_h:  36px
top_bar_h:      52px
```
