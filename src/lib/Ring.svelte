<script>
  // Vòng tròn % (SVG). Không có animation chạy liên tục.
  let { value = 0, color, size = 44, stroke = 5 } = $props();
  const r = $derived((size - stroke) / 2);
  const circ = $derived(2 * Math.PI * r);
  const v = $derived(Math.max(0, Math.min(100, value || 0)));
</script>

<svg class="ring" width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
  <circle cx={size / 2} cy={size / 2} {r} fill="none" stroke="rgba({color},.18)" stroke-width={stroke} />
  <circle cx={size / 2} cy={size / 2} {r} fill="none" stroke="rgb({color})" stroke-width={stroke} stroke-linecap="round"
    stroke-dasharray="{(v / 100) * circ} {circ}" transform="rotate(-90 {size / 2} {size / 2})" />
</svg>
