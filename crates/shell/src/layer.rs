//! wlr-layer-shell client: `PanelWindow` geometry as layer-surface state, and the protocol objects.
//!
//! smithay-client-toolkit binds `zwlr_layer_shell_v1` only up to version 4; version 5 adds
//! `zwlr_layer_surface_v1.set_exclusive_edge`, without which a corner-anchored panel (a panel
//! shorter than its edge, aligned to one end) cannot reserve space: the compositor cannot deduce
//! which of the two anchored edges the exclusive zone applies to.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use smithay_client_toolkit::compositor::Surface;
use wayland_client::{
    Dispatch, Proxy, QueueHandle,
    globals::{BindError, GlobalList},
    protocol::{wl_output, wl_surface},
};
use wayland_protocols::xdg::shell::client::xdg_popup::XdgPopup;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1,
};

bits::bit_flags! {
/// `zwlr_layer_surface_v1.anchor` bits, also the order of `PanelWindow.anchors` in [`Geometry`].
pub const TOP: u32 = 1;
pub const BOTTOM: u32 = 2;
pub const LEFT: u32 = 4;
pub const RIGHT: u32 = 8;
}

/// A Quickshell `Edges` value (`Qt::Edge`: top 1, left 2, right 4, bottom 8) as anchor bits.
pub fn from_edges(edges: u32) -> u32 {
    [(1, TOP), (2, LEFT), (4, RIGHT), (8, BOTTOM)]
        .into_iter()
        .filter(|(e, _)| edges & e != 0)
        .fold(0, |a, (_, b)| a | b)
}

/// `ExclusionMode` (Quickshell.\_Window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exclusion {
    Normal,
    Ignore,
    Auto,
}

impl Exclusion {
    pub fn from_qml(v: Option<f64>) -> Self {
        match v {
            Some(0.0) => Exclusion::Normal,
            Some(1.0) => Exclusion::Ignore,
            // Quickshell's default.
            _ => Exclusion::Auto,
        }
    }
}

/// A `PanelWindow`'s placement properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    /// `anchors` as [`TOP`]/[`BOTTOM`]/[`LEFT`]/[`RIGHT`] bits.
    pub anchor: u32,
    /// `margins` (top, right, bottom, left).
    pub margins: (i32, i32, i32, i32),
    /// `implicitWidth`, `implicitHeight`.
    pub size: (u32, u32),
    pub exclusion: Exclusion,
    /// `exclusiveZone` (used in [`Exclusion::Normal`]).
    pub zone: i32,
    /// `WlrLayershell.exclusiveEdge` (an `Edges` value, see [`from_edges`]) as an anchor bit, 0
    /// when unset.
    pub edge: u32,
}

/// The double-buffered layer-surface state for a [`Panel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Geometry {
    pub anchor: u32,
    /// `set_size`: 0 on an axis anchored to both of its edges (the compositor fills it).
    pub size: (u32, u32),
    pub margins: (i32, i32, i32, i32),
    pub zone: i32,
    /// `set_exclusive_edge` (0: deduced by the compositor from the anchors).
    pub edge: u32,
}

/// The edge an exclusive zone applies to when the anchors make it unambiguous: anchored to one
/// edge, or to one edge and both of its neighbours (Quickshell `Anchors::exclusionEdge`, the
/// protocol's deduction rule).
pub fn deduced_edge(anchor: u32) -> u32 {
    let h = (anchor & LEFT != 0) != (anchor & RIGHT != 0);
    let v = (anchor & TOP != 0) != (anchor & BOTTOM != 0);
    match (h, v) {
        (false, true) => anchor & (TOP | BOTTOM),
        (true, false) => anchor & (LEFT | RIGHT),
        _ => 0,
    }
}

impl Geometry {
    /// The state for a window on a screen of factor `f` ([`crate::hidpi`]): Qt sends the
    /// compositor the size, the margins and the zone times it (QHighDpi::toNativePixels).
    pub fn scaled(mut self, f: f64) -> Geometry {
        if f == 1.0 {
            return self;
        }
        let px = |v: i32| (v as f64 * f).round() as i32;
        self.size = (px(self.size.0 as i32) as u32, px(self.size.1 as i32) as u32);
        let (t, r, b, l) = self.margins;
        self.margins = (px(t), px(r), px(b), px(l));
        if self.zone > 0 {
            self.zone = px(self.zone);
        }
        self
    }
}

impl Panel {
    /// The edge the exclusive zone applies to: `exclusiveEdge` when it names one of the anchored
    /// edges (the protocol rejects any other with `invalid_exclusive_edge`), else the deduced one.
    pub fn exclusion_edge(&self) -> u32 {
        if self.edge.count_ones() == 1 && self.anchor & self.edge != 0 {
            self.edge
        } else {
            deduced_edge(self.anchor)
        }
    }

    pub fn geometry(&self) -> Geometry {
        let edge = self.exclusion_edge();
        let (t, r, b, l) = self.margins;
        let zone = match self.exclusion {
            Exclusion::Ignore => -1,
            Exclusion::Normal => self.zone,
            // Quickshell: the window's size plus the margin on the far side (the compositor
            // already adds the near one, wlr-layer-shell `set_exclusive_zone`).
            Exclusion::Auto => match edge {
                TOP => self.size.1 as i32 + b,
                BOTTOM => self.size.1 as i32 + t,
                LEFT => self.size.0 as i32 + r,
                RIGHT => self.size.0 as i32 + l,
                _ => 0,
            },
        };
        let both = |a: u32| self.anchor & a == a;
        Geometry {
            anchor: self.anchor,
            size: (
                if both(LEFT | RIGHT) { 0 } else { self.size.0 },
                if both(TOP | BOTTOM) { 0 } else { self.size.1 },
            ),
            margins: self.margins,
            zone,
            // Only a corner needs it; for the rest the compositor deduces the same edge.
            edge: if zone > 0 && deduced_edge(self.anchor) == 0 {
                edge
            } else {
                0
            },
        }
    }
}

/// Space a shown panel reserves on its output, (top, right, bottom, left) like `margins`: its
/// zone plus the margin on that edge, as compositors apply wlr-layer-shell exclusive zones.
pub fn reserved(g: &Geometry) -> (f64, f64, f64, f64) {
    let edge = if g.edge != 0 {
        g.edge
    } else {
        deduced_edge(g.anchor)
    };
    let mut r = (0.0, 0.0, 0.0, 0.0);
    if g.zone <= 0 {
        return r;
    }
    let (t, right, b, l) = g.margins;
    match edge {
        TOP => r.0 = (g.zone + t) as f64,
        RIGHT => r.1 = (g.zone + right) as f64,
        BOTTOM => r.2 = (g.zone + b) as f64,
        LEFT => r.3 = (g.zone + l) as f64,
        _ => {}
    }
    r
}

/// Where a layer surface of `size` sits in `area` (x, y, width, height on its output), as
/// wlr-layer-shell places it: against each anchored edge plus its margin, centred between
/// two opposite anchors, centred on an axis with neither.
pub fn place(
    anchor: u32,
    margins: (i32, i32, i32, i32),
    size: (u32, u32),
    area: (f64, f64, f64, f64),
) -> (f64, f64) {
    let (t, r, b, l) = (
        margins.0 as f64,
        margins.1 as f64,
        margins.2 as f64,
        margins.3 as f64,
    );
    let axis = |lo: bool, hi: bool, start: f64, len: f64, m0: f64, m1: f64, s: f64| match (lo, hi) {
        (true, false) => start + m0,
        (false, true) => start + len - m1 - s,
        (true, true) => start + m0 + ((len - m0 - m1 - s) / 2.0).max(0.0),
        (false, false) => start + (len - s) / 2.0,
    };
    (
        axis(
            anchor & LEFT != 0,
            anchor & RIGHT != 0,
            area.0,
            area.2,
            l,
            r,
            size.0 as f64,
        ),
        axis(
            anchor & TOP != 0,
            anchor & BOTTOM != 0,
            area.1,
            area.3,
            t,
            b,
            size.1 as f64,
        ),
    )
}

/// The bound `zwlr_layer_shell_v1` (version 1 to 5).
#[derive(Debug)]
pub struct LayerShell(ZwlrLayerShellV1);

impl LayerShell {
    pub fn bind<D>(globals: &GlobalList, qh: &QueueHandle<D>) -> Result<Self, BindError>
    where
        D: Dispatch<ZwlrLayerShellV1, ()> + 'static,
    {
        globals.bind(qh, 1..=5, ()).map(LayerShell)
    }

    pub fn create_layer_surface<D>(
        &self,
        qh: &QueueHandle<D>,
        surface: impl Into<Surface>,
        layer: Layer,
        namespace: String,
        output: Option<&wl_output::WlOutput>,
    ) -> LayerSurface
    where
        D: Dispatch<ZwlrLayerSurfaceV1, ()> + 'static,
    {
        let surface = surface.into();
        let layer =
            self.0
                .get_layer_surface(surface.wl_surface(), output, layer, namespace, qh, ());
        LayerSurface(Arc::new(Inner {
            layer,
            surface,
            edge: AtomicU32::new(0),
        }))
    }
}

struct Inner {
    layer: ZwlrLayerSurfaceV1,
    /// Destroyed after the role object (sctk `Surface` destroys the wl_surface on drop).
    surface: Surface,
    /// The exclusive edge last sent; a new surface has none (0).
    edge: AtomicU32,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.layer.destroy();
    }
}

/// A `zwlr_layer_surface_v1` and its wl_surface, destroyed with the last clone.
#[derive(Clone)]
pub struct LayerSurface(Arc<Inner>);

impl PartialEq for LayerSurface {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for LayerSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LayerSurface({:?})", self.0.layer.id())
    }
}

impl LayerSurface {
    pub fn is(&self, layer: &ZwlrLayerSurfaceV1) -> bool {
        self.0.layer == *layer
    }

    pub fn wl_surface(&self) -> &wl_surface::WlSurface {
        self.0.surface.wl_surface()
    }

    pub fn get_popup(&self, popup: &XdgPopup) {
        self.0.layer.get_popup(popup);
    }

    pub fn set_layer(&self, layer: Layer) {
        self.0.layer.set_layer(layer);
    }

    pub fn set_keyboard_interactivity(&self, value: KeyboardInteractivity) {
        self.0.layer.set_keyboard_interactivity(value);
    }

    /// Sends `g` in protocol order: the exclusive edge after the anchors it must belong to.
    /// Takes effect on the next [`commit`](Self::commit).
    pub fn set_geometry(&self, g: &Geometry) {
        let l = &self.0.layer;
        l.set_anchor(Anchor::from_bits_truncate(g.anchor));
        l.set_size(g.size.0, g.size.1);
        let (t, r, b, left) = g.margins;
        l.set_margin(t, r, b, left);
        l.set_exclusive_zone(g.zone);
        // Quickshell never names the edge, so nothing is sent while it stays unset; a named
        // edge is taken back with 0.
        if l.version() >= 5 && self.0.edge.swap(g.edge, Ordering::Relaxed) != g.edge {
            l.set_exclusive_edge(Anchor::from_bits_truncate(g.edge));
        }
    }

    pub fn commit(&self) {
        self.wl_surface().commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(anchor: u32, size: (u32, u32)) -> Panel {
        Panel {
            anchor,
            margins: (0, 0, 0, 0),
            size,
            exclusion: Exclusion::Normal,
            zone: 40,
            edge: 0,
        }
    }

    #[test]
    fn full_length_bar_on_each_edge() {
        for (anchor, size, want) in [
            (TOP | LEFT | RIGHT, (1920, 40), (0, 40)),
            (BOTTOM | LEFT | RIGHT, (1920, 40), (0, 40)),
            (LEFT | TOP | BOTTOM, (40, 1080), (40, 0)),
            (RIGHT | TOP | BOTTOM, (40, 1080), (40, 0)),
        ] {
            let g = panel(anchor, size).geometry();
            assert_eq!(g.size, want, "{anchor}");
            assert_eq!(g.zone, 40);
            assert_eq!(g.edge, 0, "deducible edge sent explicitly for {anchor}");
        }
    }

    #[test]
    fn corner_panel_names_its_edge() {
        let mut p = panel(TOP | LEFT, (960, 40));
        assert_eq!(
            p.geometry().edge,
            0,
            "no exclusiveEdge: nothing to reserve against"
        );
        assert_eq!(reserved(&p.geometry()), (0.0, 0.0, 0.0, 0.0));
        p.edge = TOP;
        let g = p.geometry();
        assert_eq!((g.size, g.zone, g.edge), ((960, 40), 40, TOP));
        assert_eq!(reserved(&g), (40.0, 0.0, 0.0, 0.0));
        p.edge = BOTTOM;
        assert_eq!(
            p.geometry().edge,
            0,
            "an unanchored edge is a protocol error"
        );
    }

    #[test]
    fn centred_short_panel_is_deduced() {
        // Centred: anchored to its edge only, the compositor centres it along the edge.
        let g = panel(BOTTOM, (800, 48)).geometry();
        assert_eq!((g.size, g.edge), ((800, 48), 0));
        assert_eq!(reserved(&g), (0.0, 0.0, 40.0, 0.0));
    }

    #[test]
    fn auto_zone_matches_quickshell() {
        let mut p = panel(TOP | LEFT | RIGHT, (1920, 40));
        p.exclusion = Exclusion::Auto;
        p.margins = (5, 6, 7, 8);
        assert_eq!(p.geometry().zone, 47);
        p.anchor = RIGHT | TOP | BOTTOM;
        assert_eq!(p.geometry().zone, 1920 + 8);
        p.anchor = TOP | BOTTOM | LEFT | RIGHT;
        assert_eq!(p.geometry().zone, 0, "no exclusion edge: no zone");
        p.anchor = TOP | LEFT;
        assert_eq!(p.geometry().zone, 0);
        p.edge = LEFT;
        assert_eq!(p.geometry().zone, 1920 + 6);
        p.exclusion = Exclusion::Ignore;
        assert_eq!(p.geometry().zone, -1);
    }

    #[test]
    fn reserved_includes_near_margin() {
        let mut p = panel(LEFT | TOP | BOTTOM, (40, 1080));
        p.margins = (0, 0, 0, 10);
        assert_eq!(reserved(&p.geometry()), (0.0, 0.0, 0.0, 50.0));
        p.zone = 0;
        assert_eq!(reserved(&p.geometry()), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn edges_to_anchor_bits() {
        assert_eq!(from_edges(1), TOP);
        assert_eq!(from_edges(2), LEFT);
        assert_eq!(from_edges(4), RIGHT);
        assert_eq!(from_edges(8), BOTTOM);
        assert_eq!(from_edges(0), 0);
    }

    #[test]
    fn exclusion_mode_values() {
        assert_eq!(Exclusion::from_qml(Some(0.0)), Exclusion::Normal);
        assert_eq!(Exclusion::from_qml(Some(1.0)), Exclusion::Ignore);
        assert_eq!(Exclusion::from_qml(Some(2.0)), Exclusion::Auto);
        assert_eq!(Exclusion::from_qml(None), Exclusion::Auto);
    }

    #[test]
    fn place_follows_anchors_margins_and_area() {
        let out = (0.0, 0.0, 1920.0, 1080.0);
        // A top bar across the output.
        assert_eq!(
            place(TOP | LEFT | RIGHT, (0, 0, 0, 0), (1920, 40), out),
            (0.0, 0.0)
        );
        // A right sidebar below that bar's exclusive zone, with a margin.
        let below = (0.0, 40.0, 1920.0, 1040.0);
        assert_eq!(
            place(TOP | BOTTOM | RIGHT, (0, 8, 0, 0), (440, 1040), below),
            (1472.0, 40.0)
        );
        // A bottom-left corner panel, and a centred floating one.
        assert_eq!(
            place(BOTTOM | LEFT, (0, 0, 10, 10), (200, 50), out),
            (10.0, 1020.0)
        );
        assert_eq!(place(0, (0, 0, 0, 0), (200, 100), out), (860.0, 490.0));
        // Anchored to both sides but narrower than the space: centred between the margins.
        assert_eq!(
            place(LEFT | RIGHT | TOP, (0, 0, 0, 0), (1000, 40), out),
            (460.0, 0.0)
        );
    }
}
