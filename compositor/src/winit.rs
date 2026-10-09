use std::time::Duration;

use smithay::{
    backend::{
        allocator::{Fourcc, dmabuf::Dmabuf},
        egl::EGLDevice,
        renderer::{
            ExportMem, ImportDma, damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement, gles::GlesRenderer,
        },
        winit::{self, WinitEvent},
    },
    delegate_dmabuf,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Transform},
    wayland::dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
};

use crate::EdexComp;

impl DmabufHandler for EdexComp {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        let imported = self
            .renderer()
            .is_some_and(|renderer| renderer.import_dmabuf(&dmabuf, None).is_ok());
        if imported {
            let _ = notifier.successful::<EdexComp>();
        } else {
            notifier.failed();
        }
    }
}
delegate_dmabuf!(EdexComp);

pub fn init_winit(
    event_loop: &mut EventLoop<'static, EdexComp>,
    state: &mut EdexComp,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &state.display_handle.clone();

    let (mut backend, winit) = winit::init::<GlesRenderer>()?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "khadi".into(),
            model: "Winit".into(),
        },
    );
    let _global = output.create_global::<EdexComp>(display_handle);
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    // Let clients hand over GPU buffers. Mesa clients need the feedback (v4) form to find
    // the render device; without it they fall back to software rendering over wl_shm.
    let formats = backend.renderer().dmabuf_formats();
    let render_node = EGLDevice::device_for_display(backend.renderer().egl_context().display())
        .and_then(|device| device.try_get_render_node());
    let feedback = match render_node {
        Ok(Some(node)) => DmabufFeedbackBuilder::new(node.dev_id(), formats.clone()).build().ok(),
        _ => None,
    };
    state.dmabuf_global = Some(match &feedback {
        Some(feedback) => state
            .dmabuf_state
            .create_global_with_default_feedback::<EdexComp>(display_handle, feedback),
        None => {
            tracing::warn!("no render node found, clients will render in software");
            state.dmabuf_state.create_global::<EdexComp>(display_handle, formats)
        }
    });

    state.backend = Some(backend);
    state.set_screens(screens_of(mode.size.w, mode.size.h, state.split));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    event_loop.handle().insert_source(winit, move |event, _, data| {
        let state = data;

        match event {
            WinitEvent::Resized { size, .. } => {
                output.change_current_state(
                    Some(Mode {
                        size,
                        refresh: 60_000,
                    }),
                    None,
                    None,
                    None,
                );
                state.set_screens(screens_of(size.w, size.h, state.split));
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                if !state.before_frame() {
                    return;
                }

                let capture = state.screenshot.is_some() && state.start_time.elapsed() > Duration::from_secs(5);
                let backend = state.backend.as_mut().unwrap();
                let size = backend.window_size();
                let damage = Rectangle::from_size(size);

                {
                    let (renderer, mut framebuffer) = backend.bind().unwrap();
                    smithay::desktop::space::render_output::<
                        _,
                        WaylandSurfaceRenderElement<GlesRenderer>,
                        _,
                        _,
                    >(
                        &output,
                        renderer,
                        &mut framebuffer,
                        1.0,
                        0,
                        [&state.space],
                        &[],
                        &mut damage_tracker,
                        [0.0, 0.0, 0.0, 1.0],
                    )
                    .unwrap();

                    if capture {
                        let region = Rectangle::from_size((size.w, size.h).into());
                        let pixels = renderer
                            .copy_framebuffer(&framebuffer, region, Fourcc::Abgr8888)
                            .and_then(|mapping| renderer.map_texture(&mapping).map(|bytes| bytes.to_vec()));
                        match pixels {
                            Ok(rgba) => write_ppm(state.screenshot.as_deref().unwrap(), size.w, size.h, &rgba),
                            Err(e) => tracing::error!("screenshot failed: {e}"),
                        }
                    }
                }
                if capture {
                    // The capture is the whole point of this run; leave without presenting.
                    state.loop_signal.stop();
                    return;
                }
                if let Err(e) = backend.submit(Some(&[damage])) {
                    tracing::error!("cannot present frame: {e}");
                }

                state.after_frame();
                let _ = state.display_handle.flush_clients();

                // Ask for redraw to schedule new frame.
                state.backend.as_mut().unwrap().window().request_redraw();
            }
            WinitEvent::CloseRequested => {
                state.loop_signal.stop();
            }
            _ => (),
        };
    })?;

    Ok(())
}

/// The displays the window stands for: itself, or its two halves when testing.
fn screens_of(width: i32, height: i32, split: bool) -> Vec<Rectangle<i32, smithay::utils::Logical>> {
    if !split {
        return vec![Rectangle::from_size((width, height).into())];
    }
    let half = width / 2;
    vec![
        Rectangle::from_size((half, height).into()),
        Rectangle::new((half, 0).into(), (width - half, height).into()),
    ]
}

/// Writes RGBA pixels as a binary PPM. GL framebuffers are bottom-up, so rows are flipped.
fn write_ppm(path: &std::path::Path, width: i32, height: i32, rgba: &[u8]) {
    let (width, height) = (width as usize, height as usize);
    let mut data = format!("P6\n{width} {height}\n255\n").into_bytes();
    for row in (0..height).rev() {
        let line = &rgba[row * width * 4..(row + 1) * width * 4];
        for pixel in line.chunks_exact(4) {
            data.extend_from_slice(&pixel[..3]);
        }
    }
    if let Err(e) = std::fs::write(path, data) {
        tracing::error!("cannot write {}: {e}", path.display());
    }
}
