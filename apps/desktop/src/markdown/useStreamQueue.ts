import { useCallback, useEffect, useRef, useState } from "react";
import { countChars, type BlockInfo, type BlockState } from "./streaming";

const baseDelay = 18;
const accelerationFactor = 0.3;
const maxBlockDuration = 3000;
const fadeDuration = 280;

function computeCharDelay(queueLength: number, charCount: number): number {
  const acceleration = 1 + queueLength * accelerationFactor;
  return Math.min(baseDelay / acceleration, maxBlockDuration / Math.max(charCount, 1));
}

export function useStreamQueue(blocks: BlockInfo[]): {
  charDelay: number;
  getBlockState: (index: number) => BlockState;
} {
  const [revealedCount, setRevealedCount] = useState(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previousBlockCountRef = useRef(0);
  const minRevealedRef = useRef(0);

  if (blocks.length === 0 && previousBlockCountRef.current !== 0) {
    minRevealedRef.current = 0;
  }
  if (blocks.length > previousBlockCountRef.current && previousBlockCountRef.current > 0) {
    const previousTail = previousBlockCountRef.current - 1;
    minRevealedRef.current = Math.max(minRevealedRef.current, previousTail + 1);
  }
  previousBlockCountRef.current = blocks.length;

  useEffect(() => {
    if (blocks.length === 0) {
      setRevealedCount(0);
      minRevealedRef.current = 0;
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    }
  }, [blocks.length]);

  const effectiveRevealedCount = Math.max(revealedCount, minRevealedRef.current);
  const tailIndex = blocks.length - 1;

  const getBlockState = useCallback(
    (index: number): BlockState => {
      if (index < effectiveRevealedCount) {
        return "revealed";
      }
      if (index === effectiveRevealedCount && index < tailIndex) {
        return "animating";
      }
      if (index === effectiveRevealedCount && index === tailIndex) {
        return "streaming";
      }
      return "queued";
    },
    [effectiveRevealedCount, tailIndex],
  );

  const queueLength = Math.max(0, tailIndex - effectiveRevealedCount - 1);
  const animatingIndex = effectiveRevealedCount < tailIndex ? effectiveRevealedCount : -1;
  const animatingCharCount =
    animatingIndex >= 0 ? countChars(blocks[animatingIndex]?.content ?? "") : 0;
  const streamingIndex =
    animatingIndex < 0 && tailIndex >= effectiveRevealedCount ? tailIndex : -1;
  const activeIndex = animatingIndex >= 0 ? animatingIndex : streamingIndex;
  const activeCharCount = activeIndex >= 0 ? countChars(blocks[activeIndex]?.content ?? "") : 0;
  const frozenRef = useRef({ delay: baseDelay, index: -1 });

  if (activeIndex >= 0 && activeIndex !== frozenRef.current.index) {
    frozenRef.current = {
      delay: computeCharDelay(queueLength, activeCharCount),
      index: activeIndex,
    };
  }

  const charDelay = activeIndex >= 0 ? frozenRef.current.delay : baseDelay;
  const onAnimationDone = useCallback(() => {
    setRevealedCount(effectiveRevealedCount + 1);
  }, [effectiveRevealedCount]);

  useEffect(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }

    if (animatingIndex < 0) {
      return;
    }

    const totalTime = Math.max(0, (animatingCharCount - 1) * charDelay) + fadeDuration;
    timerRef.current = setTimeout(onAnimationDone, totalTime);
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [animatingCharCount, animatingIndex, charDelay, onAnimationDone]);

  return { charDelay, getBlockState };
}
