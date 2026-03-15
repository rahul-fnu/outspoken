import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AppSettings {
  language: string;
  auto_start: boolean;
  hotkey: string;
  silence_threshold_db: number;
  silence_duration_secs: number;
  audio_input_device: string | null;
  active_model: string | null;
  strip_filler_words: boolean;
  personal_dictionary: string[];
  openai_api_key: string;
  anthropic_api_key: string;
}

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

interface SupportedLanguage {
  code: string;
  name: string;
}

interface AudioDeviceInfo {
  name: string;
  is_default: boolean;
}

type SettingsTab = "general" | "recording" | "models" | "text" | "ai" | "about";

function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(0)} MB`;
  return `${(bytes / 1_000).toFixed(0)} KB`;
}

interface SettingsProps {
  onBack: () => void;
  onModelLoaded: (modelName: string) => void;
  activeModel: string | null;
  onLanguageChange: (lang: string) => void;
  selectedLanguage: string;
}

export default function Settings({
  onBack,
  onModelLoaded,
  activeModel,
  onLanguageChange,
  selectedLanguage,
}: SettingsProps) {
  const [tab, setTab] = useState<SettingsTab>("general");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saveStatus, setSaveStatus] = useState<string | null>(null);

  // General
  const [languages, setLanguages] = useState<SupportedLanguage[]>([]);

  // Recording
  const [audioDevices, setAudioDevices] = useState<AudioDeviceInfo[]>([]);
  const [hotkeyInput, setHotkeyInput] = useState("");
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [hotkeySuccess, setHotkeySuccess] = useState<string | null>(null);

  // Models
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<DownloadedModel[]>([]);
  const [downloading, setDownloading] = useState<Record<string, DownloadProgress>>({});
  const [loadingModel, setLoadingModel] = useState(false);

  // Text Processing
  interface DictEntry {
    id: number;
    from_text: string;
    to_text: string;
    case_sensitive: boolean;
  }
  const [dictFrom, setDictFrom] = useState("");
  const [dictTo, setDictTo] = useState("");
  const [dictCaseSensitive, setDictCaseSensitive] = useState(false);
  const [dictEntries, setDictEntries] = useState<DictEntry[]>([]);

  // AI Providers
  const [showOpenAI, setShowOpenAI] = useState(false);
  const [showAnthropic, setShowAnthropic] = useState(false);

  const loadSettings = useCallback(async () => {
    try {
      const s = await invoke<AppSettings>("get_settings");
      setSettings(s);
      setHotkeyInput(s.hotkey);
    } catch (e) {
      setError(String(e));
    }
  }, []);

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

  useEffect(() => {
    loadSettings();
    invoke<SupportedLanguage[]>("list_supported_languages")
      .then(setLanguages)
      .catch((e) => setError(String(e)));
    invoke<AudioDeviceInfo[]>("list_audio_devices")
      .then(setAudioDevices)
      .catch(() => {}); // May fail in Docker/CI
    loadModels();
    invoke<DictEntry[]>("list_dictionary")
      .then(setDictEntries)
      .catch(() => {});
  }, [loadSettings, loadModels]);

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

  async function persistSettings(updated: AppSettings) {
    setSettings(updated);
    try {
      await invoke("update_settings", { newSettings: updated });
      setSaveStatus("Saved");
      setTimeout(() => setSaveStatus(null), 1500);
    } catch (e) {
      setError(String(e));
    }
  }

  function updateField<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
    if (!settings) return;
    const updated = { ...settings, [key]: value };
    persistSettings(updated);
  }

  async function handleSetHotkey() {
    if (!settings) return;
    setHotkeyError(null);
    setHotkeySuccess(null);
    try {
      await invoke("set_hotkey", { shortcut: hotkeyInput });
      updateField("hotkey", hotkeyInput);
      setHotkeySuccess("Hotkey applied");
      setTimeout(() => setHotkeySuccess(null), 3000);
    } catch (e) {
      setHotkeyError(String(e));
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
      await invoke("download_model", { modelName });
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
      if (activeModel === modelName) {
        onModelLoaded("");
      }
      await loadModels();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleLoadModel(modelName: string) {
    setLoadingModel(true);
    setError(null);
    try {
      const lang = selectedLanguage === "auto" ? null : selectedLanguage;
      await invoke("load_transcription_model", {
        modelName,
        config: { language: lang, translate: false, thread_count: 4, strip_filler_words: settings?.strip_filler_words ?? false },
      });
      onModelLoaded(modelName);
      updateField("active_model", modelName);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingModel(false);
    }
  }

  async function handleAddDictEntry() {
    if (!dictFrom.trim() || !dictTo.trim()) return;
    try {
      const entry = await invoke<DictEntry>("add_dictionary_entry", {
        fromText: dictFrom.trim(),
        toText: dictTo.trim(),
        caseSensitive: dictCaseSensitive,
      });
      setDictEntries((prev) => [...prev, entry]);
      setDictFrom("");
      setDictTo("");
      setDictCaseSensitive(false);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRemoveDictEntry(id: number) {
    try {
      await invoke("remove_dictionary_entry", { id });
      setDictEntries((prev) => prev.filter((e) => e.id !== id));
    } catch (e) {
      setError(String(e));
    }
  }

  if (!settings) {
    return (
      <main style={styles.container}>
        <div style={styles.header}>
          <h1 style={styles.title}>Settings</h1>
          <button onClick={onBack} style={styles.backBtn}>Back</button>
        </div>
        <p>Loading settings...</p>
      </main>
    );
  }

  const downloadedNames = new Set(downloadedModels.map((m) => m.name));

  const tabs: { id: SettingsTab; label: string }[] = [
    { id: "general", label: "General" },
    { id: "recording", label: "Recording" },
    { id: "models", label: "Models" },
    { id: "text", label: "Text Processing" },
    { id: "ai", label: "AI Providers" },
    { id: "about", label: "About" },
  ];

  return (
    <main style={styles.container}>
      <div style={styles.header}>
        <h1 style={styles.title}>Settings</h1>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {saveStatus && <span style={styles.saveIndicator}>{saveStatus}</span>}
          <button onClick={onBack} style={styles.backBtn}>Back</button>
        </div>
      </div>

      {error && (
        <div style={styles.error}>
          {error}
          <button onClick={() => setError(null)} style={styles.dismissBtn}>x</button>
        </div>
      )}

      {/* Tab navigation */}
      <div style={styles.tabBar}>
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            style={{
              ...styles.tabBtn,
              ...(tab === t.id ? styles.tabBtnActive : {}),
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div style={styles.tabContent}>
        {/* ===== GENERAL ===== */}
        {tab === "general" && (
          <div>
            <h2 style={styles.sectionTitle}>Language Preference</h2>
            <select
              value={selectedLanguage}
              onChange={(e) => {
                onLanguageChange(e.target.value);
                updateField("language", e.target.value);
              }}
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

            <h2 style={{ ...styles.sectionTitle, marginTop: 24 }}>Auto-start at Login</h2>
            <label style={styles.checkboxLabel}>
              <input
                type="checkbox"
                checked={settings.auto_start}
                onChange={(e) => updateField("auto_start", e.target.checked)}
              />
              <span>Launch Outspoken automatically when you log in</span>
            </label>
          </div>
        )}

        {/* ===== RECORDING ===== */}
        {tab === "recording" && (
          <div>
            <h2 style={styles.sectionTitle}>Global Hotkey</h2>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                type="text"
                value={hotkeyInput}
                onChange={(e) => setHotkeyInput(e.target.value)}
                placeholder="e.g. Ctrl+Shift+Space"
                style={styles.input}
              />
              <button
                onClick={handleSetHotkey}
                disabled={hotkeyInput === settings.hotkey}
                style={styles.actionBtn}
              >
                Apply
              </button>
            </div>
            <div style={styles.hint}>
              Current: <strong>{settings.hotkey || "None"}</strong> -- toggles dictation from any app
            </div>
            {hotkeyError && <div style={{ ...styles.hint, color: "#d32f2f" }}>{hotkeyError}</div>}
            {hotkeySuccess && <div style={{ ...styles.hint, color: "#4caf50" }}>{hotkeySuccess}</div>}

            <h2 style={{ ...styles.sectionTitle, marginTop: 24 }}>Silence Detection</h2>
            <div style={styles.fieldRow}>
              <label style={styles.fieldLabel}>Threshold (dB)</label>
              <input
                type="range"
                min={-60}
                max={-10}
                step={1}
                value={settings.silence_threshold_db}
                onChange={(e) => {
                  const val = parseFloat(e.target.value);
                  updateField("silence_threshold_db", val);
                  invoke("set_silence_config", {
                    thresholdDb: val,
                    durationSecs: settings.silence_duration_secs,
                  }).catch(() => {});
                }}
                style={{ flex: 1 }}
              />
              <span style={styles.fieldValue}>{settings.silence_threshold_db} dB</span>
            </div>
            <div style={styles.fieldRow}>
              <label style={styles.fieldLabel}>Duration (sec)</label>
              <input
                type="range"
                min={0.5}
                max={10}
                step={0.5}
                value={settings.silence_duration_secs}
                onChange={(e) => {
                  const val = parseFloat(e.target.value);
                  updateField("silence_duration_secs", val);
                  invoke("set_silence_config", {
                    thresholdDb: settings.silence_threshold_db,
                    durationSecs: val,
                  }).catch(() => {});
                }}
                style={{ flex: 1 }}
              />
              <span style={styles.fieldValue}>{settings.silence_duration_secs}s</span>
            </div>

            <h2 style={{ ...styles.sectionTitle, marginTop: 24 }}>Audio Input Device</h2>
            <select
              value={settings.audio_input_device ?? ""}
              onChange={(e) => {
                const val = e.target.value || null;
                updateField("audio_input_device", val);
                invoke("select_audio_device", { deviceName: val }).catch(() => {});
              }}
              style={styles.select}
            >
              <option value="">System Default</option>
              {audioDevices.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name} {d.is_default ? "(default)" : ""}
                </option>
              ))}
            </select>
          </div>
        )}

        {/* ===== MODELS ===== */}
        {tab === "models" && (
          <div>
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
                        <button onClick={() => handleLoadModel(model.name)} disabled={loadingModel} style={styles.actionBtn}>
                          {loadingModel ? "Loading..." : "Load"}
                        </button>
                      )}
                      {isDownloaded && !isDownloading && (
                        <button onClick={() => handleDelete(model.name)} style={styles.dangerBtn}>Delete</button>
                      )}
                      {!isDownloaded && !isDownloading && (
                        <button onClick={() => handleDownload(model.name)} style={styles.actionBtn}>Download</button>
                      )}
                      {isDownloading && (
                        <button onClick={() => handleCancel(model.name)} style={styles.dangerBtn}>Cancel</button>
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
                    <div style={{ fontSize: "0.8rem", color: "#4caf50", marginTop: 4 }}>Downloaded</div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        {/* ===== TEXT PROCESSING ===== */}
        {tab === "text" && (
          <div>
            <h2 style={styles.sectionTitle}>Filler Word Removal</h2>
            <label style={styles.checkboxLabel}>
              <input
                type="checkbox"
                checked={settings.strip_filler_words}
                onChange={(e) => updateField("strip_filler_words", e.target.checked)}
              />
              <span>Remove filler words (um, uh, like, you know) from transcriptions</span>
            </label>
            <div style={styles.hint}>
              Removes: um, uh, er, ah, like (filler), you know, I mean, sort of, kind of, basically, actually, literally
            </div>

            <h2 style={{ ...styles.sectionTitle, marginTop: 24 }}>Personal Dictionary</h2>
            <div style={styles.hint}>
              Map misrecognized words to their correct form (e.g., "eye phone" → "iPhone")
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 8, alignItems: "center" }}>
              <input
                type="text"
                value={dictFrom}
                onChange={(e) => setDictFrom(e.target.value)}
                placeholder="From (wrong)..."
                style={{ ...styles.input, minWidth: 120 }}
              />
              <span style={{ color: "#999" }}>→</span>
              <input
                type="text"
                value={dictTo}
                onChange={(e) => setDictTo(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleAddDictEntry()}
                placeholder="To (correct)..."
                style={{ ...styles.input, minWidth: 120 }}
              />
              <label style={{ ...styles.checkboxLabel, fontSize: "0.8rem", whiteSpace: "nowrap" }}>
                <input
                  type="checkbox"
                  checked={dictCaseSensitive}
                  onChange={(e) => setDictCaseSensitive(e.target.checked)}
                />
                <span>Case-sensitive</span>
              </label>
              <button
                onClick={handleAddDictEntry}
                disabled={!dictFrom.trim() || !dictTo.trim()}
                style={styles.actionBtn}
              >
                Add
              </button>
            </div>
            {dictEntries.length > 0 && (
              <div style={{ marginTop: 12 }}>
                {dictEntries.map((entry) => (
                  <div key={entry.id} style={styles.dictEntryRow}>
                    <span style={{ flex: 1 }}>
                      <span style={{ color: "#d32f2f" }}>{entry.from_text}</span>
                      {" → "}
                      <span style={{ color: "#2e7d32" }}>{entry.to_text}</span>
                      {entry.case_sensitive && (
                        <span style={{ fontSize: "0.75rem", color: "#888", marginLeft: 6 }}>(case-sensitive)</span>
                      )}
                    </span>
                    <button
                      onClick={() => handleRemoveDictEntry(entry.id)}
                      style={styles.chipRemove}
                    >
                      x
                    </button>
                  </div>
                ))}
              </div>
            )}
            {dictEntries.length === 0 && (
              <div style={{ ...styles.hint, marginTop: 8 }}>No dictionary entries yet.</div>
            )}
          </div>
        )}

        {/* ===== AI PROVIDERS ===== */}
        {tab === "ai" && (
          <div>
            <h2 style={styles.sectionTitle}>AI Provider API Keys</h2>
            <div style={styles.hint}>
              These keys enable AI-powered text enhancement features (Phase 2).
              Keys are stored locally on your device.
            </div>

            <div style={{ marginTop: 16 }}>
              <label style={styles.fieldLabel}>OpenAI API Key</label>
              <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                <input
                  type={showOpenAI ? "text" : "password"}
                  value={settings.openai_api_key}
                  onChange={(e) => updateField("openai_api_key", e.target.value)}
                  placeholder="sk-..."
                  style={{ ...styles.input, flex: 1 }}
                />
                <button
                  onClick={() => setShowOpenAI(!showOpenAI)}
                  style={styles.actionBtn}
                >
                  {showOpenAI ? "Hide" : "Show"}
                </button>
              </div>
            </div>

            <div style={{ marginTop: 16 }}>
              <label style={styles.fieldLabel}>Anthropic API Key</label>
              <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                <input
                  type={showAnthropic ? "text" : "password"}
                  value={settings.anthropic_api_key}
                  onChange={(e) => updateField("anthropic_api_key", e.target.value)}
                  placeholder="sk-ant-..."
                  style={{ ...styles.input, flex: 1 }}
                />
                <button
                  onClick={() => setShowAnthropic(!showAnthropic)}
                  style={styles.actionBtn}
                >
                  {showAnthropic ? "Hide" : "Show"}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* ===== ABOUT ===== */}
        {tab === "about" && (
          <div>
            <h2 style={styles.sectionTitle}>Outspoken</h2>
            <div style={styles.aboutBlock}>
              <div style={styles.aboutRow}>
                <span style={styles.aboutLabel}>Version</span>
                <span>0.1.0</span>
              </div>
              <div style={styles.aboutRow}>
                <span style={styles.aboutLabel}>GitHub</span>
                <a
                  href="https://github.com/anthropics/outspoken"
                  target="_blank"
                  rel="noopener noreferrer"
                  style={styles.link}
                >
                  github.com/anthropics/outspoken
                </a>
              </div>
              <div style={styles.aboutRow}>
                <span style={styles.aboutLabel}>License</span>
                <span>MIT</span>
              </div>
            </div>
            <div style={{ marginTop: 16 }}>
              <h3 style={{ fontSize: "0.95rem", margin: "0 0 8px" }}>Open Source Libraries</h3>
              <div style={styles.hint}>
                whisper.cpp (MIT), Tauri (MIT/Apache-2.0), React (MIT),
                cpal (Apache-2.0), rusqlite (MIT), rubato (MIT)
              </div>
            </div>
          </div>
        )}
      </div>
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
  backBtn: {
    padding: "0.4rem 1rem",
    fontSize: "0.9rem",
    cursor: "pointer",
    borderRadius: 6,
    border: "1px solid #ccc",
    background: "#f5f5f5",
  },
  saveIndicator: {
    fontSize: "0.8rem",
    color: "#4caf50",
    fontWeight: 600,
  },
  error: {
    color: "#d32f2f",
    padding: "0.5rem 0.75rem",
    background: "#ffebee",
    borderRadius: 6,
    marginBottom: "1rem",
    fontSize: "0.85rem",
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
  },
  dismissBtn: {
    background: "none",
    border: "none",
    color: "#d32f2f",
    cursor: "pointer",
    fontSize: "0.9rem",
    padding: "0 4px",
  },
  tabBar: {
    display: "flex",
    gap: 2,
    borderBottom: "2px solid #e0e0e0",
    marginBottom: "1.5rem",
    flexWrap: "wrap" as const,
  },
  tabBtn: {
    padding: "0.5rem 0.85rem",
    fontSize: "0.85rem",
    cursor: "pointer",
    border: "none",
    background: "transparent",
    color: "#666",
    borderBottom: "2px solid transparent",
    marginBottom: -2,
    transition: "color 0.15s, border-color 0.15s",
  },
  tabBtnActive: {
    color: "#1976d2",
    borderBottomColor: "#1976d2",
    fontWeight: 600,
  },
  tabContent: {
    minHeight: 300,
  },
  sectionTitle: {
    fontSize: "1.1rem",
    marginBottom: "0.5rem",
    marginTop: 0,
  },
  select: {
    padding: "0.4rem",
    fontSize: "1rem",
    minWidth: 200,
    borderRadius: 4,
    border: "1px solid #ccc",
  },
  input: {
    padding: "0.4rem 0.5rem",
    fontSize: "1rem",
    minWidth: 200,
    borderRadius: 4,
    border: "1px solid #ccc",
  },
  hint: {
    fontSize: "0.85rem",
    color: "#666",
    marginTop: 4,
  },
  checkboxLabel: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    fontSize: "0.95rem",
    cursor: "pointer",
  },
  fieldRow: {
    display: "flex",
    alignItems: "center",
    gap: 12,
    marginBottom: 8,
  },
  fieldLabel: {
    fontSize: "0.9rem",
    fontWeight: 500,
    minWidth: 100,
  },
  fieldValue: {
    fontSize: "0.85rem",
    color: "#555",
    minWidth: 50,
    textAlign: "right" as const,
  },
  actionBtn: {
    padding: "0.35rem 0.75rem",
    fontSize: "0.85rem",
    cursor: "pointer",
    borderRadius: 4,
    border: "1px solid #1976d2",
    background: "#e3f2fd",
    color: "#1976d2",
  },
  dangerBtn: {
    padding: "0.35rem 0.75rem",
    fontSize: "0.85rem",
    cursor: "pointer",
    borderRadius: 4,
    border: "1px solid #d32f2f",
    background: "#ffebee",
    color: "#d32f2f",
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
  dictList: {
    display: "flex",
    flexWrap: "wrap" as const,
    gap: 6,
    marginTop: 12,
  },
  dictChip: {
    display: "inline-flex",
    alignItems: "center",
    gap: 4,
    padding: "4px 10px",
    background: "#e3f2fd",
    borderRadius: 16,
    fontSize: "0.85rem",
  },
  dictEntryRow: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    padding: "6px 10px",
    borderBottom: "1px solid #f0f0f0",
    fontSize: "0.9rem",
  },
  chipRemove: {
    background: "none",
    border: "none",
    cursor: "pointer",
    fontSize: "0.8rem",
    color: "#999",
    padding: "0 2px",
  },
  aboutBlock: {
    border: "1px solid #e0e0e0",
    borderRadius: 8,
    padding: "0.75rem 1rem",
  },
  aboutRow: {
    display: "flex",
    justifyContent: "space-between",
    padding: "6px 0",
    borderBottom: "1px solid #f0f0f0",
  },
  aboutLabel: {
    fontWeight: 500,
    color: "#555",
  },
  link: {
    color: "#1976d2",
    textDecoration: "none",
  },
};
