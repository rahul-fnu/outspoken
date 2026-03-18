use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub trait HotkeyListener {
    fn start(&self) -> Result<(), String>;
    fn stop(&self);
}

#[cfg(target_os = "macos")]
pub use macos::MacHotkeyListener;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, EventField,
    };
    use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
    use std::time::Duration;

    const F5_KEYCODE: i64 = 96;

    pub struct MacHotkeyListener {
        callback: Arc<Mutex<Box<dyn Fn() + Send>>>,
        running: Arc<AtomicBool>,
    }

    impl MacHotkeyListener {
        pub fn new<F: Fn() + Send + 'static>(callback: F) -> Self {
            Self {
                callback: Arc::new(Mutex::new(Box::new(callback))),
                running: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl HotkeyListener for MacHotkeyListener {
        fn start(&self) -> Result<(), String> {
            self.running.store(true, Ordering::SeqCst);
            let callback = self.callback.clone();
            let running = self.running.clone();

            std::thread::spawn(move || {
                eprintln!("Starting hotkey listener thread...");
                let tap = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![CGEventType::KeyDown],
                    move |_proxy, _event_type, event: &CGEvent| {
                        let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                        if keycode == F5_KEYCODE {
                            eprintln!("Hotkey detected! (F5)");
                            if let Ok(cb) = callback.lock() {
                                cb();
                            }
                            // Swallow the event
                            return None;
                        }
                        // Pass all other events through
                        Some(event.clone())
                    },
                );

                let tap = match tap {
                    Ok(tap) => tap,
                    Err(_) => {
                        eprintln!(
                            "Error: Could not create CGEvent tap. Accessibility permission is required.\n\
                             \n\
                             To grant permission:\n\
                             1. Open System Settings > Privacy & Security > Accessibility\n\
                             2. Add this application to the list and enable it\n\
                             \n\
                             Or run: tccutil reset Accessibility\n\
                             Then re-launch the application and approve the prompt.\n\
                             \n\
                             Direct link: x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                        );
                        return;
                    }
                };

                eprintln!("CGEvent tap created successfully. Listening for Cmd+Shift+D...");
                unsafe {
                    let loop_source = tap.mach_port.create_runloop_source(0)
                        .expect("Failed to create run loop source");
                    let run_loop = CFRunLoop::get_current();
                    run_loop.add_source(&loop_source, kCFRunLoopCommonModes);
                    tap.enable();

                    while running.load(Ordering::SeqCst) {
                        CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(500), false);
                    }
                }
            });

            Ok(())
        }

        fn stop(&self) {
            self.running.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MockHotkeyListener {
    callback: Arc<Mutex<Option<Box<dyn Fn() + Send>>>>,
    running: Arc<AtomicBool>,
}

#[cfg(not(target_os = "macos"))]
impl MockHotkeyListener {
    pub fn new<F: Fn() + Send + 'static>(callback: F) -> Self {
        Self {
            callback: Arc::new(Mutex::new(Some(Box::new(callback)))),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn simulate_press(&self) {
        if let Ok(cb) = self.callback.lock() {
            if let Some(ref f) = *cb {
                f();
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl HotkeyListener for MockHotkeyListener {
    fn start(&self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn test_mock_callback_fires_on_simulate_press() {
        let counter = Arc::new(AtomicBool::new(false));
        let counter_clone = counter.clone();

        let listener = MockHotkeyListener::new(move || {
            counter_clone.store(true, Ordering::SeqCst);
        });
        listener.start().unwrap();
        listener.simulate_press();

        assert!(counter.load(Ordering::SeqCst));
        listener.stop();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn test_toggle_state_start_stop() {
        use std::sync::atomic::AtomicU8;

        const IDLE: u8 = 0;
        const RECORDING: u8 = 1;

        let state = Arc::new(AtomicU8::new(IDLE));
        let state_clone = state.clone();

        let listener = MockHotkeyListener::new(move || {
            let current = state_clone.load(Ordering::SeqCst);
            if current == IDLE {
                state_clone.store(RECORDING, Ordering::SeqCst);
            } else {
                state_clone.store(IDLE, Ordering::SeqCst);
            }
        });
        listener.start().unwrap();

        // Press 1: start recording
        listener.simulate_press();
        assert_eq!(state.load(Ordering::SeqCst), RECORDING);

        // Press 2: stop recording
        listener.simulate_press();
        assert_eq!(state.load(Ordering::SeqCst), IDLE);

        listener.stop();
    }
}
