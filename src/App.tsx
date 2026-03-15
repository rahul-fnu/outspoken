import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAudioLevel } from "./useAudioLevel";
import Settings from "./components/Settings";
import History from "./components/History";

interface TranscriptionResult {
  text: string;
  segments: { start_ms: number; end_ms: number; text: string }[];
  language: string;
  duration_ms: number;
}

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

type View = "main" | "settings" | "history";

function App() {
  const [view, setView] = useState<View>("main");

  // Recording state
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [transcription, setTranscription] = useState<TranscriptionResult | null>(null);
  const [recordingDuration, setRecordingDuration] = useState(0);
  const recordingStartRef = useRef<number | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [copied, setCopied] = useState(false);
  const [recordError, setRecordError] = useState<string | null>(null);

  // Model & language state (shared with settings)
  const [activeModel, setActiveModel] = useState<string | null>(null);
  const [selectedLanguage, setSelectedLanguage] = useState<string>("auto");

  // Audio level from hook
  const { levelDb } = useAudioLevel();

  // Normalize level for display: map -60..0 dB to 0..1
  const normalizedLevel = isRecording
    ? Math.max(0, Math.min(1, (levelDb + 60) / 60))
    : 0;

  // Recording duration timer
  useEffect(() => {
    if (isRecording) {
      recordingStartRef.current = Date.now();
      setRecordingDuration(0);
      timerRef.current = setInterval(() => {
        if (recordingStartRef.current) {
          setRecordingDuration((Date.now() - recordingStartRef.current) / 1000);
        }
      }, 100);
    } else {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
      recordingStartRef.current = null;
    }
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [isRecording]);

  async function toggleRecording() {
    setRecordError(null);
    if (isRecording) {
      // Stop recording and transcribe
      setIsRecording(false);
      setIsProcessing(true);
      try {
        const audioData = await invoke<number[]>("stop_recording");
        await invoke("set_tray_state", { state: "processing" });
        const result = await invoke<TranscriptionResult>("transcribe_recording", { audioData });
        // Apply text post-processing (filler removal + dictionary corrections)
        try {
          const settings = await invoke<{ strip_filler_words: boolean }>("get_settings");
          const processed = await invoke<string>("process_transcription_text", {
            text: result.text,
            stripFillers: settings.strip_filler_words,
          });
          result.text = processed;
        } catch {
          // If post-processing fails, use raw text
        }
        setTranscription(result);
        // Auto-save to history
        if (result.text) {
          let sourceApp = "";
          try {
            const appInfo = await invoke<{ name: string }>("get_active_app");
            sourceApp = appInfo.name || "";
          } catch { /* ignore */ }
          await invoke("save_transcription", {
            result: {
              text: result.text,
              raw_text: result.text,
              duration_ms: Math.round(recordingDuration * 1000),
              source_app: sourceApp || undefined,
              language: result.language || undefined,
              model_used: undefined,
            },
          }).catch(() => {});
        }
      } catch (e) {
        setRecordError(String(e));
      } finally {
        setIsProcessing(false);
        await invoke("set_tray_state", { state: "idle" }).catch(() => {});
      }
    } else {
      // Start recording
      try {
        await invoke("start_recording");
        setIsRecording(true);
        setTranscription(null);
        setCopied(false);
        await invoke("set_tray_state", { state: "recording" }).catch(() => {});
      } catch (e) {
        setRecordError(String(e));
      }
    }
  }

  async function handleCopy() {
    if (!transcription?.text) return;
    try {
      await navigator.clipboard.writeText(transcription.text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback
      const ta = document.createElement("textarea");
      ta.value = transcription.text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }

  const hasModel = activeModel !== null;

  // --- Main Recording View ---
  if (view === "main") {
    return (
      <main style={styles.container}>
        {/* Header */}
        <div style={styles.header}>
          <h1 style={styles.title}>Outspoken</h1>
          <div style={{ display: "flex", gap: "0.5rem" }}>
            <button onClick={() => setView("history")} style={styles.settingsBtn}>
              History
            </button>
            <button onClick={() => setView("settings")} style={styles.settingsBtn}>
              Settings
            </button>
          </div>
        </div>

        {/* Status bar */}
        <div style={styles.statusBar}>
          <span>
            Model: <strong>{activeModel ?? "None"}</strong>
          </span>
          <span>
            Language: <strong>{selectedLanguage === "auto" ? "Auto" : selectedLanguage}</strong>
          </span>
          {isRecording && (
            <span>
              Duration: <strong>{formatDuration(recordingDuration)}</strong>
            </span>
          )}
        </div>

        {!hasModel && (
          <div style={styles.notice}>
            No model loaded. Go to <button onClick={() => setView("settings")} style={styles.linkBtn}>Settings</button> to download and load a model.
          </div>
        )}

        {/* Record Button */}
        <div style={styles.recordSection}>
          <button
            onClick={toggleRecording}
            disabled={!hasModel || isProcessing}
            style={{
              ...styles.recordBtn,
              backgroundColor: isRecording ? "#f44336" : hasModel ? "#4caf50" : "#999",
              transform: isRecording ? "scale(1.05)" : "scale(1)",
            }}
          >
            <div style={styles.recordBtnInner}>
              {isProcessing ? (
                <div style={styles.spinner} />
              ) : (
                <div
                  style={{
                    width: 24,
                    height: 24,
                    borderRadius: isRecording ? 4 : 12,
                    backgroundColor: "#fff",
                  }}
                />
              )}
            </div>
            <span style={styles.recordBtnLabel}>
              {isProcessing ? "Processing..." : isRecording ? "Stop Recording" : "Start Recording"}
            </span>
          </button>
        </div>

        {/* Audio Level Meter */}
        <div style={styles.levelContainer}>
          <div style={styles.levelLabel}>Audio Level</div>
          <div style={styles.levelTrack}>
            <div
              style={{
                ...styles.levelBar,
                width: `${normalizedLevel * 100}%`,
                backgroundColor: normalizedLevel > 0.8 ? "#f44336" : normalizedLevel > 0.5 ? "#ff9800" : "#4caf50",
              }}
            />
          </div>
          {isRecording && (
            <div style={styles.levelDb}>{levelDb.toFixed(1)} dB</div>
          )}
        </div>

        {/* Error display */}
        {recordError && (
          <div style={styles.error}>{recordError}</div>
        )}

        {/* Processing indicator */}
        {isProcessing && (
          <div style={styles.processingIndicator}>
            <div style={styles.spinner} />
            <span style={{ marginLeft: 8 }}>Transcribing audio...</span>
          </div>
        )}

        {/* Transcription output */}
        <div style={styles.transcriptionSection}>
          <div style={styles.transcriptionHeader}>
            <span style={styles.transcriptionLabel}>Transcription</span>
            {transcription?.text && (
              <button onClick={handleCopy} style={styles.copyBtn}>
                {copied ? "Copied!" : "Copy"}
              </button>
            )}
          </div>
          <div style={styles.transcriptionBox}>
            {transcription?.text ? (
              <p style={styles.transcriptionText}>{transcription.text}</p>
            ) : (
              <p style={styles.transcriptionPlaceholder}>
                {isRecording
                  ? "Recording... Click stop when done."
                  : "Press the record button to start dictating."}
              </p>
            )}
          </div>
          {transcription && (
            <div style={styles.transcriptionMeta}>
              Detected language: {transcription.language} | Processed in {transcription.duration_ms}ms
            </div>
          )}
        </div>
      </main>
    );
  }

  // --- History View ---
  if (view === "history") {
    return <History onBack={() => setView("main")} />;
  }

  // --- Settings View ---
  return (
    <Settings
      onBack={() => setView("main")}
      onModelLoaded={(name) => setActiveModel(name || null)}
      activeModel={activeModel}
      onLanguageChange={setSelectedLanguage}
      selectedLanguage={selectedLanguage}
    />
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    padding: "1.5rem",
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
    maxWidth: 640,
    margin: "0 auto",
    minHeight: "100vh",
    boxSizing: "border-box",
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: "1rem",
  },
  title: {
    margin: 0,
    fontSize: "1.5rem",
  },
  settingsBtn: {
    padding: "0.4rem 1rem",
    fontSize: "0.9rem",
    cursor: "pointer",
    borderRadius: 6,
    border: "1px solid #ccc",
    background: "#f5f5f5",
  },
  statusBar: {
    display: "flex",
    gap: "1.5rem",
    fontSize: "0.85rem",
    color: "#666",
    padding: "0.5rem 0.75rem",
    background: "#f9f9f9",
    borderRadius: 6,
    marginBottom: "1.5rem",
    flexWrap: "wrap" as const,
  },
  notice: {
    padding: "0.75rem 1rem",
    background: "#fff3cd",
    border: "1px solid #ffc107",
    borderRadius: 6,
    marginBottom: "1.5rem",
    fontSize: "0.9rem",
  },
  linkBtn: {
    background: "none",
    border: "none",
    color: "#1976d2",
    textDecoration: "underline",
    cursor: "pointer",
    padding: 0,
    fontSize: "inherit",
  },
  recordSection: {
    display: "flex",
    justifyContent: "center",
    marginBottom: "1.5rem",
  },
  recordBtn: {
    display: "flex",
    flexDirection: "column" as const,
    alignItems: "center",
    gap: 8,
    border: "none",
    borderRadius: 16,
    padding: "1.25rem 2.5rem",
    cursor: "pointer",
    color: "#fff",
    fontSize: "1rem",
    fontWeight: 600,
    transition: "all 0.2s",
    boxShadow: "0 2px 8px rgba(0,0,0,0.15)",
  },
  recordBtnInner: {
    width: 48,
    height: 48,
    borderRadius: 24,
    border: "3px solid rgba(255,255,255,0.5)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  },
  recordBtnLabel: {
    fontSize: "0.85rem",
    opacity: 0.95,
  },
  levelContainer: {
    marginBottom: "1.5rem",
  },
  levelLabel: {
    fontSize: "0.8rem",
    color: "#888",
    marginBottom: 4,
  },
  levelTrack: {
    height: 8,
    background: "#e0e0e0",
    borderRadius: 4,
    overflow: "hidden",
  },
  levelBar: {
    height: "100%",
    borderRadius: 4,
    transition: "width 0.05s linear",
  },
  levelDb: {
    fontSize: "0.75rem",
    color: "#999",
    marginTop: 2,
    textAlign: "right" as const,
  },
  error: {
    color: "#d32f2f",
    padding: "0.5rem 0.75rem",
    background: "#ffebee",
    borderRadius: 6,
    marginBottom: "1rem",
    fontSize: "0.85rem",
  },
  processingIndicator: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: "1rem",
    color: "#666",
    fontSize: "0.9rem",
  },
  spinner: {
    width: 20,
    height: 20,
    border: "3px solid #e0e0e0",
    borderTop: "3px solid #666",
    borderRadius: "50%",
    animation: "spin 0.8s linear infinite",
  },
  transcriptionSection: {
    flex: 1,
  },
  transcriptionHeader: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 8,
  },
  transcriptionLabel: {
    fontSize: "0.85rem",
    fontWeight: 600,
    color: "#555",
  },
  copyBtn: {
    padding: "0.3rem 0.75rem",
    fontSize: "0.8rem",
    cursor: "pointer",
    borderRadius: 4,
    border: "1px solid #ccc",
    background: "#f5f5f5",
  },
  transcriptionBox: {
    border: "1px solid #e0e0e0",
    borderRadius: 8,
    padding: "1rem",
    minHeight: 120,
    background: "#fafafa",
  },
  transcriptionText: {
    margin: 0,
    lineHeight: 1.6,
    fontSize: "1rem",
    whiteSpace: "pre-wrap" as const,
  },
  transcriptionPlaceholder: {
    margin: 0,
    color: "#aaa",
    fontStyle: "italic",
  },
  transcriptionMeta: {
    fontSize: "0.75rem",
    color: "#999",
    marginTop: 6,
  },
};

export default App;
