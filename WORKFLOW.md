---
tracker:
  kind: github
  repo: rahul-fnu/outspoken
  labels: [sub-issue, phase-1]
  auto_close: true
  in_progress_label: in-progress
  done_label: "forge:done"
agent:
  type: claude-code
  timeout: 30m
concurrency:
  max_agents: 2
validation:
  steps:
    - name: build
      command: "xcodebuild -scheme Outspoken -destination 'platform=macOS' build 2>&1 | tail -20 || true"
      retries: 2
      description: "Verify Xcode project builds"
  on_failure: output-wip
---
You are an autonomous coding agent building **Outspoken**, a free self-hosted AI dictation app for macOS and iOS.

## Issue: {{issue.title}}

{{issue.description}}

## Tech Stack
- **Language:** Swift 5.9+
- **UI:** SwiftUI
- **Platforms:** macOS 14.0+ (menu bar app), iOS 17.0+ (keyboard extension)
- **Speech-to-text:** whisper.cpp (C library with Swift wrapper)
- **Storage:** SwiftData
- **Build:** Xcode 15+, Swift Package Manager

## Architecture
- `OutspokenCore/` — shared Swift package (audio, transcription, models, processing)
- `Outspoken/` — macOS app target (menu bar, SwiftUI)
- `OutspokenMobile/` — iOS app target
- `OutspokenKeyboard/` — iOS keyboard extension target

## Instructions
1. Read existing source files to understand what's already built before making changes.
2. Follow Swift conventions: protocols for abstraction, async/await for concurrency, actors for thread safety.
3. Put shared logic in `OutspokenCore/` package — platform-specific code in app targets.
4. Use SwiftUI for all UI. Use `@Observable` (not ObservableObject) for state.
5. For whisper.cpp integration, use the C API directly via a Swift bridging header.
6. Write minimal, focused code. Don't over-abstract.
7. If creating the initial project structure, use proper Xcode project layout with Package.swift for the shared package.
8. Commit your changes with clear messages describing what was built.
