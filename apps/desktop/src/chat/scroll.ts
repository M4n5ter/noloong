export function isNearTranscriptBottom(
  metrics: Pick<Element, "clientHeight" | "scrollHeight" | "scrollTop">,
  thresholdPx = 96,
): boolean {
  return metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight <= thresholdPx;
}

export function scrollTranscriptToEnd(transcript: HTMLDivElement): void {
  transcript.scrollTop = transcript.scrollHeight;
}
