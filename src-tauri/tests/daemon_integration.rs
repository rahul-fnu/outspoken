use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use outspoken_lib::daemon::{
    Daemon, DaemonState, FailingTranscriber, MockAudioCapture, MockHotkeyListener,
    MockStatusIndicator, MockTextInjector, MockTranscriber,
};

#[test]
fn integration_full_pipeline_mock() {
    let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
    let audio_capture = MockAudioCapture::new(vec![0.1, 0.2, 0.3, 0.4]);
    let transcriber = MockTranscriber::new("hello from outspoken");
    let (injector, injected_texts) = MockTextInjector::new();
    let (indicator, indicator_states) = MockStatusIndicator::new();
    let shutdown = Arc::new(AtomicBool::new(false));

    let daemon = Daemon::new(
        Box::new(hotkey_listener),
        Box::new(audio_capture),
        Box::new(transcriber),
        Box::new(injector),
        Box::new(indicator),
        shutdown.clone(),
    );

    let shutdown_clone = shutdown.clone();
    let handle = std::thread::spawn(move || daemon.run());

    // Simulate hotkey press: Idle -> Recording
    hotkey_tx.send(()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Simulate hotkey press: Recording -> Processing -> Idle
    hotkey_tx.send(()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Shutdown daemon
    shutdown_clone.store(true, Ordering::SeqCst);
    drop(hotkey_tx);

    let result = handle.join().unwrap();
    assert!(result.is_ok());

    // Verify the mock text injector received the transcribed text
    let texts = injected_texts.lock().unwrap();
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0], "hello from outspoken");

    // Verify state transitions: Idle -> Recording -> Processing -> Idle
    let states = indicator_states.lock().unwrap();
    assert_eq!(states.len(), 4);
    assert_eq!(states[0], DaemonState::Idle);
    assert_eq!(states[1], DaemonState::Recording);
    assert_eq!(states[2], DaemonState::Processing);
    assert_eq!(states[3], DaemonState::Idle);
}

#[test]
fn integration_transcription_error_recovers() {
    let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
    let audio_capture = MockAudioCapture::new(vec![0.5; 100]);
    let transcriber = FailingTranscriber;
    let (injector, injected_texts) = MockTextInjector::new();
    let (indicator, indicator_states) = MockStatusIndicator::new();
    let shutdown = Arc::new(AtomicBool::new(false));

    let daemon = Daemon::new(
        Box::new(hotkey_listener),
        Box::new(audio_capture),
        Box::new(transcriber),
        Box::new(injector),
        Box::new(indicator),
        shutdown.clone(),
    );

    let shutdown_clone = shutdown.clone();
    let handle = std::thread::spawn(move || daemon.run());

    // Start recording
    hotkey_tx.send(()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Stop recording (transcription will fail)
    hotkey_tx.send(()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Daemon should be back in Idle — start another cycle to prove recovery
    hotkey_tx.send(()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Shutdown
    shutdown_clone.store(true, Ordering::SeqCst);
    drop(hotkey_tx);

    handle.join().unwrap().unwrap();

    // No text should have been injected (transcription failed)
    let texts = injected_texts.lock().unwrap();
    assert!(texts.is_empty());

    // States: Idle, Recording, Processing, Idle (error recovery), Recording (new cycle)
    let states = indicator_states.lock().unwrap();
    assert!(states.len() >= 4);
    assert_eq!(states[0], DaemonState::Idle);
    assert_eq!(states[1], DaemonState::Recording);
    assert_eq!(states[2], DaemonState::Processing);
    assert_eq!(states[3], DaemonState::Idle);
    // After recovery, a new recording started
    assert_eq!(states[4], DaemonState::Recording);
}

#[test]
fn integration_graceful_shutdown() {
    let (hotkey_listener, hotkey_tx) = MockHotkeyListener::new();
    let audio_capture = MockAudioCapture::new(vec![]);
    let transcriber = MockTranscriber::new("");
    let (injector, _) = MockTextInjector::new();
    let (indicator, indicator_states) = MockStatusIndicator::new();
    let shutdown = Arc::new(AtomicBool::new(false));

    let daemon = Daemon::new(
        Box::new(hotkey_listener),
        Box::new(audio_capture),
        Box::new(transcriber),
        Box::new(injector),
        Box::new(indicator),
        shutdown.clone(),
    );

    let shutdown_clone = shutdown.clone();
    let handle = std::thread::spawn(move || daemon.run());

    // Signal shutdown immediately (simulates SIGINT/SIGTERM)
    shutdown_clone.store(true, Ordering::SeqCst);
    drop(hotkey_tx);

    let result = handle.join().unwrap();
    assert!(result.is_ok());

    let states = indicator_states.lock().unwrap();
    assert_eq!(states[0], DaemonState::Idle);
}
