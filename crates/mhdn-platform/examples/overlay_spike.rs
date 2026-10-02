//! Translucent click-through panel. Proves ADR-0005 on macOS.
//!
//! ```text
//! cargo run -p mhdn-platform --example overlay-spike
//! ```
//!
//! The panel does not take clicks. Stop it with Ctrl-C in this terminal.

fn main() {
    #[cfg(target_os = "macos")]
    {
        if let Err(err) = run() {
            eprintln!("overlay spike: {err}");
            std::process::exit(1);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!(
            "overlay spike: macOS only (docs/adr/0005-winit-window-with-appkit-overlay-flags.md)"
        );
    }
}

#[cfg(target_os = "macos")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;

    use mhdn_platform::{apply_click_through, PlatformError};
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::window::{Window, WindowAttributes, WindowId};

    struct Spike {
        window: Option<Arc<Window>>,
    }

    impl ApplicationHandler for Spike {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attributes = WindowAttributes::default()
                .with_title("mhdn overlay spike")
                .with_inner_size(LogicalSize::new(480.0, 270.0))
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(false);
            let window = match event_loop.create_window(attributes) {
                Ok(window) => Arc::new(window),
                Err(err) => {
                    eprintln!("overlay spike: create window: {err}");
                    event_loop.exit();
                    return;
                }
            };
            if let Err(err) = install(&window) {
                eprintln!("overlay spike: {err}");
                event_loop.exit();
                return;
            }
            eprintln!("overlay spike: click-through panel is up. Ctrl-C to quit.");
            self.window = Some(window);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: winit::event::WindowEvent,
        ) {
            if let winit::event::WindowEvent::CloseRequested = event {
                event_loop.exit();
            }
        }
    }

    fn install(window: &Window) -> Result<(), PlatformError> {
        apply_click_through(window, true)?;
        mhdn_platform::tint_spike(window)?;
        Ok(())
    }

    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut Spike { window: None })?;
    Ok(())
}
