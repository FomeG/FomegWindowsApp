<script>
  // Popup chung xem lại đề xuất của trợ lý AI (xoá thư mục, đóng cổng…): mỗi mục một ô tích, "Chọn tất cả",
  // ô tích tuỳ chọn của từng loại (vd. "Xoá vĩnh viễn"), rồi mới thực hiện. AI không bao giờ tự thao tác.
  // Nội dung theo loại nằm ở reviewKinds.js; mở bằng ai.review = { kind, items } (AiPanel, khi bấm nút trên thẻ).
  import { untrack } from 'svelte';
  import { ai } from './ai.svelte.js';
  import { REVIEW_KINDS } from './reviewKinds.js';
  import { procs } from './procs.svelte.js';
  import { toasts } from './toasts.svelte.js';
  import { fmtInt, errText } from './format.js';
  import Glyph from './Glyph.svelte';
  import { t } from './i18n.svelte.js';

  /** [{ key, label, reason, sub?, side?, icon, blocked, checked, status: ''|'ok'|'fail', error }] */
  let rows = $state([]);
  let loading = $state(false);
  let busy = $state(false);
  let opts = $state({});
  let box = $state();
  let allBox = $state();
  let prevFocus = null;
  /** hàng đang mở phần chi tiết (`details`, vd. từng PID của một ứng dụng) */
  let expanded = $state(new Set());

  const open = $derived(ai.review);
  const K = $derived(open ? REVIEW_KINDS[open.kind] : null);
  const selectable = $derived(rows.filter((r) => !r.blocked && r.status !== 'ok'));
  const chosen = $derived(selectable.filter((r) => r.checked));
  const allOn = $derived(selectable.length > 0 && chosen.length === selectable.length);

  $effect(() => {
    if (allBox) allBox.indeterminate = chosen.length > 0 && !allOn;
  });

  $effect(() => {
    const r = open;
    if (!r) return;
    untrack(() => {
      prevFocus = document.activeElement;
      // giá trị đầu của ô tuỳ chọn: o.init() (vd. theo cài đặt của trang) hoặc chưa tích
      opts = Object.fromEntries((REVIEW_KINDS[r.kind]?.options || []).map((o) => [o.id, !!o.init?.()]));
      load(r);
    });
    return () => {
      const el = prevFocus;
      prevFocus = null;
      if (el?.isConnected) el.focus({ preventScroll: true });
    };
  });

  async function load(r) {
    const kind = REVIEW_KINDS[r.kind];
    if (!kind) return;
    anchor = -1;
    expanded = new Set();
    rows = r.items.map((x) => ({ key: x.key, label: x.label, reason: x.reason, icon: 'document', blocked: '', checked: false, side: '', status: '', error: '' }));
    loading = true;
    try {
      const got = await kind.load(r.items);
      if (ai.review !== r) return; // đã đóng / mở đề xuất khác
      rows = got.map((x) => ({ status: '', error: '', ...x }));
    } catch (e) {
      toasts.error(t('ai.review.check_failed'), [errText(e)]);
    } finally {
      loading = false;
      queueMicrotask(() => box?.querySelector('button.btn')?.focus());
    }
  }

  // Bấm ô = đảo một mục; Shift+bấm = tích / bỏ tích cả dải từ ô bấm trước (như cột tích của DataTable).
  let anchor = -1;
  function rowCheck(e, i) {
    const r = rows[i];
    const on = !r.checked;
    // không preventDefault: ô bấm tự đổi sang `on`, đúng trạng thái mới (chặn thì trình duyệt đảo lại sau khi đã vẽ)
    if (e.shiftKey && anchor >= 0 && anchor < rows.length) {
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      for (const x of rows.slice(a, b + 1)) if (!x.blocked && x.status !== 'ok') x.checked = on;
    } else r.checked = on;
    anchor = i;
  }

  function toggleAll() {
    const on = !allOn;
    for (const r of selectable) r.checked = on;
  }
  function close() {
    if (!busy) ai.review = null;
  }

  async function confirm() {
    if ((!chosen.length && !K?.allowEmpty) || busy || !K) return;
    busy = true;
    const targets = chosen.slice();
    try {
      const res = await K.run(targets, opts);
      const ok = [], fail = [];
      for (const r of targets) {
        const x = res.get(r.key);
        if (x?.ok) { r.status = 'ok'; r.checked = false; ok.push(r); }
        else { r.status = 'fail'; r.error = x?.error || t('ai.review.run_failed'); fail.push(r); }
      }
      K.after?.(ok, fail, opts);
      if (!fail.length) { busy = false; close(); }
    } catch (e) {
      toasts.error(t('ai.review.run_failed'), [errText(e)]);
    } finally {
      busy = false;
    }
  }

  function key(e) {
    if (!open) return;
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    else if (e.key === 'Tab' && box) {
      const nodes = [...box.querySelectorAll('button:not(:disabled), input:not(:disabled)')];
      if (!nodes.length) return;
      const i = nodes.indexOf(document.activeElement);
      e.preventDefault();
      nodes[(i + (e.shiftKey ? -1 : 1) + nodes.length) % nodes.length]?.focus();
    }
  }
</script>

<svelte:window onkeydown={key} />

{#if open && K}
  <div class="modal-backdrop" role="presentation" onclick={close}></div>
  <div class="modal del-review" bind:this={box} role="dialog" aria-modal="true" aria-labelledby="act-title">
    <h2 id="act-title">{typeof K.title === 'function' ? K.title(open) : K.title}</h2>
    <p class="muted small">{typeof K.hint === 'function' ? K.hint(open) : K.hint}</p>

    <div class="del-list" role="group" aria-label={t('ai.review.list_aria')}>
      <label class="del-row del-all">
        <input type="checkbox" bind:this={allBox} checked={allOn} disabled={!selectable.length || busy || loading} onchange={toggleAll} />
        <span class="grow">{t('ai.review.select_all')}</span>
        <span class="muted small">{fmtInt(chosen.length)}/{fmtInt(selectable.length)}</span>
      </label>
      {#if !loading && !rows.length && K.emptyText}<p class="muted small del-empty">{K.emptyText}</p>{/if}
      {#each rows as r, i (r.key)}
        {@const img = (procs.iconV, r.exe ? procs.icons.get(r.exe) : null)}
        {@const open = expanded.has(r.key)}
        <label class="del-row" class:off={!!r.blocked || r.status === 'ok'} title={r.label}>
          <input type="checkbox" checked={r.checked} disabled={!!r.blocked || r.status === 'ok' || busy} onclick={(e) => rowCheck(e, i)} />
          <span class="del-ico" class:img={!!r.exe}>{#if img}<img src={img} width="18" height="18" alt="" />{:else}<Glyph name={r.icon} size={r.exe ? 16 : 14} />{/if}</span>
          <span class="del-main">
            <span class="del-path ellipsis" class:plain={K.plainLabel}>{r.label}</span>
            {#if r.status === 'ok'}
              <span class="small ok-text"><Glyph name="check" size={11} /> {K.doneText}</span>
            {:else if r.status === 'fail'}
              <span class="small warn-text">{r.error}</span>
            {:else if r.blocked}
              <span class="small warn-text">{r.blocked}</span>
            {:else if r.reason}
              <span class="muted small ellipsis" title={r.reason}>{r.reason}</span>
            {/if}
          </span>
          <span class="del-size small">{#if loading}<span class="spin xs"></span>{:else}{r.side}{/if}</span>
          {#if r.details?.length > 1}
            <button type="button" class="icon-btn xs del-exp" aria-expanded={open} title={open ? t('ai.review.collapse') : t('ai.review.expand_procs', { count: r.details.length, n: fmtInt(r.details.length) })} aria-label={open ? t('ai.review.collapse') : t('ai.review.expand')}
              onclick={(e) => { e.preventDefault(); const n = new Set(expanded); open ? n.delete(r.key) : n.add(r.key); expanded = n; }}>
              <Glyph name={open ? 'chevronUp' : 'chevronDown'} size={11} />
            </button>
          {/if}
        </label>
        {#if open}
          <ul class="del-details">
            {#each r.details as d, j (j)}
              <li><span class="ellipsis" class:warn-text={!!d.warn} title={d.warn || d.text}>{d.text}{#if d.warn} · {d.warn}{/if}</span><span class="del-size small">{d.side}</span></li>
            {/each}
          </ul>
        {/if}
      {/each}
    </div>

    {#each K.options || [] as o (o.id)}
      <label class="check del-perm">
        <input type="checkbox" bind:checked={opts[o.id]} disabled={busy} />
        <span>{o.label}</span>
      </label>
      {#if opts[o.id] && o.warn}<p class="warn-text small del-warn"><Glyph name="warning" size={12} /> {o.warn}</p>{/if}
    {/each}

    <div class="modal-actions">
      <span class="grow muted small">{#if chosen.length && K.summary}{K.summary(chosen)}{/if}</span>
      <button type="button" class="btn danger solid" disabled={(!chosen.length && !K.allowEmpty) || busy || loading} onclick={confirm}>
        {#if busy}<span class="spin sm" aria-hidden="true"></span>{/if}
        {K.confirmLabel(opts, chosen.length)}
      </button>
      <button type="button" class="btn" disabled={busy} onclick={close}>{t('ai.review.close')}</button>
    </div>
  </div>
{/if}
