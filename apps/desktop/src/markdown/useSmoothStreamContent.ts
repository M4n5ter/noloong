import { useCallback, useEffect, useRef, useState } from "react";
import { countChars, findOpenFenceLanguage, type StreamSmoothingPreset } from "./streaming";

type StreamSmoothingPresetConfig = {
  activeInputWindowMs: number;
  bypassFencedLanguages: readonly string[];
  defaultCps: number;
  emaAlpha: number;
  flushCps: number;
  maxActiveCps: number;
  maxFlushCps: number;
  minCps: number;
  shortContentChars: number;
  settleAfterMs: number;
  settleDrainMaxMs: number;
  settleDrainMinMs: number;
  targetBufferMs: number;
};

const presetConfig: Record<StreamSmoothingPreset, StreamSmoothingPresetConfig> = {
  balanced: {
    activeInputWindowMs: 220,
    bypassFencedLanguages: ["html"],
    defaultCps: 38,
    emaAlpha: 0.2,
    flushCps: 120,
    maxActiveCps: 132,
    maxFlushCps: 280,
    minCps: 18,
    shortContentChars: 64,
    settleAfterMs: 360,
    settleDrainMaxMs: 520,
    settleDrainMinMs: 180,
    targetBufferMs: 120,
  },
  realtime: {
    activeInputWindowMs: 140,
    bypassFencedLanguages: ["html"],
    defaultCps: 50,
    emaAlpha: 0.3,
    flushCps: 170,
    maxActiveCps: 180,
    maxFlushCps: 360,
    minCps: 24,
    shortContentChars: 64,
    settleAfterMs: 260,
    settleDrainMaxMs: 360,
    settleDrainMinMs: 140,
    targetBufferMs: 40,
  },
  silky: {
    activeInputWindowMs: 320,
    bypassFencedLanguages: ["html"],
    defaultCps: 28,
    emaAlpha: 0.14,
    flushCps: 96,
    maxActiveCps: 102,
    maxFlushCps: 220,
    minCps: 14,
    shortContentChars: 64,
    settleAfterMs: 460,
    settleDrainMaxMs: 680,
    settleDrainMinMs: 240,
    targetBufferMs: 170,
  },
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function now(): number {
  return typeof performance === "undefined" ? Date.now() : performance.now();
}

export function useSmoothStreamContent(
  content: string,
  { enabled = true, preset = "balanced" }: { enabled?: boolean; preset?: StreamSmoothingPreset } = {},
): string {
  const config = presetConfig[preset];
  const [displayedContent, setDisplayedContent] = useState(content);

  const displayedContentRef = useRef(content);
  const displayedCountRef = useRef(countChars(content));
  const targetContentRef = useRef(content);
  const targetCharsRef = useRef([...content]);
  const targetCountRef = useRef(targetCharsRef.current.length);
  const emaCpsRef = useRef(config.defaultCps);
  const lastInputTsRef = useRef(0);
  const lastInputCountRef = useRef(targetCountRef.current);
  const chunkSizeEmaRef = useRef(1);
  const arrivalCpsEmaRef = useRef(config.defaultCps);
  const rafRef = useRef<number | null>(null);
  const lastFrameTsRef = useRef<number | null>(null);
  const wakeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hasStreamingHistoryRef = useRef(enabled);

  const clearWakeTimer = useCallback(() => {
    if (wakeTimerRef.current !== null) {
      clearTimeout(wakeTimerRef.current);
      wakeTimerRef.current = null;
    }
  }, []);

  const stopFrameLoop = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    lastFrameTsRef.current = null;
  }, []);

  const stopScheduling = useCallback(() => {
    stopFrameLoop();
    clearWakeTimer();
  }, [clearWakeTimer, stopFrameLoop]);

  const syncImmediate = useCallback(
    (nextContent: string) => {
      stopScheduling();
      const chars = [...nextContent];
      const currentTime = now();

      targetContentRef.current = nextContent;
      targetCharsRef.current = chars;
      targetCountRef.current = chars.length;
      displayedContentRef.current = nextContent;
      displayedCountRef.current = chars.length;
      setDisplayedContent(nextContent);
      emaCpsRef.current = config.defaultCps;
      chunkSizeEmaRef.current = 1;
      arrivalCpsEmaRef.current = config.defaultCps;
      lastInputTsRef.current = currentTime;
      lastInputCountRef.current = chars.length;
    },
    [config.defaultCps, stopScheduling],
  );

  const startFrameLoopRef = useRef<() => void>(() => {});

  const scheduleFrameWake = useCallback(
    (delayMs: number) => {
      clearWakeTimer();
      wakeTimerRef.current = setTimeout(() => {
        wakeTimerRef.current = null;
        startFrameLoopRef.current();
      }, Math.max(1, Math.ceil(delayMs)));
    },
    [clearWakeTimer],
  );

  const startFrameLoop = useCallback(() => {
    clearWakeTimer();
    if (rafRef.current !== null) {
      return;
    }

    const tick = (timestamp: number) => {
      if (lastFrameTsRef.current === null) {
        lastFrameTsRef.current = timestamp;
        rafRef.current = requestAnimationFrame(tick);
        return;
      }

      const frameIntervalMs = Math.max(0, timestamp - lastFrameTsRef.current);
      const dtSeconds = Math.max(0.001, Math.min(frameIntervalMs / 1000, 0.05));
      lastFrameTsRef.current = timestamp;

      const targetCount = targetCountRef.current;
      const displayedCount = displayedCountRef.current;
      const backlog = targetCount - displayedCount;
      if (backlog <= 0) {
        stopFrameLoop();
        return;
      }

      const currentTime = now();
      const idleMs = currentTime - lastInputTsRef.current;
      const inputActive = idleMs <= config.activeInputWindowMs;
      const settling = !inputActive && idleMs >= config.settleAfterMs;
      const baseCps = clamp(emaCpsRef.current, config.minCps, config.maxFlushCps);
      const baseLagChars = Math.max(1, Math.round((baseCps * config.targetBufferMs) / 1000));
      const lagUpperBound = Math.max(baseLagChars + 2, baseLagChars * 3);
      const targetLagChars = inputActive
        ? Math.round(
            clamp(baseLagChars + chunkSizeEmaRef.current * 0.35, baseLagChars, lagUpperBound),
          )
        : 0;
      const desiredDisplayed = Math.max(0, targetCount - targetLagChars);

      let currentCps = baseCps;
      if (inputActive) {
        const backlogPressure = targetLagChars > 0 ? backlog / targetLagChars : 1;
        const chunkPressure = targetLagChars > 0 ? chunkSizeEmaRef.current / targetLagChars : 1;
        const arrivalPressure = arrivalCpsEmaRef.current / Math.max(baseCps, 1);
        const combinedPressure = clamp(
          backlogPressure * 0.6 + chunkPressure * 0.25 + arrivalPressure * 0.15,
          1,
          4.5,
        );
        const activeCap = clamp(
          config.maxActiveCps + chunkSizeEmaRef.current * 6,
          config.maxActiveCps,
          config.maxFlushCps,
        );
        currentCps = clamp(baseCps * combinedPressure, config.minCps, activeCap);
      } else if (settling) {
        const drainTargetMs = clamp(backlog * 8, config.settleDrainMinMs, config.settleDrainMaxMs);
        currentCps = clamp((backlog * 1000) / drainTargetMs, config.flushCps, config.maxFlushCps);
      } else {
        currentCps = clamp(
          Math.max(config.flushCps, baseCps * 1.8, arrivalCpsEmaRef.current * 0.8),
          config.flushCps,
          config.maxFlushCps,
        );
      }

      let revealChars = Math.max(inputActive ? 1 : 2, Math.round(currentCps * dtSeconds));
      if (inputActive) {
        const shortfall = desiredDisplayed - displayedCount;
        if (shortfall <= 0) {
          stopFrameLoop();
          scheduleFrameWake(config.activeInputWindowMs - idleMs);
          return;
        }
        revealChars = Math.min(revealChars, shortfall, backlog);
      } else {
        revealChars = Math.min(revealChars, backlog);
      }

      const nextCount = displayedCount + revealChars;
      const segment = targetCharsRef.current.slice(displayedCount, nextCount).join("");
      if (segment.length > 0) {
        const nextDisplayed = displayedContentRef.current + segment;
        displayedContentRef.current = nextDisplayed;
        displayedCountRef.current = nextCount;
        setDisplayedContent(nextDisplayed);
      } else {
        displayedContentRef.current = targetContentRef.current;
        displayedCountRef.current = targetCount;
        setDisplayedContent(targetContentRef.current);
      }

      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
  }, [
    clearWakeTimer,
    config.activeInputWindowMs,
    config.flushCps,
    config.maxActiveCps,
    config.maxFlushCps,
    config.minCps,
    config.settleAfterMs,
    config.settleDrainMaxMs,
    config.settleDrainMinMs,
    config.targetBufferMs,
    scheduleFrameWake,
    stopFrameLoop,
  ]);

  startFrameLoopRef.current = startFrameLoop;

  const resetTargetToDisplayed = useCallback(() => {
    const displayedChars = [...displayedContentRef.current];
    targetContentRef.current = displayedContentRef.current;
    targetCharsRef.current = displayedChars;
    targetCountRef.current = displayedChars.length;
    lastInputCountRef.current = displayedChars.length;
  }, []);

  const queueTargetContent = useCallback(
    (nextContent: string, { recordInput }: { recordInput: boolean }) => {
      const previous = targetContentRef.current;
      if (nextContent === previous) {
        startFrameLoop();
        return;
      }

      if (!nextContent.startsWith(previous)) {
        syncImmediate(nextContent);
        return;
      }

      const appendedChars = [...nextContent.slice(previous.length)];
      const currentTime = now();
      targetContentRef.current = nextContent;
      appendChars(targetCharsRef.current, appendedChars);
      targetCountRef.current += appendedChars.length;

      const deltaChars = targetCountRef.current - lastInputCountRef.current;
      const deltaMs = Math.max(1, currentTime - lastInputTsRef.current);
      if (recordInput && deltaChars > 0) {
        const instantCps = (deltaChars * 1000) / deltaMs;
        const normalizedInstantCps = clamp(instantCps, config.minCps, config.maxFlushCps * 2);
        const chunkEmaAlpha = 0.35;
        chunkSizeEmaRef.current =
          chunkSizeEmaRef.current * (1 - chunkEmaAlpha) + appendedChars.length * chunkEmaAlpha;
        arrivalCpsEmaRef.current =
          arrivalCpsEmaRef.current * (1 - chunkEmaAlpha) +
          normalizedInstantCps * chunkEmaAlpha;
        const clampedCps = clamp(instantCps, config.minCps, config.maxActiveCps);
        emaCpsRef.current =
          emaCpsRef.current * (1 - config.emaAlpha) + clampedCps * config.emaAlpha;
        lastInputTsRef.current = currentTime;
        lastInputCountRef.current = targetCountRef.current;
      } else if (!recordInput) {
        lastInputTsRef.current = currentTime - config.settleAfterMs;
      }

      startFrameLoop();
    },
    [
      config.emaAlpha,
      config.maxActiveCps,
      config.maxFlushCps,
      config.minCps,
      config.settleAfterMs,
      startFrameLoop,
      syncImmediate,
    ],
  );

  useEffect(() => {
    if (enabled) {
      hasStreamingHistoryRef.current = true;
    }

    if (!enabled) {
      const canSettleFinalContent =
        hasStreamingHistoryRef.current &&
        content.startsWith(displayedContentRef.current) &&
        content !== displayedContentRef.current;

      if (canSettleFinalContent) {
        if (!content.startsWith(targetContentRef.current)) {
          resetTargetToDisplayed();
        }
        queueTargetContent(content, { recordInput: false });
        return;
      }

      syncImmediate(content);
      return;
    }

    const previous = targetContentRef.current;
    if (content === previous) {
      return;
    }
    if (!content.startsWith(previous)) {
      syncImmediate(content);
      return;
    }

    if (countChars(content) <= config.shortContentChars) {
      syncImmediate(content);
      return;
    }

    const openLanguage = findOpenFenceLanguage(content);
    if (openLanguage !== null && config.bypassFencedLanguages.includes(openLanguage)) {
      syncImmediate(content);
      return;
    }

    queueTargetContent(content, { recordInput: true });
  }, [
    config.bypassFencedLanguages,
    config.shortContentChars,
    content,
    enabled,
    queueTargetContent,
    resetTargetToDisplayed,
    syncImmediate,
  ]);

  useEffect(() => stopScheduling, [stopScheduling]);

  return displayedContent;
}

function appendChars(target: string[], appended: string[]): void {
  for (const char of appended) {
    target.push(char);
  }
}
