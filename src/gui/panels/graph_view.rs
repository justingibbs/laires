use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use petgraph::visit::EdgeRef;

use crate::concepts::narrative_graph::{GraphEdge, GraphNode};
use crate::gui::state::GuiState;
use crate::gui::theme::LairesTheme;
use crate::gui::ProjectSnapshot;

const NODE_RADIUS: f32 = 20.0;
const REPULSION: f32 = 8000.0;
const ATTRACTION: f32 = 0.01;
const DAMPING: f32 = 0.85;
const REST_LENGTH: f32 = 150.0;

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

        // Add nodes with initial circular placement
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

        // Add edges
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
            // Clamp velocity
            let max_vel = 10.0;
            if node.vel.length() > max_vel {
                node.vel = node.vel.normalized() * max_vel;
            }
            node.pos += node.vel;
        }

        self.steps += 1;
    }
}

/// Data extracted from NarrativeGraph for rendering.
pub struct GraphNodeInfo {
    pub id: String,
    pub label: String,
    pub node_type: String,
}

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
                    RichText::new("Graph is empty. Run `laires scan` to analyze your story.")
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

        // Run simulation steps (more at start, fewer once settled)
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

        // Draw edges
        for &(src, dst, ref _label) in &layout.edges {
            let p1 = world_to_screen(layout.nodes[src].pos);
            let p2 = world_to_screen(layout.nodes[dst].pos);
            if rect.contains(p1) || rect.contains(p2) {
                painter.line_segment([p1, p2], Stroke::new(1.0, theme.border));
            }
        }

        // Draw nodes
        let mut clicked_node = None;
        for (i, node) in layout.nodes.iter().enumerate() {
            let screen_pos = world_to_screen(node.pos);
            if !rect.contains(screen_pos) {
                continue;
            }

            let color = node_color(&node.node_type, theme);
            let is_selected = state.selected_node_id.as_deref() == Some(&node.node_id);
            let r = NODE_RADIUS * layout.zoom;

            // Node circle
            if is_selected {
                painter.circle_filled(screen_pos, r + 3.0, theme.accent);
            }
            painter.circle_filled(screen_pos, r, color);

            // Label
            let font = egui::FontId::proportional(11.0 * layout.zoom.max(0.5));
            let label_pos = Pos2::new(screen_pos.x, screen_pos.y + r + 8.0);
            painter.text(
                label_pos,
                egui::Align2::CENTER_TOP,
                &node.label,
                font,
                theme.text_primary,
            );

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

        // Node info panel (when a node is selected)
        if let Some(selected_id) = &state.selected_node_id {
            if let Some(info) = snap
                .graph_nodes
                .iter()
                .find(|n| n.id == *selected_id)
            {
                // Draw info box in bottom-left of the graph area
                let info_rect = Rect::from_min_size(
                    Pos2::new(rect.min.x + 8.0, rect.max.y - 80.0),
                    Vec2::new(250.0, 70.0),
                );
                painter.rect_filled(info_rect, 4.0, Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 230));
                painter.rect_stroke(info_rect, 4.0, Stroke::new(1.0, theme.border), egui::StrokeKind::Outside);

                let text_pos = Pos2::new(info_rect.min.x + 8.0, info_rect.min.y + 8.0);
                painter.text(
                    text_pos,
                    egui::Align2::LEFT_TOP,
                    format!("{} ({})", info.label, info.node_type),
                    egui::FontId::proportional(13.0),
                    theme.text_primary,
                );
            }
        }
    });
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
