<script>
  // Bố cục trang "bảng + ngăn chi tiết bên phải" dùng chung (Tiến trình, Khởi động, Dịch vụ).
  // main: nội dung trang (tiêu đề, thanh lọc, bảng). inspector(overlay): ngăn chi tiết.
  // hasTarget: đang có đúng một hàng được chọn (dạng phủ chỉ mở khi có hàng).
  import { untrack } from 'svelte';
  import { t } from './i18n.svelte.js';
  let { insp, hasTarget = false, cls = '', main, inspector } = $props();

  // trang rộng ra -> ngăn nằm cạnh bảng, bỏ dạng phủ; bỏ chọn -> đóng dạng phủ
  $effect(() => {
    if (insp.shown || !hasTarget) untrack(() => { insp.overlay = false; });
  });

  function startResize(e) {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = insp.width;
    const tgt = e.currentTarget;
    tgt.setPointerCapture(e.pointerId);
    const move = (ev) => insp.setWidth(w0 - (ev.clientX - x0));
    const up = () => {
      tgt.removeEventListener('pointermove', move);
      tgt.removeEventListener('pointerup', up);
      tgt.removeEventListener('pointercancel', up);
      insp.saveWidth();
    };
    tgt.addEventListener('pointermove', move);
    tgt.addEventListener('pointerup', up);
    tgt.addEventListener('pointercancel', up);
  }
  function resizeKey(e) {
    const step = e.shiftKey ? 40 : 10;
    if (e.key === 'ArrowLeft') insp.setWidth(insp.width + step);
    else if (e.key === 'ArrowRight') insp.setWidth(insp.width - step);
    else return;
    e.preventDefault();
    insp.saveWidth();
  }
</script>

<section class="page split-page {cls}" bind:clientWidth={insp.pageW}>
  <div class="pp-main">
    {@render main()}
    {#if insp.overlay && !insp.shown && hasTarget}
      <div class="insp-overlay" style="width:{insp.overlayWidth}px">{@render inspector(true)}</div>
    {/if}
  </div>
  {#if insp.shown}
    <button type="button" class="insp-resizer" aria-label={t('shell.split.resize')} title={t('shell.split.resize_tip')} onpointerdown={startResize} onkeydown={resizeKey}></button>
    <div class="insp-wrap" style="width:{insp.width}px">{@render inspector(false)}</div>
  {/if}
</section>
