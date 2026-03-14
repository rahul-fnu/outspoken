import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

interface AudioLevelPayload {
  level_db: number;
  level_rms: number;
}

interface UseAudioLevelOptions {
  onSilenceDetected?: () => void;
}

export function useAudioLevel(options: UseAudioLevelOptions = {}) {
  const [levelDb, setLevelDb] = useState(-60);
  const [levelRms, setLevelRms] = useState(0);
  const onSilenceRef = useRef(options.onSilenceDetected);
  onSilenceRef.current = options.onSilenceDetected;

  useEffect(() => {
    const unlistenLevel = listen<AudioLevelPayload>("audio-level", (event) => {
      setLevelDb(event.payload.level_db);
      setLevelRms(event.payload.level_rms);
    });

    const unlistenSilence = listen("silence-detected", () => {
      onSilenceRef.current?.();
    });

    return () => {
      unlistenLevel.then((fn) => fn());
      unlistenSilence.then((fn) => fn());
    };
  }, []);

  const setSilenceConfig = useCallback(
    async (thresholdDb: number, durationSecs: number) => {
      await invoke("set_silence_config", { thresholdDb, durationSecs });
    },
    []
  );

  return { levelDb, levelRms, setSilenceConfig };
}
