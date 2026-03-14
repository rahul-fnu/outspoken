import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

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

function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(0)} MB`;
  return `${(bytes / 1_000).toFixed(0)} KB`;
}

function App() {
  const [availableModels, setAvailableModels] = useState<ModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<DownloadedModel[]>([]);
  const [downloading, setDownloading] = useState<Record<string, DownloadProgress>>({});
  const [error, setError] = useState<string | null>(null);
  const [languages, setLanguages] = useState<SupportedLanguage[]>([]);
  const [selectedLanguage, setSelectedLanguage] = useState<string>("auto");

  const loadLanguages = useCallback(async () => {
    try {
      const langs = await invoke<SupportedLanguage[]>("list_supported_languages");
      setLanguages(langs);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    loadLanguages();
  }, [loadLanguages]);

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
    loadModels();
  }, [loadModels]);

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
      await loadModels();
    } catch (e) {
      setError(String(e));
    }
  }

  const downloadedNames = new Set(downloadedModels.map((m) => m.name));

  return (
    <main style={{ padding: "2rem", fontFamily: "sans-serif", maxWidth: 600 }}>
      <h1>Outspoken</h1>
      <p>AI-powered dictation, right on your desktop.</p>

      {error && (
        <div style={{ color: "red", marginBottom: "1rem" }}>{error}</div>
      )}

      <h2>Language</h2>
      <div style={{ marginBottom: "1.5rem" }}>
        <select
          value={selectedLanguage}
          onChange={(e) => setSelectedLanguage(e.target.value)}
          style={{ padding: "0.4rem", fontSize: "1rem", minWidth: 200 }}
        >
          <option value="auto">Auto-detect</option>
          {languages.map((lang) => (
            <option key={lang.code} value={lang.code}>
              {lang.name} ({lang.code})
            </option>
          ))}
        </select>
        <div style={{ fontSize: "0.85rem", color: "#666", marginTop: 4 }}>
          {selectedLanguage === "auto"
            ? "Whisper will automatically detect the spoken language."
            : `Manual selection: ${selectedLanguage}`}
        </div>
      </div>

      <h2>Whisper Models</h2>

      {availableModels.map((model) => {
        const isDownloaded = downloadedNames.has(model.name);
        const progress = downloading[model.name];
        const isDownloading = progress?.status === "Downloading";

        return (
          <div
            key={model.name}
            style={{
              border: "1px solid #ccc",
              borderRadius: 8,
              padding: "1rem",
              marginBottom: "0.75rem",
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <div>
                <strong>{model.name}</strong>
                <span style={{ marginLeft: 8, color: "#666" }}>
                  {formatBytes(model.size_bytes)}
                </span>
              </div>
              <div>
                {isDownloaded && !isDownloading && (
                  <button onClick={() => handleDelete(model.name)} style={{ marginLeft: 8 }}>
                    Delete
                  </button>
                )}
                {!isDownloaded && !isDownloading && (
                  <button onClick={() => handleDownload(model.name)}>Download</button>
                )}
                {isDownloading && (
                  <button onClick={() => handleCancel(model.name)}>Cancel</button>
                )}
              </div>
            </div>
            <div style={{ fontSize: "0.85rem", color: "#666", marginTop: 4 }}>
              {model.description}
            </div>
            {isDownloading && progress && (
              <div style={{ marginTop: 8 }}>
                <div
                  style={{
                    background: "#eee",
                    borderRadius: 4,
                    height: 8,
                    overflow: "hidden",
                  }}
                >
                  <div
                    style={{
                      background: "#4caf50",
                      height: "100%",
                      width: `${progress.progress_percent}%`,
                      transition: "width 0.3s",
                    }}
                  />
                </div>
                <div style={{ fontSize: "0.8rem", marginTop: 4 }}>
                  {formatBytes(progress.downloaded_bytes)} / {formatBytes(progress.total_bytes)} (
                  {progress.progress_percent.toFixed(1)}%)
                </div>
              </div>
            )}
            {isDownloaded && (
              <div style={{ fontSize: "0.8rem", color: "green", marginTop: 4 }}>
                Downloaded
              </div>
            )}
          </div>
        );
      })}
    </main>
  );
}

export default App;
