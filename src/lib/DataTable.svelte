<script>
  // Bảng ảo hoá dùng chung (Khởi động, Dịch vụ, Cổng): chiều cao hàng cố định, chỉ render hàng đang thấy (+ overscan).
  // Cột kéo giãn được (lưu localStorage), bấm tiêu đề để sắp xếp.
  //
  // Chọn hàng (giống trang Tiến trình): bấm = chọn riêng hàng đó, Ctrl+bấm = thêm/bớt, Shift+bấm = chọn dải.
  // Ô tích (nếu có `selection`) thêm/bớt mà không bỏ các hàng khác. Bấm đúp / Enter = onopen (xem chi tiết).
  // Bàn phím trên bảng: ↑/↓/PgUp/PgDn/Home/End di chuyển, Space tích, Ctrl+A chọn hết, Esc bỏ chọn.
  // Không có `selection` -> bảng chỉ đọc (không chọn hàng); vẫn có hàng "đang trỏ" (onfocus) và Space = ontoggle.
  // Cột tích RIÊNG cho bảng chỉ đọc (vd. "Dọn" ở trang Dọn dẹp): `checked` (Set id, phản ứng được) + `oncheck(ids, on)`
  // (+ `canCheck(row)`). Hành vi y như cột chọn: ô đầu bảng = tất cả, Shift+bấm ô / hàng = cả dải từ hàng bấm trước,
  // Ctrl+bấm hàng = đảo một ô, Space = đảo ô hàng đang trỏ. Bấm thường chỉ trỏ hàng (ngăn chi tiết), không tích.
  import { untrack } from 'svelte';
  import { t } from './i18n.svelte.js';
  let {
    columns,
    rows,
    sort,
    onsort,
    selection = null,
    loading = false,
    storageKey,
    cell,
    onopen,
    onmenu, // (event, row) – menu chuột phải; hàng được chọn trước nếu chưa chọn
    onfocus, // (id | null) – hàng đang trỏ đổi (bấm / phím mũi tên); dùng cho ngăn chi tiết của bảng chỉ đọc
    ontoggle, // (row) – Space trên bảng chỉ đọc khi không có cột tích
    checked = null, // Set id đang tích – cột tích riêng (chỉ khi không có `selection`)
    oncheck, // (ids, on) – tích / bỏ tích các hàng (đã lọc theo canCheck)
    canCheck = () => true, // (row) -> hàng này tích được không
    checkLabel,
    rowHeight = 32,
    label,
    emptyText,
    empty, // snippet tuỳ chọn thay cho emptyText
  } = $props();

  const OVERSCAN = 10;
  const CHECK_W = 36;
  const HEAD_H = 32;
  const checks = $derived(!!selection || !!checked);
  const isOn = (id) => (selection ? selection.has(id) : !!checked?.has(id));
  const checkable = (r) => !!selection || canCheck(r);
  /** Tích / bỏ tích các hàng (một chỗ cho ô, ô đầu bảng, Shift, Space). */
  function setBoxes(list, on) {
    const ok = list.filter(checkable);
    if (selection) for (const r of ok) on ? selection.add(r.id) : selection.delete(r.id);
    else if (ok.length) oncheck?.(ok.map((r) => r.id), on);
  }

  function loadWidths() {
    let saved = {};
    try { saved = JSON.parse(localStorage.getItem('cols:' + storageKey) || '{}'); } catch {}
    const w = {};
    for (const c of columns) w[c.key] = Math.max(c.min ?? 50, Number(saved[c.key]) || c.width);
    return w;
  }
  let widths = $state(loadWidths());
  function saveWidths() {
    try { localStorage.setItem('cols:' + storageKey, JSON.stringify(widths)); } catch {}
  }

  let scroller = $state();
  let scrollTop = $state(0);
  let viewH = $state(600);
  let focusId = $state(null);
  let anchor = -1;
  $effect(() => {
    const id = focusId;
    untrack(() => onfocus?.(id));
  });

  const start = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewH) / rowHeight) + OVERSCAN));
  const visible = $derived(rows.slice(start, end));
  const template = $derived(
    (checks ? `${CHECK_W}px ` : '') +
      columns.map((c, i) => (i === columns.length - 1 ? `minmax(${widths[c.key]}px, 1fr)` : `${widths[c.key]}px`)).join(' ')
  );
  const minW = $derived((checks ? CHECK_W : 0) + columns.reduce((a, c) => a + widths[c.key], 0));

  // ô "chọn tất cả" (các hàng đang hiện theo bộ lọc)
  const boxRows = $derived(selection ? rows : rows.filter(checkable));
  const selCount = $derived.by(() => {
    if (!(selection ?? checked)?.size) return 0;
    let n = 0;
    for (const r of boxRows) if (isOn(r.id)) n++;
    return n;
  });
  const allSel = $derived(boxRows.length > 0 && selCount === boxRows.length);
  const someSel = $derived(selCount > 0 && !allSel);

  function toggleAll() {
    setBoxes(boxRows, !allSel);
  }
  function setRange(idx, on) {
    const [a, b] = anchor < idx ? [anchor, idx] : [idx, anchor];
    setBoxes(rows.slice(a, b + 1), on);
  }

  function indexOf(id) {
    for (let i = 0; i < rows.length; i++) if (rows[i].id === id) return i;
    return -1;
  }
  function selectOnly(idx) {
    const r = rows[idx];
    if (!r) return;
    focusId = r.id;
    anchor = idx;
    if (!selection) return;
    if (selection.size !== 1 || !selection.has(r.id)) {
      selection.clear();
      selection.add(r.id);
    }
  }
  function checkClick(e, row, idx) {
    e.stopPropagation();
    const want = !isOn(row.id);
    if (e.shiftKey && anchor >= 0 && anchor < rows.length) setRange(idx, want);
    else setBoxes([row], want);
    anchor = idx;
    focusId = row.id;
  }

  function isControl(t) {
    return t instanceof Element && !!t.closest('button, input, select, a, label, .resizer');
  }
  function rowClick(e, row, idx) {
    if (isControl(e.target)) return;
    scroller?.focus({ preventScroll: true });
    if (!selection) {
      // bảng chỉ đọc: bấm = trỏ hàng; có cột tích riêng thì Ctrl = đảo ô, Shift = tích cả dải
      if (checked && (e.ctrlKey || e.metaKey)) setBoxes([row], !isOn(row.id));
      else if (checked && e.shiftKey && anchor >= 0 && anchor < rows.length) {
        setRange(idx, !isOn(row.id));
        focusId = row.id;
        return;
      }
      focusId = row.id;
      anchor = idx;
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      selection.has(row.id) ? selection.delete(row.id) : selection.add(row.id);
      anchor = idx;
      focusId = row.id;
    } else if (e.shiftKey && anchor >= 0 && anchor < rows.length) {
      selection.clear();
      const [a, b] = anchor < idx ? [anchor, idx] : [idx, anchor];
      for (let i = a; i <= b; i++) selection.add(rows[i].id);
      focusId = row.id;
    } else selectOnly(idx);
  }
  function rowMenu(e, row, idx) {
    if (!onmenu) return;
    e.preventDefault();
    if (selection && !selection.has(row.id)) selectOnly(idx);
    else focusId = row.id;
    onmenu(e, row);
  }
  function rowDbl(e, row) {
    if (isControl(e.target)) return;
    onopen?.(row);
  }

  function ensureVisible(i) {
    if (!scroller) return;
    const top = i * rowHeight;
    const avail = scroller.clientHeight - HEAD_H;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + rowHeight > scroller.scrollTop + avail) scroller.scrollTop = top + rowHeight - avail;
  }
  function move(delta) {
    const n = rows.length;
    if (!n) return;
    let i = focusId != null ? indexOf(focusId) : -1;
    if (i < 0) i = delta > 0 ? -1 : n;
    const j = Math.max(0, Math.min(n - 1, i + delta));
    selectOnly(j);
    ensureVisible(j);
  }
  function tableKey(e) {
    if (isControl(e.target) && e.target !== scroller) return;
    const page = Math.max(1, Math.floor((viewH - HEAD_H) / rowHeight) - 1);
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); move(1); break;
      case 'ArrowUp': e.preventDefault(); move(-1); break;
      case 'PageDown': e.preventDefault(); move(page); break;
      case 'PageUp': e.preventDefault(); move(-page); break;
      case 'Home': e.preventDefault(); focusId = null; move(1); break;
      case 'End': e.preventDefault(); focusId = null; move(-1); break;
      case 'Enter': {
        const i = focusId != null ? indexOf(focusId) : -1;
        if (i >= 0) { e.preventDefault(); onopen?.(rows[i]); }
        break;
      }
      case ' ': {
        const i = focusId != null ? indexOf(focusId) : -1;
        if (i >= 0 && selection) {
          e.preventDefault();
          const id = rows[i].id;
          selection.has(id) ? selection.delete(id) : selection.add(id);
        } else if (i >= 0 && checked) {
          e.preventDefault();
          setBoxes([rows[i]], !isOn(rows[i].id));
          anchor = i;
        } else if (i >= 0 && ontoggle) {
          e.preventDefault();
          ontoggle(rows[i]);
        }
        break;
      }
      case 'ContextMenu': {
        const i = focusId != null ? indexOf(focusId) : -1;
        if (i >= 0 && onmenu && scroller) {
          e.preventDefault();
          const r = scroller.getBoundingClientRect();
          onmenu({ clientX: r.left + 80, clientY: r.top + HEAD_H + i * rowHeight - scroller.scrollTop + rowHeight }, rows[i]);
        }
        break;
      }
      case 'Escape':
        if (selection?.size) { e.preventDefault(); selection.clear(); }
        break;
      case 'a':
        if ((e.ctrlKey || e.metaKey) && selection) { e.preventDefault(); for (const r of rows) selection.add(r.id); }
        break;
    }
  }

  function startResize(e, c) {
    e.preventDefault();
    e.stopPropagation();
    const x0 = e.clientX;
    const w0 = widths[c.key];
    const tgt = e.currentTarget;
    tgt.setPointerCapture(e.pointerId);
    const mv = (ev) => { widths[c.key] = Math.max(c.min ?? 50, Math.round(w0 + ev.clientX - x0)); };
    const up = () => {
      tgt.removeEventListener('pointermove', mv);
      tgt.removeEventListener('pointerup', up);
      tgt.removeEventListener('pointercancel', up);
      saveWidths();
    };
    tgt.addEventListener('pointermove', mv);
    tgt.addEventListener('pointerup', up);
    tgt.addEventListener('pointercancel', up);
  }
  function resetWidth(c) {
    widths[c.key] = c.width;
    saveWidths();
  }

  // Sự kiện scroll vốn đã được trình duyệt gộp theo khung hình.
  function onScroll() {
    scrollTop = scroller.scrollTop;
  }

  /** Cuộn tới hàng `id`, đặt con trỏ bàn phím vào đó (dùng khi mở trang để xem một mục). */
  export function reveal(id) {
    const i = indexOf(id);
    if (i < 0 || !scroller) return;
    focusId = id;
    anchor = i;
    scroller.scrollTop = Math.max(0, i * rowHeight - (scroller.clientHeight - HEAD_H) / 3);
    scroller.focus({ preventScroll: true });
  }
</script>

<div class="dt" style="--cols:{template}; --row-h:{rowHeight}px; --head-h:{HEAD_H}px">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="dt-scroll" bind:this={scroller} bind:clientHeight={viewH} onscroll={onScroll} onkeydown={tableKey} tabindex="0" role="grid" aria-label={label ?? t('shell.table.label')} aria-rowcount={rows.length + 1} aria-multiselectable={!!selection}>
    <div class="dt-head" role="row" style="min-width:{minW}px">
      {#if checks}
        <div class="th th-check" role="columnheader">
          <input type="checkbox" checked={allSel} indeterminate={someSel} onchange={toggleAll} disabled={!boxRows.length} aria-label={checkLabel ?? t('shell.table.check_all')} title={checkLabel ?? t('shell.table.check_all')} />
        </div>
      {/if}
      {#each columns as c (c.key)}
        <div class="th" class:sorted={sort?.key === c.key} class:right={c.align === 'right'} role="columnheader" aria-sort={sort?.key === c.key ? (sort.desc ? 'descending' : 'ascending') : 'none'}>
          {#if c.sortable !== false && onsort}
            <button type="button" class="th-btn" onclick={() => onsort(c.key)} title={t('shell.table.sort_by', { col: c.label })}>
              <span class="ellipsis">{c.label}</span>
              <span class="arrow">{sort?.key === c.key ? (sort.desc ? '↓' : '↑') : ''}</span>
            </button>
          {:else}
            <span class="th-btn static ellipsis">{c.label}</span>
          {/if}
          <button type="button" class="resizer" aria-label={t('shell.table.resize_col', { col: c.label })} tabindex="-1" title={t('shell.table.resize_tip')} onpointerdown={(e) => startResize(e, c)} ondblclick={() => resetWidth(c)}></button>
        </div>
      {/each}
    </div>

    {#if loading && rows.length === 0}
      <div class="dt-body" style="min-width:{minW}px">
        {#each Array(14) as _, i (i)}
          <div class="tr skeleton-row" style="opacity:{1 - i * 0.06}">
            {#if checks}<div class="td td-check"><span class="sk sk-box"></span></div>{/if}
            {#each columns as c (c.key)}
              <div class="td"><span class="sk" style="width:{40 + ((i * 37 + c.width) % 50)}%"></span></div>
            {/each}
          </div>
        {/each}
      </div>
    {:else if rows.length === 0}
      <div class="dt-empty">{#if empty}{@render empty()}{:else}{emptyText ?? t('shell.table.empty')}{/if}</div>
    {:else}
      <div class="dt-body" style="height:{rows.length * rowHeight}px; min-width:{minW}px">
        <div class="dt-window" style="transform:translateY({start * rowHeight}px)">
          {#each visible as row, i (row.id)}
            {@const idx = start + i}
            {@const sel = isOn(row.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="tr"
              class:sel
              class:focus={focusId === row.id}
              role="row"
              aria-rowindex={idx + 2}
              aria-selected={selection ? sel : undefined}
              onclick={(e) => rowClick(e, row, idx)}
              ondblclick={(e) => rowDbl(e, row)}
              oncontextmenu={(e) => rowMenu(e, row, idx)}
            >
              {#if checks}
                <div class="td td-check" role="gridcell">
                  <input type="checkbox" tabindex="-1" checked={sel} disabled={!checkable(row)} onclick={(e) => checkClick(e, row, idx)} aria-label={t('shell.table.select_row', { name: row.name })} />
                </div>
              {/if}
              {#each columns as c (c.key)}
                <div class="td" class:right={c.align === 'right'} role="gridcell">{@render cell(row, c.key)}</div>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
