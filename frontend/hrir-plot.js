const PLOT_WIDTH = 260;
const PLOT_HEIGHT = 112;
const PLOT_PADDING = 6;

export function sharedAbsolutePeak(left, right) {
  let peak = 0;
  for (const channel of [left, right]) {
    for (const sample of channel) {
      if (Number.isFinite(sample)) peak = Math.max(peak, Math.abs(sample));
    }
  }
  return peak;
}

export function createHrirPath(samples, peak) {
  if (samples.length === 0 || !Number.isFinite(peak) || peak <= 0) return "";
  const center = PLOT_HEIGHT / 2;
  const amplitude = center - PLOT_PADDING;
  const last = Math.max(1, samples.length - 1);
  return Array.from(samples, (sample, index) => {
    const x = index * PLOT_WIDTH / last;
    const finiteSample = Number.isFinite(sample) ? sample : 0;
    const y = center - Math.max(-1, Math.min(1, finiteSample / peak)) * amplitude;
    return `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`;
  }).join(" ");
}

export function renderHrirPlot(elements, left, right, sampleRate) {
  const peak = sharedAbsolutePeak(left, right);
  elements.leftPath.setAttribute("d", createHrirPath(left, peak));
  elements.rightPath.setAttribute("d", createHrirPath(right, peak));
  elements.length.value = `${left.length} samples`;
  const durationMilliseconds = left.length / sampleRate * 1000;
  elements.duration.textContent = `${durationMilliseconds.toFixed(2)} ms`;
}
