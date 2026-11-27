<script>
  // Cây chọn phần cần dọn của MỘT mục trang Dọn dẹp (trong ngăn chi tiết, để bảng chính gọn):
  //   Cache / Log / Báo cáo lỗi (ô chọn ba trạng thái + tổng)
  //     └ từng thư mục gốc (ô chọn, dung lượng, mở trong Explorer; Shift+bấm = chọn cả dải)
  //         └ bấm mở: tệp lớn nhất ngay trong thư mục (chỉ xem, đọc khi mở – du_files, 20 tệp)
  // Chọn ở đây và cột tích của bảng là MỘT lựa chọn (cleanup.selected / setParts); nút cuối ngăn dọn đúng phần đã chọn.
  import { untrack } from 'svelte';
  import { cleanup, PART_LABELS } from './cleanup.svelte.js';
  import { ai } from './ai.svelte.js';
  import ContextMenu from './ContextMenu.svelte';
  import { store } from './store.svelte.js';
  import * as api from './api.js';
  import { fmtBytes, fmtInt, errText, copyText } from './format.js';
  import { t } from './i18n.svelte.js';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';

  let { item, onopen } = $props();

  const ORDER = ['cache', 'log', 'crash', ''];
  const busy = $derived(!!cleanup.phase);
  // nhóm theo loại, giữ thứ tự của backend (đã sắp theo loại rồi đường dẫn)
  const groups = $derived(
    ORDER.map((kind) => ({ kind, ks: item.paths.map((_, k) => k).filter((k) => (item.parts?.[k]?.kind || '') === kind) })).filter((g) => g.ks.length)
  );
  const flat = $derived(groups.flatMap((g) => g.ks));
  const sel = $derived(cleanup.selected(item));
  const selBytes = $derived([...sel].reduce((n, k) => n + cleanup.partFreeable(item, k), 0));
  const kindLabel = (k) => (k ? PART_LABELS[k] : t('cleanup.tree.folders_label'));

  // đường dẫn ngắn: bỏ phần đầu chung của mọi thư mục (đường dẫn đầy đủ ở tooltip)
  const prefix = $derived.by(() => {
    if (item.paths.length < 2) return '';
    const split = item.paths.map((p) => p.split('\\'));
    let n = 0;
    while (split.every((s) => s.length > n + 1 && s[n].toLowerCase() === split[0][n].toLowerCase())) n++;
    return n ? split[0].slice(0, n).join('\\') + '\\' : '';
  });
  const short = (p) => (prefix && p.startsWith(prefix) ? p.slice(prefix.length) : p.split('\\').slice(-2).join('\\'));

  // ---------- mở rộng ----------
  let closedKinds = $state(new Set());
  let openDirs = $state(new Set());
  let files = $state.raw(new Map()); // k -> { loading, items, total, error }
  let last = null; // thư mục bấm gần nhất (Shift+bấm)
  $effect(() => {
    item.id;
    untrack(() => {
      closedKinds = new Set();
      openDirs = new Set();
      files = new Map();
      last = null;
    });
  });
  function toggleKind(kind) {
    const s = new Set(closedKinds);
    s.has(kind) ? s.delete(kind) : s.add(kind);
    closedKinds = s;
  }
  async function toggleDir(k) {
    const s = new Set(openDirs);
    if (s.has(k)) {
      s.delete(k);
      openDirs = s;
      return;
    }
    s.add(k);
    openDirs = s;
    if (files.has(k)) return;
    const id = item.id;
    files = new Map(files).set(k, { loading: true });
    let v;
    try {
      const r = await api.duFiles(item.paths[k], 20);
      v = { items: r?.items || [], total: r?.total || 0 };
    } catch (e) {
      v = { error: errText(e) };
    }
    if (item.id === id) files = new Map(files).set(k, v);
  }

  // ---------- chọn ----------
  function setKs(ks, on) {
    const s = new Set(sel);
    for (const k of ks) on ? s.add(k) : s.delete(k);
    cleanup.setParts(item, s);
  }
  function pickDir(k, on, shift) {
    if (shift && last != null && last !== k) {
      const a = flat.indexOf(last), b = flat.indexOf(k);
      setKs(flat.slice(Math.min(a, b), Math.max(a, b) + 1), on);
    } else setKs([k], on);
    last = k;
  }
  const groupState = (g) => {
    const n = g.ks.filter((k) => sel.has(k)).length;
    return n === 0 ? 'none' : n === g.ks.length ? 'all' : 'some';
  };
  const sizeOf = (k) => (item.pending ? null : cleanup.partFreeable(item, k));
  const groupSize = (g) => (item.pending ? null : g.ks.reduce((n, k) => n + cleanup.partFreeable(item, k), 0));

  // ---------- menu chuột phải (loại / thư mục / tệp): chọn, mở, sao chép đường dẫn, hỏi AI ----------
  let menu = $state(null);
  function openMenu(e, m) {
    e.preventDefault();
    e.stopPropagation();
    menu = { x: e.clientX, y: e.clientY, ...m };
  }
  const menuItems = $derived.by(() => {
    if (!menu) return [];
    const ask = { id: 'ask', label: t('cleanup.tree.menu.ask'), glyph: 'chat' };
    if (menu.type === 'kind') {
      const on = groupState(menu.g) === 'all';
      return [
        { id: on ? 'unpick' : 'pick', label: t(on ? 'cleanup.tree.menu.unpick_kind' : 'cleanup.tree.menu.pick_kind'), glyph: 'check', disabled: busy },
        { id: 'open', label: t(menu.g.ks.length > 1 ? 'cleanup.tree.menu.open_first' : 'cleanup.tree.menu.open'), glyph: 'folder' },
        { sep: true },
        ask,
      ];
    }
    if (menu.type === 'dir') {
      const on = sel.has(menu.k);
      return [
        { id: on ? 'unpick' : 'pick', label: t(on ? 'cleanup.tree.menu.unpick' : 'cleanup.tree.menu.pick'), glyph: 'check', disabled: busy },
        { id: 'open', label: t('cleanup.tree.menu.open'), glyph: 'folder' },
        { id: 'copy', label: t('cleanup.tree.menu.copy'), glyph: 'copy' },
        { sep: true },
        ask,
      ];
    }
    return [
      { id: 'open', label: t('cleanup.tree.menu.open_file'), glyph: 'folder' },
      { id: 'copy', label: t('cleanup.tree.menu.copy'), glyph: 'copy' },
      { sep: true },
      ask,
    ];
  });
  function pickMenu(id) {
    const m = menu;
    menu = null;
    if (!m) return;
    const dir = m.type === 'kind' ? item.paths[m.g.ks[0]] : item.paths[m.k];
    const file = m.type === 'file' ? `${dir}\\${m.file}` : '';
    if (id === 'pick' || id === 'unpick') setKs(m.type === 'kind' ? m.g.ks : [m.k], id === 'pick');
    else if (id === 'open') (m.type === 'file' ? api.duOpen(file, false).catch(() => {}) : onopen(dir));
    else if (id === 'copy') copyText(m.type === 'file' ? file : dir);
    else if (id === 'ask') {
      if (m.type === 'kind') {
        const paths = m.g.ks.slice(0, 5).map((k) => item.paths[k]).join('; ');
        ai.askCleanup(t('cleanup.ask.kind', { part: kindLabel(m.g.kind), name: item.name, id: item.id, paths }));
      } else if (m.type === 'dir') ai.askCleanup(t('cleanup.ask.folder', { path: dir, name: item.name }));
      else ai.askCleanup(t('cleanup.ask.file', { path: file, name: item.name }));
    }
  }

  async function cleanSelected() {
    if (!sel.size || busy) return;
    const ok = await store.confirm({
      title: t('cleanup.tree.confirm_title', { name: item.name }),
      message: [t('cleanup.tree.confirm_msg', { count: sel.size, n: fmtInt(sel.size), size: fmtBytes(selBytes) }), t(cleanup.keepRecent ? 'cleanup.confirm.skip_keep' : 'cleanup.confirm.skip')].join(' '),
      okLabel: t('cleanup.confirm.ok'),
      danger: true,
    });
    if (ok) await cleanup.clean([item.id]);
  }
</script>

<section class="insp-card res cl-tree" aria-label={t('cleanup.tree.title')}>
  <div class="res-head">
    <Glyph name="tree" size={14} /><b>{t('cleanup.tree.title')}</b>
    <span class="grow"></span>
    <button type="button" class="link-btn small" disabled={busy || sel.size === item.paths.length} onclick={() => setKs(flat, true)}>{t('cleanup.tree.all')}</button>
    <button type="button" class="link-btn small" disabled={busy || !sel.size} onclick={() => setKs(flat, false)}>{t('cleanup.tree.none')}</button>
  </div>

  <ul class="clt" role="tree">
    {#each groups as g (g.kind)}
      {@const st = groupState(g)}
      {@const open = !closedKinds.has(g.kind)}
      {@const gs = groupSize(g)}
      <li role="treeitem" aria-expanded={open} aria-selected={st === 'all'}>
        <div class="clt-row kind" role="presentation" oncontextmenu={(e) => openMenu(e, { type: 'kind', g })}>
          <button type="button" class="clt-tw" onclick={() => toggleKind(g.kind)} aria-label={open ? t('cleanup.tree.collapse') : t('cleanup.tree.expand')}><Glyph name={open ? 'chevronDown' : 'chevronRight'} size={10} /></button>
          <input type="checkbox" checked={st === 'all'} indeterminate={st === 'some'} disabled={busy} onclick={(e) => setKs(g.ks, e.currentTarget.checked)} aria-label={kindLabel(g.kind)} />
          <button type="button" class="clt-name" onclick={() => toggleKind(g.kind)}>
            <b>{kindLabel(g.kind)}</b> <span class="faint">· {t('cleanup.tree.folders', { count: g.ks.length, n: fmtInt(g.ks.length) })}</span>
          </button>
          <span class="mono muted clt-size">{gs == null ? t('cleanup.cell.measuring') : fmtBytes(gs)}</span>
        </div>
        {#if open}
          <ul role="group">
            {#each g.ks as k (k)}
              {@const p = item.paths[k]}
              {@const dOpen = openDirs.has(k)}
              {@const sz = sizeOf(k)}
              {@const f = files.get(k)}
              <li role="treeitem" aria-expanded={dOpen} aria-selected={sel.has(k)}>
                <div class="clt-row dir" class:on={sel.has(k)} role="presentation" oncontextmenu={(e) => openMenu(e, { type: 'dir', k })}>
                  <button type="button" class="clt-tw" onclick={() => toggleDir(k)} title={dOpen ? t('cleanup.tree.collapse') : t('cleanup.tree.preview')} aria-label={dOpen ? t('cleanup.tree.collapse') : t('cleanup.tree.preview')}><Glyph name={dOpen ? 'chevronDown' : 'chevronRight'} size={10} /></button>
                  <input type="checkbox" checked={sel.has(k)} disabled={busy} onclick={(e) => pickDir(k, e.currentTarget.checked, e.shiftKey)} aria-label={p} />
                  <button type="button" class="clt-name mono" onclick={() => toggleDir(k)} title={p}><FsIcon kind="folder" size={12} /><span class="ellipsis">{short(p)}</span></button>
                  <span class="mono muted clt-size" class:faint={sz === 0}>{sz == null ? '' : fmtBytes(sz)}</span>
                  <button type="button" class="icon-btn xs clt-open" onclick={() => onopen(p)} title={t('cleanup.insp.open_tip')} aria-label={t('cleanup.insp.open_tip')}><Glyph name="open" size={10} /></button>
                </div>
                {#if dOpen}
                  <ul class="clt-files" role="group">
                    {#if !f || f.loading}
                      <li class="faint small"><span class="spin xs"></span> {t('cleanup.tree.loading')}</li>
                    {:else if f.error}
                      <li class="warn-text small">{t('cleanup.tree.read_failed')}: {f.error}</li>
                    {:else if !f.items.length}
                      <li class="faint small">{t('cleanup.tree.only_dirs')}</li>
                    {:else}
                      {#each f.items as x (x.name)}
                        <li class="clt-file" oncontextmenu={(e) => openMenu(e, { type: 'file', k, file: x.name })}><FsIcon kind="file" size={11} /><span class="ellipsis mono" title={x.name}>{x.name}</span><span class="mono faint">{fmtBytes(x.size)}</span></li>
                      {/each}
                      {#if f.total > f.items.length}<li class="faint small">{t('cleanup.tree.more', { count: f.total - f.items.length, n: fmtInt(f.total - f.items.length) })}</li>{/if}
                    {/if}
                  </ul>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </li>
    {/each}
  </ul>
  <div class="faint small">{t('cleanup.tree.hint')}</div>
</section>

<div class="cl-foot">
  <span class="small grow">
    {#if sel.size}{t('cleanup.tree.selected', { count: sel.size, n: fmtInt(sel.size) })} · <b>{item.pending ? t('cleanup.cell.measuring') : fmtBytes(selBytes)}</b>{:else}<span class="muted">{t('cleanup.tree.nothing')}</span>{/if}
  </span>
  <button type="button" class="btn sm danger solid" disabled={!sel.size || busy} onclick={cleanSelected}>
    <Glyph name="delete" size={12} /> {cleanup.phase === 'clean' ? t('cleanup.cleaning') : t('cleanup.tree.clean')}
  </button>
</div>

{#if menu}<ContextMenu x={menu.x} y={menu.y} items={menuItems} label={t('cleanup.tree.menu.label')} onpick={pickMenu} onclose={() => (menu = null)} />{/if}
