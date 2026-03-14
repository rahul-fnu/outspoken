import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAudioLevel } from "./useAudioLevel";

interface ModelInfo {
  name: string;
  filename: string;
  size_bytes: number;
  description: string;
}

interface DownloadedModel {
  name: string;
  filename: string;
  size_bytes: number;
  path: string;
  version: string;
  downloaded_at: string;
}

interface DownloadProgress {
  model_name: string;
  downloaded_bytes: number;
  total_bytes: number;
  progress_percent: number;
  status: "Downloading" | "Completed" | "Cancelled" | "Failed";
}

interface TranscriptionResult {
  text: string;
  segments: { start_ms: number; end_ms: number; text: string }[];
  language: string;
  duration_ms: number;
}

interface SupportedLanguage {
  code: string;
  name: string;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(0)} MB`;
  return `${(bytes / 1_000).toFixed(0)} KB`;
}

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

type View = "main" | "settings";

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

  // Model state
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<DownloadedModel[]>([]);
  const [downloading, setDownloading] = useState<Record<string, DownloadProgress>>({});
  const [activeModel, setActiveModel] = useState<string | null>(null);
  const [loadingModel, setLoadingModel] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Settings state
  const [languages, setLanguages] = useState<SupportedLanguage[]>([]);
  const [selectedLanguage, setSelectedLanguage] = useState<string>("auto");
  const [hotkey, setHotkey] = useState<string>("");
  const [hotkeyInput, setHotkeyInput] = useState<string>("");
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [hotkeySuccess, setHotkeySuccess] = useState<string | null>(null);

  // Audio level from hook
  const { levelDb } = useAudioLevel();

  // Normalize level for display: map -60..0 dB to 0..1
  const normalizedLevel = isRecording
    ? Math.max(0, Math.min(1, (levelDb + 60) / 60))
    : 0;

  // Load models and languages on mount
  const loadModels = useCallback(async () => {
    try {
      const [available, downloaded] = await Promise.all([
        invoke<ModelInfo[]>("list_available_models"),
        invoke<DownloadedModel[]>("list_models"),
      ]);
      setAvailableModels(available);
      setDownloadedModels(downloaded);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadLanguages = useCallback(async () => {
    try {
      const langs = await invoke<SupportedLanguage[]>("list_supported_languages");
      setLanguages(langs);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadHotkey = useCallback(async () => {
    try {
      const current = await invoke<string>("get_hotkey");
      setHotkey(current);
      setHotkeyInput(current);
    } catch (e) {
      setHotkeyError(String(e));
    }
  }, []);

  useEffect(() => {
    loadModels();
    loadLanguages();
    loadHotkey();
  }, [loadModels, loadLanguages, loadHotkey]);

  // Poll download progress
  useEffect(() => {
    const activeDownloads = Object.keys(downloading).filter(
      (name) => downloading[name].status === "Downloading"
    );
    if (activeDownloads.length === 0) return;

    const interval = setInterval(async () => {
      for (const name of activeDownloads) {
        try {
          const progress = await invoke<DownloadProgress>("get_download_progress", {
            modelName: name,
          });
          setDownloading((prev) => ({ ...prev, [name]: progress }));
          if (progress.status === "Completed") {
            loadModels();
          }
        } catch {
          // Progress not available yet
        }
      }
    }, 500);

    return () => clearInterval(interval);
  }, [downloading, loadModels]);

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

  async function handleLoadModel(modelName: string) {
    setLoadingModel(true);
    setError(null);
    try {
      const lang = selectedLanguage === "auto" ? null : selectedLanguage;
      await invoke("load_transcription_model", {
        modelName,
        config: { language: lang, translate: false, thread_count: 4, strip_filler_words: false },
      });
      setActiveModel(modelName);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingModel(false);
    }
  }

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
        setTranscription(result);
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

  async function handleDownload(modelName: string) {
    setError(null);
    setDownloading((prev) => ({
      ...prev,
      [modelName]: {
        model_name: modelName,
        downloaded_bytes: 0,
        total_bytes: 0,
        progress_percent: 0,
        status: "Downloading",
      },
    }));
    try {
      await invoke<DownloadedModel>("download_model", { modelName });
      await loadModels();
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading((prev) => {
        const next = { ...prev };
        delete next[modelName];
        return next;
      });
    }
  }

  async function handleCancel(modelName: string) {
    try {
      await invoke("cancel_download", { modelName });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleDelete(modelName: string) {
    setError(null);
    try {
      await invoke("delete_model", { name: modelName });
      if (activeModel === modelName) setActiveModel(null);
      await loadModels();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleSetHotkey() {
    setHotkeyError(null);
    setHotkeySuccess(null);
    try {
      await invoke("set_hotkey", { shortcut: hotkeyInput });
      setHotkey(hotkeyInput);
      setHotkeySuccess("Hotkey updated successfully");
      setTimeout(() => setHotkeySuccess(null), 3000);
    } catch (e) {
      setHotkeyError(String(e));
    }
  }

  const downloadedNames = new Set(downloadedModels.map((m) => m.name));
  const hasModel = activeModel !== null;

  // --- Main Recording View ---
  if (view === "main") {
    return (
      <main style={styles.container}>
        {/* Header */}
        <div style={styles.header}>
          <h1 style={styles.title}>Outspoken</h1>
          <button onClick={() => setView("settings")} style={styles.settingsBtn}>
            Settings
          </button>
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

  // --- Settings View ---
  return (
    <main style={styles.container}>
      <div style={styles.header}>
        <h1 style={styles.title}>Settings</h1>
        <button onClick={() => setView("main")} style={styles.settingsBtn}>
          Back
        </button>
      </div>

      {error && (
        <div style={styles.error}>{error}</div>
      )}

      {/* Language */}
      <section style={styles.section}>
        <h2 style={styles.sectionTitle}>Language</h2>
        <select
          value={selectedLanguage}
          onChange={(e) => setSelectedLanguage(e.target.value)}
          style={styles.select}
        >
          <option value="auto">Auto-detect</option>
          {languages.map((lang) => (
            <option key={lang.code} value={lang.code}>
              {lang.name} ({lang.code})
            </option>
          ))}
        </select>
        <div style={styles.hint}>
          {selectedLanguage === "auto"
            ? "Whisper will automatically detect the spoken language."
            : `Manual selection: ${selectedLanguage}`}
        </div>
      </section>

      {/* Hotkey */}
      <section style={styles.section}>
        <h2 style={styles.sectionTitle}>Global Hotkey</h2>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            type="text"
            value={hotkeyInput}
            onChange={(e) => setHotkeyInput(e.target.value)}
            placeholder="e.g. Ctrl+Shift+Space"
            style={styles.input}
          />
          <button onClick={handleSetHotkey} disabled={hotkeyInput === hotkey}>
            Apply
          </button>
        </div>
        <div style={styles.hint}>
          Current: <strong>{hotkey || "None"}</strong> -- toggles dictation from any app
        </div>
        {hotkeyError && <div style={{ ...styles.hint, color: "red" }}>{hotkeyError}</div>}
        {hotkeySuccess && <div style={{ ...styles.hint, color: "green" }}>{hotkeySuccess}</div>}
      </section>

      {/* Models */}
      <section style={styles.section}>
        <h2 style={styles.sectionTitle}>Whisper Models</h2>
        {availableModels.map((model) => {
          const isDownloaded = downloadedNames.has(model.name);
          const progress = downloading[model.name];
          const isDownloading = progress?.status === "Downloading";
          const isActive = activeModel === model.name;

          return (
            <div key={model.name} style={styles.modelCard}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <div>
                  <strong>{model.name}</strong>
                  {isActive && <span style={styles.activeBadge}>Active</span>}
                  <span style={{ marginLeft: 8, color: "#888" }}>{formatBytes(model.size_bytes)}</span>
                </div>
                <div style={{ display: "flex", gap: 4 }}>
                  {isDownloaded && !isDownloading && !isActive && (
                    <button onClick={() => handleLoadModel(model.name)} disabled={loadingModel}>
                      {loadingModel ? "Loading..." : "Load"}
                    </button>
                  )}
                  {isDownloaded && !isDownloading && (
                    <button onClick={() => handleDelete(model.name)}>Delete</button>
                  )}
                  {!isDownloaded && !isDownloading && (
                    <button onClick={() => handleDownload(model.name)}>Download</button>
                  )}
                  {isDownloading && (
                    <button onClick={() => handleCancel(model.name)}>Cancel</button>
                  )}
                </div>
              </div>
              <div style={styles.hint}>{model.description}</div>
              {isDownloading && progress && (
                <div style={{ marginTop: 8 }}>
                  <div style={styles.progressTrack}>
                    <div style={{ ...styles.progressBar, width: `${progress.progress_percent}%` }} />
                  </div>
                  <div style={{ fontSize: "0.8rem", marginTop: 4 }}>
                    {formatBytes(progress.downloaded_bytes)} / {formatBytes(progress.total_bytes)} (
                    {progress.progress_percent.toFixed(1)}%)
                  </div>
                </div>
              )}
              {isDownloaded && !isActive && (
                <div style={{ fontSize: "0.8rem", color: "green", marginTop: 4 }}>Downloaded</div>
              )}
            </div>
          );
        })}
      </section>
    </main>
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
  section: {
    marginBottom: "1.5rem",
  },
  sectionTitle: {
    fontSize: "1.1rem",
    marginBottom: "0.5rem",
  },
  select: {
    padding: "0.4rem",
    fontSize: "1rem",
    minWidth: 200,
  },
  input: {
    padding: "0.4rem",
    fontSize: "1rem",
    minWidth: 200,
  },
  hint: {
    fontSize: "0.85rem",
    color: "#666",
    marginTop: 4,
  },
  modelCard: {
    border: "1px solid #ddd",
    borderRadius: 8,
    padding: "0.75rem 1rem",
    marginBottom: "0.5rem",
  },
  activeBadge: {
    marginLeft: 8,
    fontSize: "0.75rem",
    padding: "2px 6px",
    background: "#4caf50",
    color: "#fff",
    borderRadius: 4,
  },
  progressTrack: {
    background: "#eee",
    borderRadius: 4,
    height: 8,
    overflow: "hidden",
  },
  progressBar: {
    background: "#4caf50",
    height: "100%",
    transition: "width 0.3s",
  },
};

export default App;
