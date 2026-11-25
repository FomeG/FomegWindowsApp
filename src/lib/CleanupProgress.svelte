<script>
  // Thanh tiến độ dọn dẹp, nằm ngay trên thanh trạng thái, chỉ ở trang Dọn dẹp (App.svelte) và chỉ khi đang / vừa dọn.
  // Nhẹ: một dòng chữ + một thanh 4 px kéo bằng transform (không tính lại bố cục), cập nhật theo sự kiện của backend
  // (~4 lần/giây), không có hoạt ảnh chạy liên tục.
  import { cleanup } from './cleanup.svelte.js';
  import { fmtBytes, fmtInt } from './format.js';
  import { t } from './i18n.svelte.js';
  import Glyph from './Glyph.svelte';

  const p = $derived(cleanup.progress);
  const done = $derived(p?.stage === 'done');
  const measuring = $derived(p?.stage === 'measure');
  const known = $derived(!!p?.total_files);
  // đang xoá: không bao giờ báo 100% trước khi backend xong (tổng lấy từ lần quét, có thể lệch)
  const ratio = $derived(!p ? 0 : done || measuring ? 1 : known ? Math.min(0.99, p.done_files / p.total_files) : 0);
  const eta = $derived.by(() => {
    if (!p || done || measuring || !known || p.elapsed_ms < 2000 || p.done_files < 200) return '';
    const left = ((p.total_files - p.done_files) * p.elapsed_ms) / p.done_files / 1000;
    if (left <= 0) return '';
    return left < 60 ? t('cleanup.prog.eta_s', { n: Math.max(1, Math.round(left)) }) : t('cleanup.prog.eta_m', { n: Math.round(left / 60) });
  });
  const secs = (ms) => (ms < 10000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms / 1000)} s`);
  // t() có một tham số in đậm: trả [trước, sau] để vẽ {a}<b>{giá trị}</b>{b}
  const MARK = String.fromCharCode(1);
  const around = (key, name, params) => t(key, { ...params, [name]: MARK }).split(MARK);
</script>

{#if p}
  <div class="clean-prog" class:done role="status" aria-live="polite">
    <span class="cp-track" aria-hidden="true"><span class="cp-fill" style="transform:scaleX({ratio})"></span></span>
    <span class="cp-pct">{#if done}<Glyph name="check" size={11} />{:else if known}{Math.floor(ratio * 100)}%{:else}<span class="spin xs"></span>{/if}</span>
    <span class="cp-text ellipsis">
      {#if done}
        {@const d = around('cleanup.prog.done', 'bytes')}
        {d[0]}<b>{fmtBytes(p.freed_bytes)}</b>{d[1]} · {`${t('cleanup.prog.files', { count: p.removed, n: fmtInt(p.removed) })}${p.skipped ? ` · ${t('cleanup.prog.skipped', { n: fmtInt(p.skipped) })}` : ''} · ${secs(p.elapsed_ms)}`}
      {:else if measuring}
        {t('cleanup.prog.measuring')}
      {:else}
        {#if p.item}{@const c = around('cleanup.prog.cleaning_item', 'item')}{c[0]}<b>{p.item}</b>{c[1]}{:else}{t('cleanup.prog.cleaning')}{/if}
        {#if known} · {t('cleanup.prog.count', { done: fmtInt(p.done_files), total: fmtInt(p.total_files) })}{/if}
        {@const f = around('cleanup.prog.freed', 'bytes')}
        · {f[0]}<b>{fmtBytes(p.freed_bytes)}</b>{#if p.total_bytes}{` / ${fmtBytes(p.total_bytes)}`}{/if}{f[1]}
        {#if p.recycle} · {t('cleanup.prog.recycle')}{/if}
        {#if eta} · {eta}{/if}
      {/if}
    </span>
  </div>
{/if}
