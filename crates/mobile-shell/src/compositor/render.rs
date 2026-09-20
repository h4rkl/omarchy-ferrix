//! Renderer-agnostic scene assembly: the same element list feeds the pixman
//! software renderer (guest DRM) and the GLES renderer (nested host window).

use smithay::{
    backend::renderer::{
        Color32F, ImportAll, ImportMem, Renderer,
        element::{
            AsRenderElements, Kind, surface::WaylandSurfaceRenderElement,
            surface::render_elements_from_surface_tree,
        },
    },
    desktop::layer_map_for_output,
    input::pointer::CursorImageStatus,
    utils::{IsAlive, Scale},
    wayland::shell::wlr_layer::Layer,
};

use super::{Shell, cursor_hotspot};

/// Tokyo Night background (#1a1b26): visible before any client maps, which is
/// how a framebuffer capture proves this compositor is the one scanning out.
pub const CLEAR_COLOR: Color32F = Color32F::new(0.102, 0.106, 0.149, 1.0);

smithay::backend::renderer::element::render_elements! {
    pub ShellRenderElement<R> where R: ImportAll + ImportMem;
    Surface = WaylandSurfaceRenderElement<R>,
}

impl<R: Renderer> std::fmt::Debug for ShellRenderElement<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(e) => f.debug_tuple("Surface").field(e).finish(),
            Self::_GenericCatcher(e) => f.debug_tuple("_GenericCatcher").field(e).finish(),
        }
    }
}

/// Collect render elements front-to-back: client cursor, overlay/top layers,
/// the fullscreen toplevel (and its popups), then bottom/background layers.
pub fn collect_elements<R>(shell: &Shell, renderer: &mut R) -> Vec<ShellRenderElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + 'static,
{
    let scale = Scale::from(shell.output.current_scale().fractional_scale());
    let mut elements: Vec<ShellRenderElement<R>> = Vec::new();

    if let CursorImageStatus::Surface(surface) = &shell.cursor_status
        && surface.alive()
    {
        let hotspot = cursor_hotspot(surface);
        let location = (shell.pointer_location - hotspot.to_f64())
            .to_physical(scale)
            .to_i32_round();
        elements.extend(render_elements_from_surface_tree(
            renderer,
            surface,
            location,
            scale,
            1.0,
            Kind::Cursor,
        ));
    }

    let map = layer_map_for_output(&shell.output);
    let push_layers =
        |layers: &[Layer], elements: &mut Vec<ShellRenderElement<R>>, renderer: &mut R| {
            for layer_kind in layers {
                for layer in map.layers_on(*layer_kind).rev() {
                    let location = map
                        .layer_geometry(layer)
                        .map(|g| g.loc)
                        .unwrap_or_default()
                        .to_physical_precise_round(scale);
                    elements.extend(
                        layer
                            .render_elements::<WaylandSurfaceRenderElement<R>>(
                                renderer, location, scale, 1.0,
                            )
                            .into_iter()
                            .map(ShellRenderElement::Surface),
                    );
                }
            }
        };
    push_layers(&[Layer::Overlay, Layer::Top], &mut elements, renderer);

    if let Some(window) = shell.windows.focused() {
        // The toplevel fills the output; honour its xdg window geometry offset.
        let location = (smithay::utils::Point::from((0, 0)) - window.geometry().loc)
            .to_physical_precise_round(scale);
        elements.extend(
            window
                .render_elements::<WaylandSurfaceRenderElement<R>>(renderer, location, scale, 1.0)
                .into_iter()
                .map(ShellRenderElement::Surface),
        );
    }

    push_layers(&[Layer::Bottom, Layer::Background], &mut elements, renderer);
    elements
}
