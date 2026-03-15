import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Transcription {
  id: string;
  text: string;
  raw_text: string;
  timestamp: number;
  duration_ms: number | null;
  source_app: string | null;
  language: string | null;
  model_used: string | null;
  is_bookmarked: boolean;
}

interface SearchFilters {
  query?: string;
  source_app?: string;
  date_from?: number;
  date_to?: number;
  bookmarked_only?: boolean;
  limit?: number;
  offset?: number;
}

interface HistoryStats {
  total_count: number;
  source_apps: string[];
}

interface HistoryProps {
  onBack: () => void;
}

const PAGE_SIZE = 50;

function formatTimestamp(ts: number): string {
  try {
    const d = new Date(ts * 1000);
    return d.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return String(ts);
  }
}

function formatDuration(ms: number | null): string {
  if (ms == null) return "";
  const secs = ms / 1000;
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return `${m}m ${s}s`;
}

function truncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + "...";
}

function dateToTimestamp(dateStr: string, endOfDay = false): number | undefined {
  if (!dateStr) return undefined;
  const d = new Date(dateStr);
  if (endOfDay) {
    d.setHours(23, 59, 59, 999);
  }
  return Math.floor(d.getTime() / 1000);
}

export default function History({ onBack }: HistoryProps) {
  const [entries, setEntries] = useState<Transcription[]>([]);
  const [stats, setStats] = useState<HistoryStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [sourceAppFilter, setSourceAppFilter] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [bookmarkedOnly, setBookmarkedOnly] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(true);
  const [exportFormat, setExportFormat] = useState("txt");
  const [error, setError] = useState<string | null>(null);
  const searchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const buildFilters = useCallback(
    (offset = 0): SearchFilters => ({
      query: search || undefined,
      source_app: sourceAppFilter || undefined,
      date_from: dateToTimestamp(dateFrom),
      date_to: dateToTimestamp(dateTo, true),
      bookmarked_only: bookmarkedOnly || undefined,
      limit: PAGE_SIZE,
      offset,
    }),
    [search, sourceAppFilter, dateFrom, dateTo, bookmarkedOnly]
  );

  const loadEntries = useCallback(
    async (append = false) => {
      setError(null);
      setLoading(true);
      try {
        const offset = append ? entries.length : 0;
        const filters = buildFilters(offset);
        const result = await invoke<Transcription[]>("search_history", {
          query: filters.query,
          filters,
        });
        if (append) {
          setEntries((prev) => [...prev, ...result]);
        } else {
          setEntries(result);
        }
        setHasMore(result.length === PAGE_SIZE);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [buildFilters, entries.length]
  );

  const loadStats = useCallback(async () => {
    try {
      const result = await invoke<HistoryStats>("get_history_stats");
      setStats(result);
    } catch {
      // non-critical
    }
  }, []);

  // Initial load
  useEffect(() => {
    loadEntries();
    loadStats();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Reload on filter changes (debounced for search)
  useEffect(() => {
    if (searchTimeoutRef.current) clearTimeout(searchTimeoutRef.current);
    searchTimeoutRef.current = setTimeout(() => {
      loadEntries();
    }, 300);
    return () => {
      if (searchTimeoutRef.current) clearTimeout(searchTimeoutRef.current);
    };
  }, [search, sourceAppFilter, dateFrom, dateTo, bookmarkedOnly]); // eslint-disable-line react-hooks/exhaustive-deps

  async function handleDelete(id: string) {
    try {
      await invoke("delete_transcription", { id });
      setEntries((prev) => prev.filter((e) => e.id !== id));
      if (expandedId === id) setExpandedId(null);
      loadStats();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleToggleBookmark(id: string) {
    try {
      const isNowBookmarked = await invoke<boolean>("toggle_bookmark", { id });
      setEntries((prev) =>
        prev.map((e) =>
          e.id === id ? { ...e, is_bookmarked: isNowBookmarked } : e
        )
      );
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleClearHistory() {
    if (!confirm("Are you sure you want to delete all transcription history? This cannot be undone.")) {
      return;
    }
    try {
      await invoke("clear_history");
      setEntries([]);
      setExpandedId(null);
      loadStats();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleCopy(text: string, id: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setCopied(id);
    setTimeout(() => setCopied(null), 2000);
  }

  async function handleExport() {
    setError(null);
    try {
      const filters: SearchFilters = {
        query: search || undefined,
        source_app: sourceAppFilter || undefined,
        date_from: dateToTimestamp(dateFrom),
        date_to: dateToTimestamp(dateTo, true),
        bookmarked_only: bookmarkedOnly || undefined,
        limit: 10000,
        offset: 0,
      };
      const content = await invoke<string>("export_history", {
        filters,
        format: exportFormat,
      });
      // Trigger download via blob
      const mimeTypes: Record<string, string> = {
        txt: "text/plain",
        json: "application/json",
        csv: "text/csv",
      };
      const blob = new Blob([content], { type: mimeTypes[exportFormat] || "text/plain" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `outspoken-history.${exportFormat}`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(String(e));
    }
  }

  // Virtualized list: render only visible items
  const listRef = useRef<HTMLDivElement>(null);
  const ITEM_HEIGHT = 72;
  const [scrollTop, setScrollTop] = useState(0);
  const visibleCount = 12;

  const handleScroll = () => {
    if (listRef.current) {
      setScrollTop(listRef.current.scrollTop);
    }
  };

  const startIndex = Math.max(0, Math.floor(scrollTop / ITEM_HEIGHT) - 2);
  const endIndex = Math.min(entries.length, startIndex + visibleCount + 4);
  const visibleEntries = entries.slice(startIndex, endIndex);
  const totalHeight = entries.length * ITEM_HEIGHT;

  return (
    <div style={styles.container}>
      {/* Header */}
      <div style={styles.header}>
        <button onClick={onBack} style={styles.backBtn}>
          Back
        </button>
        <h2 style={styles.title}>History</h2>
        <span style={styles.count}>
          {stats ? `${stats.total_count} total` : ""}
        </span>
        {stats && stats.total_count > 0 && (
          <button onClick={handleClearHistory} style={styles.clearBtn}>
            Clear All
          </button>
        )}
      </div>

      {/* Search bar */}
      <div style={styles.searchBar}>
        <input
          type="text"
          placeholder="Search transcriptions..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={styles.searchInput}
        />
      </div>

      {/* Filters row */}
      <div style={styles.filtersRow}>
        <div style={styles.filterGroup}>
          <label style={styles.filterLabel}>Source App</label>
          <select
            value={sourceAppFilter}
            onChange={(e) => setSourceAppFilter(e.target.value)}
            style={styles.filterSelect}
          >
            <option value="">All Apps</option>
            {stats?.source_apps.map((app) => (
              <option key={app} value={app}>
                {app}
              </option>
            ))}
          </select>
        </div>
        <div style={styles.filterGroup}>
          <label style={styles.filterLabel}>From</label>
          <input
            type="date"
            value={dateFrom}
            onChange={(e) => setDateFrom(e.target.value)}
            style={styles.filterInput}
          />
        </div>
        <div style={styles.filterGroup}>
          <label style={styles.filterLabel}>To</label>
          <input
            type="date"
            value={dateTo}
            onChange={(e) => setDateTo(e.target.value)}
            style={styles.filterInput}
          />
        </div>
        <div style={styles.filterGroup}>
          <label style={styles.filterLabel}>
            <input
              type="checkbox"
              checked={bookmarkedOnly}
              onChange={(e) => setBookmarkedOnly(e.target.checked)}
              style={{ marginRight: 4 }}
            />
            Bookmarked
          </label>
        </div>
        <div style={styles.filterGroup}>
          <label style={styles.filterLabel}>Export</label>
          <div style={{ display: "flex", gap: 4 }}>
            <select
              value={exportFormat}
              onChange={(e) => setExportFormat(e.target.value)}
              style={styles.filterSelect}
            >
              <option value="txt">TXT</option>
              <option value="json">JSON</option>
              <option value="csv">CSV</option>
            </select>
            <button onClick={handleExport} style={styles.exportBtn}>
              Export
            </button>
          </div>
        </div>
      </div>

      {error && <div style={styles.error}>{error}</div>}

      {/* Entry list (virtualized) */}
      <div
        ref={listRef}
        onScroll={handleScroll}
        style={styles.listContainer}
      >
        <div style={{ height: totalHeight, position: "relative" }}>
          {visibleEntries.map((entry, i) => {
            const isExpanded = expandedId === entry.id;
            const actualIndex = startIndex + i;
            return (
              <div
                key={entry.id}
                style={{
                  position: isExpanded ? "relative" : "absolute",
                  top: isExpanded ? undefined : actualIndex * ITEM_HEIGHT,
                  left: 0,
                  right: 0,
                  ...(isExpanded ? {} : { height: ITEM_HEIGHT }),
                }}
              >
                <div
                  style={{
                    ...styles.entryItem,
                    ...(isExpanded ? styles.entryExpanded : {}),
                  }}
                  onClick={() => setExpandedId(isExpanded ? null : entry.id)}
                >
                  {/* Summary row */}
                  <div style={styles.entrySummary}>
                    <div style={styles.entryMeta}>
                      <span style={styles.entryDate}>{formatTimestamp(entry.timestamp)}</span>
                      {entry.source_app && (
                        <span style={styles.entryApp}>{entry.source_app}</span>
                      )}
                      {entry.duration_ms != null && (
                        <span style={styles.entryDuration}>
                          {formatDuration(entry.duration_ms)}
                        </span>
                      )}
                      {entry.is_bookmarked && (
                        <span style={styles.bookmarkIndicator}>*</span>
                      )}
                    </div>
                    <div style={styles.entryPreview}>
                      {isExpanded ? "" : truncate(entry.text, 100)}
                    </div>
                  </div>

                  {/* Expanded detail */}
                  {isExpanded && (
                    <div
                      style={styles.entryDetail}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <div style={styles.entryFullText}>{entry.text}</div>
                      <div style={styles.entryActions}>
                        <button
                          onClick={() => handleCopy(entry.text, entry.id)}
                          style={styles.actionBtn}
                        >
                          {copied === entry.id ? "Copied!" : "Copy"}
                        </button>
                        <button
                          onClick={() => handleToggleBookmark(entry.id)}
                          style={styles.actionBtn}
                        >
                          {entry.is_bookmarked ? "Unbookmark" : "Bookmark"}
                        </button>
                        <button
                          onClick={() => handleDelete(entry.id)}
                          style={{ ...styles.actionBtn, ...styles.deleteBtn }}
                        >
                          Delete
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        {/* Load more */}
        {hasMore && entries.length > 0 && (
          <div style={styles.loadMore}>
            <button
              onClick={() => loadEntries(true)}
              style={styles.loadMoreBtn}
              disabled={loading}
            >
              {loading ? "Loading..." : "Load More"}
            </button>
          </div>
        )}
      </div>

      {/* Empty state */}
      {!loading && entries.length === 0 && (
        <div style={styles.emptyState}>
          {search || sourceAppFilter || dateFrom || dateTo || bookmarkedOnly
            ? "No transcriptions match your filters."
            : "No transcriptions yet. Start recording to build your history."}
        </div>
      )}

      {loading && entries.length === 0 && (
        <div style={styles.loadingState}>Loading history...</div>
      )}
    </div>
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
    display: "flex",
    flexDirection: "column",
  },
  header: {
    display: "flex",
    alignItems: "center",
    gap: "0.75rem",
    marginBottom: "1rem",
  },
  backBtn: {
    padding: "0.4rem 0.75rem",
    fontSize: "0.85rem",
    cursor: "pointer",
    borderRadius: 6,
    border: "1px solid #ccc",
    background: "#f5f5f5",
  },
  clearBtn: {
    padding: "0.4rem 0.75rem",
    fontSize: "0.8rem",
    cursor: "pointer",
    borderRadius: 6,
    border: "1px solid #ef5350",
    background: "#fff",
    color: "#d32f2f",
  },
  title: {
    margin: 0,
    fontSize: "1.25rem",
    flex: 1,
  },
  count: {
    fontSize: "0.8rem",
    color: "#888",
  },
  searchBar: {
    marginBottom: "0.75rem",
  },
  searchInput: {
    width: "100%",
    padding: "0.6rem 0.75rem",
    fontSize: "0.9rem",
    border: "1px solid #ddd",
    borderRadius: 6,
    boxSizing: "border-box",
    outline: "none",
  },
  filtersRow: {
    display: "flex",
    gap: "0.75rem",
    marginBottom: "1rem",
    flexWrap: "wrap",
    alignItems: "flex-end",
  },
  filterGroup: {
    display: "flex",
    flexDirection: "column",
    gap: 2,
    flex: "1 1 auto",
    minWidth: 100,
  },
  filterLabel: {
    fontSize: "0.7rem",
    color: "#888",
    textTransform: "uppercase",
    letterSpacing: "0.5px",
  },
  filterSelect: {
    padding: "0.35rem 0.5rem",
    fontSize: "0.8rem",
    border: "1px solid #ddd",
    borderRadius: 4,
    background: "#fff",
  },
  filterInput: {
    padding: "0.35rem 0.5rem",
    fontSize: "0.8rem",
    border: "1px solid #ddd",
    borderRadius: 4,
  },
  exportBtn: {
    padding: "0.35rem 0.75rem",
    fontSize: "0.8rem",
    cursor: "pointer",
    borderRadius: 4,
    border: "1px solid #1976d2",
    background: "#1976d2",
    color: "#fff",
    whiteSpace: "nowrap",
  },
  error: {
    color: "#d32f2f",
    padding: "0.5rem 0.75rem",
    background: "#ffebee",
    borderRadius: 6,
    marginBottom: "0.75rem",
    fontSize: "0.85rem",
  },
  listContainer: {
    flex: 1,
    overflowY: "auto",
    minHeight: 300,
    maxHeight: "calc(100vh - 280px)",
  },
  entryItem: {
    padding: "0.6rem 0.75rem",
    borderBottom: "1px solid #eee",
    cursor: "pointer",
    transition: "background 0.15s",
  },
  entryExpanded: {
    background: "#f8f9fa",
    borderRadius: 6,
    border: "1px solid #e0e0e0",
    marginBottom: 4,
  },
  entrySummary: {
    display: "flex",
    flexDirection: "column",
    gap: 4,
  },
  entryMeta: {
    display: "flex",
    gap: "0.75rem",
    fontSize: "0.75rem",
    color: "#888",
  },
  entryDate: {
    fontWeight: 500,
  },
  entryApp: {
    background: "#e3f2fd",
    color: "#1565c0",
    padding: "0 6px",
    borderRadius: 3,
    fontSize: "0.7rem",
  },
  entryDuration: {
    color: "#999",
  },
  bookmarkIndicator: {
    color: "#f9a825",
    fontWeight: "bold",
  },
  entryPreview: {
    fontSize: "0.85rem",
    color: "#444",
    lineHeight: 1.4,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  entryDetail: {
    marginTop: "0.5rem",
  },
  entryFullText: {
    fontSize: "0.9rem",
    lineHeight: 1.6,
    color: "#333",
    whiteSpace: "pre-wrap",
    padding: "0.5rem",
    background: "#fff",
    borderRadius: 4,
    border: "1px solid #e8e8e8",
    maxHeight: 300,
    overflowY: "auto",
  },
  entryActions: {
    display: "flex",
    gap: "0.5rem",
    marginTop: "0.5rem",
  },
  actionBtn: {
    padding: "0.3rem 0.75rem",
    fontSize: "0.8rem",
    cursor: "pointer",
    borderRadius: 4,
    border: "1px solid #ccc",
    background: "#f5f5f5",
  },
  deleteBtn: {
    borderColor: "#ef5350",
    color: "#d32f2f",
    background: "#fff",
  },
  loadMore: {
    display: "flex",
    justifyContent: "center",
    padding: "1rem",
  },
  loadMoreBtn: {
    padding: "0.4rem 1.5rem",
    fontSize: "0.85rem",
    cursor: "pointer",
    borderRadius: 6,
    border: "1px solid #ccc",
    background: "#f5f5f5",
  },
  emptyState: {
    textAlign: "center",
    color: "#999",
    padding: "3rem 1rem",
    fontSize: "0.9rem",
    fontStyle: "italic",
  },
  loadingState: {
    textAlign: "center",
    color: "#888",
    padding: "2rem 1rem",
    fontSize: "0.9rem",
  },
};
