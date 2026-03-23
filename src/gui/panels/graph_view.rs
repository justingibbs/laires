use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Stroke, Vec2};

use crate::gui::state::GuiState;
use crate::gui::theme::LairesTheme;
use crate::gui::ProjectSnapshot;

const NODE_RADIUS: f32 = 20.0;
const NODE_BORDER: f32 = 2.0;
const REPULSION: f32 = 8000.0;
const ATTRACTION: f32 = 0.01;
const DAMPING: f32 = 0.85;
const REST_LENGTH: f32 = 150.0;
const EDGE_WIDTH: f32 = 1.5;
const EDGE_COLOR_ALPHA: u8 = 100;
const HOVER_GLOW_EXTRA: f32 = 8.0;
const HOVER_GLOW_ALPHA: u8 = 40;

/// Per-node layout state for force-directed positioning.
#[derive(Clone)]
struct NodeLayout {
    pos: Pos2,
    vel: Vec2,
    label: String,
    node_type: String,
    node_id: String,
}

/// Cached graph layout that persists across frames.
pub struct GraphLayoutState {
    nodes: Vec<NodeLayout>,
    /// Maps node ID → index into `nodes` vec.
    id_to_idx: HashMap<String, usize>,
    /// Edges as (source_idx, target_idx, label).
    edges: Vec<(usize, usize, String)>,
    /// Camera offset (for panning).
    pan: Vec2,
    /// Zoom level.
    zoom: f32,
    /// Whether layout is initialized.
    initialized: bool,
    /// Number of simulation steps completed.
    steps: usize,
    /// Hovered node index (for glow effect).
    hovered_node: Option<usize>,
}

impl Default for GraphLayoutState {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            id_to_idx: HashMap::new(),
            edges: Vec::new(),
            pan: Vec2::ZERO,
            zoom: 1.0,
            initialized: false,
            steps: 0,
            hovered_node: None,
        }
    }
}

impl GraphLayoutState {
    /// Build layout from a snapshot's graph data.
    pub fn build_from_snapshot(&mut self, snapshot: &ProjectSnapshot) {
        self.nodes.clear();
        self.id_to_idx.clear();
        self.edges.clear();
        self.steps = 0;
        self.hovered_node = None;

        let n = snapshot.graph_nodes.len();
        for (i, gn) in snapshot.graph_nodes.iter().enumerate() {
            let angle = (i as f32 / n.max(1) as f32) * std::f32::consts::TAU;
            let radius = 200.0;
            let pos = Pos2::new(angle.cos() * radius, angle.sin() * radius);

            self.id_to_idx.insert(gn.id.clone(), self.nodes.len());
            self.nodes.push(NodeLayout {
                pos,
                vel: Vec2::ZERO,
                label: gn.label.clone(),
                node_type: gn.node_type.clone(),
                node_id: gn.id.clone(),
            });
        }

        for ge in &snapshot.graph_edges {
            if let (Some(&src), Some(&dst)) =
                (self.id_to_idx.get(&ge.source), self.id_to_idx.get(&ge.target))
            {
                self.edges.push((src, dst, ge.label.clone()));
            }
        }

        self.initialized = true;
    }

    /// Run one step of force-directed simulation.
    fn step(&mut self) {
        let n = self.nodes.len();
        if n == 0 {
            return;
        }

        let mut forces = vec![Vec2::ZERO; n];

        // Repulsion between all node pairs
        for i in 0..n {
            for j in (i + 1)..n {
                let delta = self.nodes[i].pos - self.nodes[j].pos;
                let dist = delta.length().max(1.0);
                let force = delta.normalized() * (REPULSION / (dist * dist));
                forces[i] += force;
                forces[j] -= force;
            }
        }

        // Attraction along edges
        for &(src, dst, _) in &self.edges {
            let delta = self.nodes[dst].pos - self.nodes[src].pos;
            let dist = delta.length().max(1.0);
            let force = delta.normalized() * ATTRACTION * (dist - REST_LENGTH);
            forces[src] += force;
            forces[dst] -= force;
        }

        // Apply forces
        for (i, node) in self.nodes.iter_mut().enumerate() {
            node.vel = (node.vel + forces[i]) * DAMPING;
            let max_vel = 10.0;
            if node.vel.length() > max_vel {
                node.vel = node.vel.normalized() * max_vel;
            }
            node.pos += node.vel;
        }

        self.steps += 1;
    }
}

/// Full node detail for the inspector panel.
#[derive(Clone)]
pub enum NodeDetail {
    Character {
        name: String,
        aliases: Vec<String>,
        description: Option<String>,
    },
    Objective {
        character_id: String,
        scope: String,
        description: String,
        evidence: Vec<String>,
        confidence: f64,
        status: String,
    },
    Scene {
        title: Option<String>,
        summary: String,
        characters_present: Vec<String>,
        location: Option<String>,
        time: Option<String>,
        file_path: String,
    },
    Conflict {
        description: String,
        objectives: Vec<String>,
    },
}

/// Data extracted from NarrativeGraph for rendering and inspection.
pub struct SnapshotNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub detail: NodeDetail,
}

/// Backwards-compatible alias used by dashboard and other panels.
pub type GraphNodeInfo = SnapshotNode;

pub struct GraphEdgeInfo {
    pub source: String,
    pub target: String,
    pub label: String,
}

pub fn render(
    ui: &mut egui::Ui,
    state: &mut GuiState,
    snapshot: &Option<ProjectSnapshot>,
    layout: &mut GraphLayoutState,
    theme: &LairesTheme,
) {
    ui.vertical(|ui| {
        let Some(snap) = snapshot else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No project loaded.")
                        .color(theme.text_secondary)
                        .italics(),
                );
            });
            return;
        };

        if snap.graph_nodes.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Graph is empty. Run Scan to analyze your story.")
                        .color(theme.text_secondary)
                        .italics(),
                );
            });
            return;
        }

        // Build/rebuild layout if needed
        if !layout.initialized || state.graph_needs_rebuild {
            layout.build_from_snapshot(snap);
            state.graph_needs_rebuild = false;
        }

        // Run simulation steps
        if layout.steps < 200 {
            let steps_per_frame = if layout.steps < 50 { 5 } else { 1 };
            for _ in 0..steps_per_frame {
                layout.step();
            }
            ui.ctx().request_repaint();
        }

        // Allocate drawing area
        let available = ui.available_size();
        let (response, painter) =
            ui.allocate_painter(available, egui::Sense::click_and_drag());
        let rect = response.rect;
        let center = rect.center();

        // Background fill (slightly lighter than panel)
        painter.rect_filled(rect, CornerRadius::same(0), theme.bg_secondary);

        // Handle pan (drag)
        if response.dragged() {
            layout.pan += response.drag_delta();
        }

        // Handle zoom (scroll)
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_delta != 0.0 {
            let factor = 1.0 + scroll_delta * 0.002;
            layout.zoom = (layout.zoom * factor).clamp(0.1, 5.0);
        }

        // Transform: world position -> screen position
        let world_to_screen = |pos: Pos2| -> Pos2 {
            let scaled = Pos2::new(pos.x * layout.zoom, pos.y * layout.zoom);
            Pos2::new(
                center.x + scaled.x + layout.pan.x,
                center.y + scaled.y + layout.pan.y,
            )
        };

        // Edge color (softer)
        let edge_color = Color32::from_rgba_premultiplied(
            0xCE, 0xD4, 0xDA, EDGE_COLOR_ALPHA,
        );

        // === Draw edges as quadratic bezier curves ===
        for &(src, dst, ref _label) in &layout.edges {
            let p1 = world_to_screen(layout.nodes[src].pos);
            let p2 = world_to_screen(layout.nodes[dst].pos);

            if !rect.contains(p1) && !rect.contains(p2) {
                continue;
            }

            // Compute a control point offset perpendicular to the edge
            let mid = Pos2::new((p1.x + p2.x) / 2.0, (p1.y + p2.y) / 2.0);
            let dx = p2.x - p1.x;
            let dy = p2.y - p1.y;
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            // Perpendicular offset scaled by edge length
            let offset_amount = (len * 0.12).clamp(8.0, 40.0);
            let ctrl = Pos2::new(
                mid.x + (-dy / len) * offset_amount,
                mid.y + (dx / len) * offset_amount,
            );

            // Draw quadratic bezier as line segments
            let segments = 16;
            let stroke = Stroke::new(EDGE_WIDTH, edge_color);
            for s in 0..segments {
                let t0 = s as f32 / segments as f32;
                let t1 = (s + 1) as f32 / segments as f32;
                let a = quadratic_bezier(p1, ctrl, p2, t0);
                let b = quadratic_bezier(p1, ctrl, p2, t1);
                painter.line_segment([a, b], stroke);
            }
        }

        // === Hover detection (before drawing nodes so we know which to glow) ===
        let pointer_pos = response.hover_pos();
        layout.hovered_node = None;
        if let Some(pp) = pointer_pos {
            for (i, node) in layout.nodes.iter().enumerate() {
                let screen_pos = world_to_screen(node.pos);
                let r = NODE_RADIUS * layout.zoom;
                if screen_pos.distance(pp) <= r + 4.0 {
                    layout.hovered_node = Some(i);
                    break;
                }
            }
        }

        // === Draw nodes ===
        let mut clicked_node = None;
        for (i, node) in layout.nodes.iter().enumerate() {
            let screen_pos = world_to_screen(node.pos);
            if !rect.contains(screen_pos) {
                continue;
            }

            let color = node_color(&node.node_type, theme);
            let is_selected = state.selected_node_id.as_deref() == Some(&node.node_id);
            let is_hovered = layout.hovered_node == Some(i);
            let r = NODE_RADIUS * layout.zoom;

            // Hover glow — soft larger circle behind
            if is_hovered && !is_selected {
                let glow_color = Color32::from_rgba_premultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    HOVER_GLOW_ALPHA,
                );
                painter.circle_filled(screen_pos, r + HOVER_GLOW_EXTRA * layout.zoom, glow_color);
            }

            // Selection ring
            if is_selected {
                painter.circle_filled(screen_pos, r + 4.0, theme.accent);
            }

            // Node fill
            painter.circle_filled(screen_pos, r, color);

            // White border
            painter.circle_stroke(
                screen_pos,
                r,
                Stroke::new(NODE_BORDER, Color32::WHITE),
            );

            // Label — try to fit short labels inside the node, longer ones below
            let font_size = (11.0 * layout.zoom).clamp(7.0, 16.0);
            let font = egui::FontId::proportional(font_size);
            if node.label.len() <= 6 && layout.zoom >= 0.6 {
                // Inside the node
                painter.text(
                    screen_pos,
                    egui::Align2::CENTER_CENTER,
                    &node.label,
                    font,
                    Color32::WHITE,
                );
            } else {
                // Below the node
                let label_pos = Pos2::new(screen_pos.x, screen_pos.y + r + 6.0);
                painter.text(
                    label_pos,
                    egui::Align2::CENTER_TOP,
                    &node.label,
                    font,
                    theme.text_primary,
                );
            }

            // Click detection
            let node_rect = Rect::from_center_size(screen_pos, Vec2::splat(r * 2.0));
            if response.clicked() {
                if let Some(click_pos) = response.interact_pointer_pos() {
                    if node_rect.contains(click_pos) {
                        clicked_node = Some(i);
                    }
                }
            }
        }

        // Handle node click
        if let Some(idx) = clicked_node {
            state.selected_node_id = Some(layout.nodes[idx].node_id.clone());
        }

        // === Legend overlay (bottom-left) ===
        render_legend(&painter, rect, theme);

        // Deselect when clicking empty space (no node hit)
        if response.clicked() && clicked_node.is_none() {
            state.selected_node_id = None;
        }
    });
}

/// Evaluate a quadratic bezier at parameter t.
fn quadratic_bezier(p0: Pos2, p1: Pos2, p2: Pos2, t: f32) -> Pos2 {
    let inv = 1.0 - t;
    Pos2::new(
        inv * inv * p0.x + 2.0 * inv * t * p1.x + t * t * p2.x,
        inv * inv * p0.y + 2.0 * inv * t * p1.y + t * t * p2.y,
    )
}

/// Renders the graph legend in the bottom-left corner.
fn render_legend(painter: &egui::Painter, rect: Rect, theme: &LairesTheme) {
    let legend_w = 180.0;
    let legend_h = 80.0;
    let margin = 12.0;
    let legend_rect = Rect::from_min_size(
        Pos2::new(rect.min.x + margin, rect.max.y - legend_h - margin),
        Vec2::new(legend_w, legend_h),
    );

    // Semi-transparent background
    painter.rect_filled(
        legend_rect,
        CornerRadius::same(8),
        Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 220),
    );
    painter.rect_stroke(
        legend_rect,
        CornerRadius::same(8),
        Stroke::new(1.0, theme.border),
        egui::StrokeKind::Outside,
    );

    // Title
    painter.text(
        Pos2::new(legend_rect.min.x + 10.0, legend_rect.min.y + 10.0),
        egui::Align2::LEFT_TOP,
        "Graph Legend",
        egui::FontId::proportional(10.0),
        theme.text_secondary,
    );

    // Legend items — 2 columns
    let items = [
        ("Character", theme.character_color),
        ("Objective", theme.objective_color),
        ("Scene", theme.scene_color),
        ("Conflict", theme.conflict_color),
    ];

    let col_w = legend_w / 2.0;
    let start_y = legend_rect.min.y + 28.0;
    let row_h = 20.0;

    for (i, (label, color)) in items.iter().enumerate() {
        let col = i % 2;
        let row = i / 2;
        let x = legend_rect.min.x + 10.0 + col as f32 * col_w;
        let y = start_y + row as f32 * row_h;

        // Dot
        painter.circle_filled(Pos2::new(x + 5.0, y + 5.0), 4.0, *color);

        // Label
        painter.text(
            Pos2::new(x + 14.0, y + 5.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(10.0),
            theme.text_primary,
        );
    }
}


fn node_color(node_type: &str, theme: &LairesTheme) -> Color32 {
    match node_type {
        "character" => theme.character_color,
        "objective" => theme.objective_color,
        "scene" => theme.scene_color,
        "conflict" => theme.conflict_color,
        _ => theme.text_secondary,
    }
}
