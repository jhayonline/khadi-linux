//! Toasts: a part of the shell's window shown above everything else.
//!
//! The shell draws its notifications into its own window, which lies beneath the
//! applications, and says where they are. That rectangle of the shell is then drawn
//! once more on top, and the pointer there belongs to the shell.

use smithay::{
    backend::renderer::{
        element::{
            Element, Id, Kind, RenderElement, UnderlyingStorage,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            utils::CropRenderElement,
        },
        gles::GlesRenderer,
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    desktop::Window,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Transform},
};

use crate::state::DesComp;

/// The cropped copy of the shell. It goes by a name of its own, so that what tracks
/// changes on screen does not take it for the shell's window beneath.
pub struct ToastElement {
    inner: CropRenderElement<WaylandSurfaceRenderElement<GlesRenderer>>,
    id: Id,
}

impl Element for ToastElement {
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn damage_since(&self, scale: Scale<f64>, commit: Option<CommitCounter>) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for ToastElement {
    fn draw(
        &self,
        frame: &mut <GlesRenderer as smithay::backend::renderer::RendererSuper>::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), <GlesRenderer as smithay::backend::renderer::RendererSuper>::Error> {
        RenderElement::<GlesRenderer>::draw(&self.inner, frame, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

impl DesComp {
    /// Where on the desktop the shell's toasts are, if it is showing any. Locked, the
    /// shell is all there is to see, and nothing needs lifting above anything.
    pub fn toast_area(&self) -> Option<Rectangle<i32, Logical>> {
        let mut area = self.toast.filter(|_| !self.locked && self.shell.is_some())?;
        area.loc += self.screens.first()?.loc;
        Some(area)
    }
}

/// The toasts as something to draw on the display whose top left corner is at
/// `origin`, above all else there. Taken from parts, since the renderer is borrowed
/// from the same state while frames are drawn.
pub fn elements(
    renderer: &mut GlesRenderer,
    shell: Option<&Window>,
    area: Option<Rectangle<i32, Logical>>,
    primary: Option<Point<i32, Logical>>,
    id: &Id,
    origin: Point<i32, Logical>,
    scale: f64,
) -> Vec<ToastElement> {
    let (Some(area), Some(shell), Some(primary)) = (area, shell, primary) else {
        return Vec::new();
    };
    let Some(surface) = shell.toplevel().map(|toplevel| toplevel.wl_surface().clone()) else {
        return Vec::new();
    };
    let crop = Rectangle::new(area.loc - origin, area.size).to_f64().to_physical(scale).to_i32_round();
    let whole: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = render_elements_from_surface_tree(
        renderer,
        &surface,
        (primary - origin).to_f64().to_physical(scale).to_i32_round(),
        scale,
        1.0,
        Kind::Unspecified,
    );
    // The shell's window is one surface; were there more, the last is the window itself.
    whole
        .into_iter()
        .next_back()
        .and_then(|element| CropRenderElement::from_element(element, scale, crop))
        .map(|inner| ToastElement { inner, id: id.clone() })
        .into_iter()
        .collect()
}
