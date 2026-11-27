<script>
  // Trang Ổ cứng: chọn ổ / thư mục -> cây dung lượng kiểu TreeSize, lớn dần NGAY trong lúc quét.
  // Bảng ảo hoá (chỉ render hàng đang thấy). Chuột phải = menu gốc của Windows (bản Tauri),
  // lỗi / trình duyệt -> menu riêng (Mở, Xoá, Xoá vĩnh viễn…). Delete = Thùng rác, Shift+Delete = vĩnh viễn.
  import { onMount, untrack } from 'svelte';
  import { du } from './du.svelte.js';
  import { store } from './store.svelte.js';
  import * as api from './api.js';
  import { isTauri } from './api.js';
  import { toasts } from './toasts.svelte.js';
  import { ai } from './ai.svelte.js';
  import { fmtBytes, fmtInt, fmtPct, copyText, errText } from './format.js';
  import { t, i18n } from './i18n.svelte.js';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import ContextMenu from './ContextMenu.svelte';

  const ROW_H = 26;
  const HEAD_H = 34;
  const OVERSCAN = 10;
  const COLS = $derived([
    { key: 'name', label: t('disk.col.name'), w: 'minmax(320px, 1fr)' },
    { key: 'size', label: t('disk.col.size'), w: '104px', r: true },
    { key: 'alloc', label: t('disk.col.alloc'), w: '104px', r: true },
    { key: 'files', label: t('disk.col.files'), w: '92px', r: true },
    { key: 'dirs', label: t('disk.col.dirs'), w: '84px', r: true },
    { key: 'pct', label: t('disk.col.pct'), w: '132px', sortKey: 'size' },
    { key: 'mtime', label: t('disk.col.mtime'), w: '136px' },
  ]);
  const template = 'minmax(320px, 1fr) 104px 104px 92px 84px 132px 136px';
  // t() có một tham số in đậm: trả [trước, sau] để vẽ {a}<b>{giá trị}</b>{b} (một câu = một khoá)
  const MARK = String.fromCharCode(1);
  const around = (key, name, params) => t(key, { ...params, [name]: MARK }).split(MARK);

  onMount(() => {
    du.init();
  });

  let pathInput = $state('');
  let drivesOpen = $state(false);
  let menu = $state(null); // { x, y, tg }
  let selKey = $state(null);

  $effect(() => {
    du.version;
    const root = du.root;
    if (root && !pathFocused) untrack(() => { pathInput = root; });
  });
  let pathFocused = false;

  // ---------- hàng ----------
  const sortVal = (key, x) => (key === 'alloc' ? x.alloc : key === 'files' ? x.files : x.size);
  const rows = $derived.by(() => {
    du.version;
    const out = [];
    const root = du.nodes.get(0);
    if (!root) return out;
    const { key, desc } = du.sort;
    const walk = (id, depth, parentSize) => {
      const n = du.nodes.get(id);
      if (!n) return;
      const open = du.expanded.has(id);
      const expandable = n.kids > 0 || n.own_files > 0 || (!n.done && !n.link && !n.denied);
      out.push({ key: 'd' + id, type: 'dir', id, n, depth, open, expandable, parentSize });
      if (!open) return;
      const k = du.kids.get(id);
      const entries = [];
      for (const cid of k?.ids || []) {
        const c = du.nodes.get(cid);
        if (c) entries.push({ dir: cid, v: c });
      }
      if (n.own_files > 0) {
        const fe = { files: true, v: { size: n.own_size, alloc: n.own_alloc, files: n.own_files } };
        // chèn "[N tệp]" đúng vị trí theo kích thước (như TreeSize); sắp theo tên/thời gian -> cuối
        let at = entries.length;
        if (key === 'size' || key === 'alloc' || key === 'files') {
          const fv = sortVal(key, fe.v);
          at = entries.findIndex((e) => (desc ? sortVal(key, e.v) < fv : sortVal(key, e.v) > fv));
          if (at < 0) at = entries.length;
        }
        entries.splice(at, 0, fe);
      }
      for (const e of entries) {
        if (e.dir != null) walk(e.dir, depth + 1, n.size);
        else {
          const fopen = du.filesOpen.has(id);
          out.push({ key: 'f' + id, type: 'files', id, depth: depth + 1, open: fopen, expandable: true, size: n.own_size, alloc: n.own_alloc, count: n.own_files, parentSize: n.size });
          if (fopen) {
            const f = du.files.get(id);
            if (!f || f.loading) out.push({ key: 'fl' + id, type: 'note', depth: depth + 2, text: t('disk.note.reading_files'), busy: true });
            else if (f.error) out.push({ key: 'fe' + id, type: 'note', depth: depth + 2, text: f.error, warn: true });
            else {
              const items = sortFiles(f.items, key, desc);
              for (const it of items) out.push({ key: `x${id}/${it.name}`, type: 'file', dir: id, f: it, depth: depth + 2, parentSize: n.own_size });
              if (f.total > f.items.length) out.push({ key: 'fm' + id, type: 'note', depth: depth + 2, text: t('disk.note.more_files', { count: f.total - f.items.length, n: fmtInt(f.total - f.items.length) }) });
            }
          }
        }
      }
      if (k?.more) out.push({ key: 'm' + id, type: 'note', depth: depth + 1, text: t('disk.note.more_dirs', { count: k.more, n: fmtInt(k.more) }) });
    };
    walk(0, 0, root.size);
    return out;
  });
  function sortFiles(items, key, desc) {
    if (key === 'size' && desc) return items; // backend đã sắp lớn trước
    const a = items.slice();
    const v = (x) => (key === 'name' ? x.name.toLowerCase() : key === 'mtime' ? x.mtime : key === 'alloc' ? x.alloc : x.size);
    a.sort((x, y) => { const p = v(x), q = v(y); const o = p < q ? -1 : p > q ? 1 : 0; return desc ? -o : o; });
    return a;
  }

  let scroller = $state();
  let scrollTop = $state(0);
  let viewH = $state(600);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewH) / ROW_H) + OVERSCAN));
  const vis = $derived(rows.slice(start, end));

  // ---------- định dạng ----------
  const dtf = $derived(new Intl.DateTimeFormat(i18n.intl, { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' }));
  const fmtDate = (ms) => (ms > 0 ? dtf.format(ms) : '');
  const pctOf = (v, p) => (p > 0 ? Math.min(100, (v / p) * 100) : 0);
  const rate = $derived.by(() => {
    const p = du.progress;
    return p && p.elapsed_ms > 300 ? Math.round(p.files / (p.elapsed_ms / 1000)) : 0;
  });
  const fmtSec = (ms) => t('common.dur.s', { s: ms < 10000 ? (ms / 1000).toLocaleString(i18n.intl, { maximumFractionDigits: 1 }) : Math.round(ms / 1000) });
  function driveName(d) {
    if (d.kind === 'network') return `${t('disk.drive.network')} (${d.letter})`;
    return `${d.label || t(d.kind === 'removable' ? 'disk.drive.removable' : d.kind === 'cdrom' ? 'disk.drive.cdrom' : 'disk.drive.local')} (${d.letter})`;
  }

  // ---------- chọn / phím ----------
  function targetOf(r) {
    if (!r) return null;
    if (r.type === 'dir') return { path: du.paths.get(r.id), isDir: true, r };
    if (r.type === 'file') {
      const base = du.paths.get(r.dir) || '';
      return { path: (base.endsWith('\\') ? base : base + '\\') + r.f.name, isDir: false, r };
    }
    if (r.type === 'files') return { path: du.paths.get(r.id), isDir: true, r, filesRow: true };
    return null;
  }
  const idx = (k) => rows.findIndex((r) => r.key === k);
  function select(r) {
    selKey = r.key;
    scroller?.focus({ preventScroll: true });
  }
  function ensureVisible(i) {
    if (!scroller) return;
    const top = i * ROW_H;
    const avail = scroller.clientHeight - HEAD_H;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW_H > scroller.scrollTop + avail) scroller.scrollTop = top + ROW_H - avail;
  }
  function move(d) {
    if (!rows.length) return;
    let i = selKey ? idx(selKey) : -1;
    let j = Math.max(0, Math.min(rows.length - 1, (i < 0 ? (d > 0 ? -1 : rows.length) : i) + d));
    while (rows[j]?.type === 'note' && j + Math.sign(d) >= 0 && j + Math.sign(d) < rows.length) j += Math.sign(d);
    selKey = rows[j].key;
    ensureVisible(j);
  }
  function toggleRow(r) {
    if (r.type === 'dir' && r.expandable) du.toggle(r.id);
    else if (r.type === 'files') du.toggleFiles(r.id);
  }
  function onKey(e) {
    if (menu) return;
    const r = selKey ? rows[idx(selKey)] : null;
    const page = Math.max(1, Math.floor((viewH - HEAD_H) / ROW_H) - 1);
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); move(1); break;
      case 'ArrowUp': e.preventDefault(); move(-1); break;
      case 'PageDown': e.preventDefault(); move(page); break;
      case 'PageUp': e.preventDefault(); move(-page); break;
      case 'Home': e.preventDefault(); selKey = null; move(1); break;
      case 'End': e.preventDefault(); selKey = null; move(-1); break;
      case 'ArrowRight': if (r && r.expandable && !r.open) { e.preventDefault(); toggleRow(r); } break;
      case 'ArrowLeft':
        if (r && r.expandable && r.open) { e.preventDefault(); toggleRow(r); }
        else if (r) {
          // về thư mục cha
          e.preventDefault();
          for (let i = idx(r.key) - 1; i >= 0; i--) if (rows[i].depth < r.depth && rows[i].type !== 'note') { selKey = rows[i].key; ensureVisible(i); break; }
        }
        break;
      case 'Enter': { const tg = targetOf(r); if (tg) { e.preventDefault(); open(tg); } break; }
      case 'Delete': {
        const tg = targetOf(r);
        if (tg && !tg.filesRow && !(r.type === 'dir' && r.id === 0)) { e.preventDefault(); del(tg, e.shiftKey); }
        break;
      }
      case 'ContextMenu': case 'F10':
        if (e.key === 'F10' && !e.shiftKey) break;
        if (r) { e.preventDefault(); const i = idx(r.key); const b = scroller.getBoundingClientRect(); openMenu(r, b.left + 80, b.top + HEAD_H + i * ROW_H - scroller.scrollTop + ROW_H, false); }
        break;
    }
  }

  // ---------- hành động ----------
  async function open(tg) {
    try { await api.duOpen(tg.path, tg.isDir); } catch (e) { toasts.error(t('disk.toast.open_failed'), [errText(e)]); }
  }
  async function copyPath(tg) {
    if (await copyText(tg.path)) toasts.success(t('disk.toast.copied'));
  }
  // lệnh của menu Windows không đổi đĩa -> khỏi đồng bộ lại cây (đo lại một thư mục lớn tốn thời gian)
  const READ_ONLY_VERBS = new Set(['copy', 'properties', 'open', 'opennew', 'explore', 'find', 'print', 'edit', 'openas', 'copyaspath', 'pintohome', 'pintostartscreen', 'opencontaining']);
  /** Sau lệnh xoá / cắt / dán… của menu Windows: backend đưa cây về đúng với đĩa cho đường dẫn này (cùng logic với xoá trong app), rồi làm mới. */
  async function syncShell(tg) {
    let res = null;
    try { res = await api.duSyncPaths([tg.path]); } catch {}
    let gone = false;
    try { gone = !(await api.duExists(tg.path)); } catch {}
    if (tg.r.type === 'dir') await du.deletedDir(tg.r.id, gone);
    else if (gone) await du.deletedFile(tg.r.dir, tg.r.f);
    else await du.dirContentsChanged(tg.r.dir);
    if (res?.inexact?.length) toasts.warn(t('disk.toast.inexact'), [t('disk.toast.inexact_detail')]);
    return gone;
  }
  async function del(tg, permanent) {
    if (!tg || tg.filesRow) return;
    const size = tg.r.type === 'dir' ? tg.r.n.size : tg.r.f.size;
    const params = { path: tg.path, size: fmtBytes(size) };
    const ok = await store.confirm({
      title: t(permanent ? 'disk.confirm.nuke_title' : 'disk.confirm.recycle_title'),
      message: t(permanent ? (tg.isDir ? 'disk.confirm.nuke_dir' : 'disk.confirm.nuke_file') : tg.isDir ? 'disk.confirm.recycle_dir' : 'disk.confirm.recycle_file', params),
      okLabel: t(permanent ? 'disk.confirm.nuke_ok' : 'disk.confirm.recycle_ok'),
      danger: true,
    });
    if (!ok) return;
    // backend xoá rồi cập nhật cây NGAY (mục đã mất: trừ số liệu; xoá một phần: chỉ đo lại nhánh đó), không quét lại ổ.
    // Windows báo lỗi cũng có thể đã xoá được một phần (đã giải phóng chỗ) -> luôn đọc lại số liệu.
    let res = null;
    let err = null;
    try { res = await api.duDelete([tg.path], permanent); } catch (e) { err = e; }
    if (res?.error) err = res.error;
    let gone = false;
    try { gone = !(await api.duExists(tg.path)); } catch {}
    if (tg.r.type === 'dir') du.deletedDir(tg.r.id, gone);
    else if (gone) du.deletedFile(tg.r.dir, tg.r.f);
    if (gone) toasts.success(t(permanent ? 'disk.toast.nuked' : 'disk.toast.recycled', { name: tg.r.n?.name || tg.r.f?.name }), permanent ? [] : [t('disk.toast.recycled_hint')]);
    else if (err) toasts.error(t(tg.isDir ? 'disk.toast.del_dir_failed' : 'disk.toast.del_file_failed'), [errText(err)]);
    else toasts.error(t('disk.toast.partial'), [t('disk.toast.partial_detail')]);
    if (res?.inexact?.length) toasts.warn(t('disk.toast.inexact'), [t('disk.toast.inexact_detail')]);
  }
  async function properties(tg) {
    try { await api.procProperties(tg.path); } catch (e) { toasts.error(t('disk.toast.props_failed'), [errText(e)]); }
  }

  async function openMenu(r, x, y, shift) {
    select(r);
    const tg = targetOf(r);
    if (!tg || tg.filesRow) return;
    if (isTauri) {
      try {
        const pick = await api.duMenu(tg.path, tg.isDir, shift);
        if (pick?.kind === 'app') {
          if (pick.id === 'scan') du.start(tg.path);
          else if (pick.id === 'ask_ai') ai.askFolder(tg.path);
          else if (pick.id === 'copy_path') copyPath(tg);
        } else if (pick?.kind === 'shell') {
          // lệnh shell (xoá, cắt, đổi chỗ…) có thể chạy nền: đồng bộ ngay, nếu mục còn đó thì kiểm tra lại sau 2,5 s
          if (!READ_ONLY_VERBS.has(String(pick.verb || '').toLowerCase()) && !(await syncShell(tg))) setTimeout(() => syncShell(tg), 2500);
        }
        return;
      } catch {
        // không mở được menu Windows -> dùng menu riêng
      }
    }
    menu = { x, y, tg };
  }
  const menuItems = $derived.by(() => {
    if (!menu) return [];
    const tg = menu.tg;
    const root = tg.r.type === 'dir' && tg.r.id === 0;
    return [
      { id: 'open', label: t(tg.isDir ? 'disk.menu.open_dir' : 'disk.menu.reveal'), glyph: 'folder', hint: 'Enter' },
      ...(tg.isDir && !root ? [{ id: 'scan', label: t('disk.menu.scan'), glyph: 'search' }] : []),
      ...(tg.isDir ? [{ id: 'ask_ai', label: t('disk.menu.ask_ai'), glyph: 'chat' }] : []),
      { id: 'copy', label: t('disk.menu.copy'), glyph: 'copy' },
      { sep: true },
      { id: 'del', label: t('disk.menu.del'), glyph: 'delete', hint: 'Delete', disabled: root },
      { id: 'nuke', label: t('disk.menu.nuke'), glyph: 'delete', hint: 'Shift+Delete', danger: true, disabled: root },
      { sep: true },
      { id: 'props', label: t('disk.menu.props'), glyph: 'properties' },
    ];
  });
  function pick(id) {
    const tg = menu?.tg;
    if (!tg) return;
    if (id === 'open') open(tg);
    else if (id === 'scan') du.start(tg.path);
    else if (id === 'ask_ai') ai.askFolder(tg.path);
    else if (id === 'copy') copyPath(tg);
    else if (id === 'del') del(tg, false);
    else if (id === 'nuke') del(tg, true);
    else if (id === 'props') properties(tg);
  }

  async function pickFolder() {
    if (!isTauri) {
      const p = prompt(t('disk.prompt.path'), 'C:\\Users');
      if (p) du.start(p);
      return;
    }
    try {
      const p = await api.duPickFolder();
      if (p) du.start(p);
    } catch (e) {
      toasts.error(t('disk.toast.picker_failed'), [errText(e)]);
    }
  }
  function startDrive(d) {
    drivesOpen = false;
    du.start(d.root);
  }
  function toggleDrives() {
    drivesOpen = !drivesOpen;
    if (drivesOpen) du.loadDrives();
  }
</script>

<section class="page disk-page">
  <header class="page-head">
    <div>
      <h1>{t('disk.title')}</h1>
      <p class="muted">{t('disk.subtitle')}</p>
    </div>
    {#if du.scan}
      <div class="ph-actions">
        {#if du.scanning}
          <button type="button" class="btn" onclick={() => du.cancel()}><Glyph name="stop" size={12} /> {t('disk.stop')}</button>
        {:else}
          <RefreshButton onclick={() => du.start(du.root)} disabled={du.starting} label={t('disk.rescan')} />
        {/if}
        <button type="button" class="btn" onclick={() => du.close()} title={t('disk.close_tip')}><Glyph name="close" size={11} /> {t('disk.close')}</button>
      </div>
    {/if}
  </header>

  <div class="toolbar du-bar">
    <div class="du-drives-wrap">
      <button type="button" class="btn" aria-haspopup="menu" aria-expanded={drivesOpen} onclick={toggleDrives}><Glyph name="disk" size={13} /> {t('disk.pick_drive')} <Glyph name="chevronDown" size={9} /></button>
      {#if drivesOpen}
        <div class="cm-backdrop" role="presentation" onpointerdown={() => (drivesOpen = false)}></div>
        <div class="du-drives" role="menu" aria-label={t('disk.drives')} tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (drivesOpen = false)}>
          <div class="du-drives-title">{t('disk.drives')}</div>
          {#each du.drives as d (d.root)}
            <button type="button" role="menuitem" class="du-drive" disabled={!d.ready} onclick={() => startDrive(d)}>
              <Glyph name={d.kind === 'network' ? 'network' : 'disk'} size={15} />
              <span class="ellipsis">{driveName(d)}</span>
              {#if d.ready}<span class="faint small">{fmtBytes(d.total - d.free, 0)} / {fmtBytes(d.total, 0)}</span>{/if}
            </button>
          {:else}
            <div class="muted small" style="padding:8px 10px">{du.drivesLoading ? t('disk.drives_loading') : t('disk.no_drives')}</div>
          {/each}
        </div>
      {/if}
    </div>
    <button type="button" class="btn" onclick={pickFolder}><Glyph name="folder" size={13} /> {t('disk.pick_folder')}</button>
    <form class="du-path" onsubmit={(e) => { e.preventDefault(); du.start(pathInput); }}>
      <Glyph name="search" size={12} />
      <input type="text" bind:value={pathInput} placeholder={t('disk.path_ph')} spellcheck="false" aria-label={t('disk.path_label')} onfocus={() => (pathFocused = true)} onblur={() => (pathFocused = false)} />
    </form>
  </div>

  {#if !du.scan}
    <!-- chưa quét: chọn nhanh một ổ -->
    <div class="du-pick">
      {#each du.drives as d (d.root)}
        {@const used = d.total - d.free}
        <button type="button" class="du-card" disabled={!d.ready || du.starting} onclick={() => du.start(d.root)}>
          <div class="du-card-head"><Glyph name={d.kind === 'network' ? 'network' : 'disk'} size={18} /><b class="ellipsis">{driveName(d)}</b></div>
          {#if d.ready}
            <div class="res-bar"><span style="width:{pctOf(used, d.total)}%; background:{pctOf(used, d.total) > 90 ? 'var(--red)' : 'var(--accent)'}"></span></div>
            <div class="muted small">{t('disk.card.free', { free: fmtBytes(d.free), total: fmtBytes(d.total) })}{d.fs ? ` · ${d.fs}` : ''}</div>
          {:else}
            <div class="muted small">{d.kind === 'network' ? t('disk.card.network') : t('disk.card.not_ready')}</div>
          {/if}
        </button>
      {/each}
      <button type="button" class="du-card" onclick={pickFolder}>
        <div class="du-card-head"><Glyph name="folder" size={18} /><b>{t('disk.card.folder')}</b></div>
        <div class="muted small">{t('disk.card.folder_hint')}</div>
      </button>
    </div>
  {:else}
    {@const p = du.progress}
    <div class="du-status small" role="status">
      {#if p && !p.done}
        {@const sp = around('disk.status.scanning', 'root')}
        <span class="spin sm"></span>
        <span>{sp[0]}<b>{du.root}</b>{sp[1]}</span>
      {:else if p?.cancelled}
        <Glyph name="warning" size={12} /><span>{t('disk.status.cancelled')}</span>
      {:else}
        <Glyph name="check" size={12} /><span>{t('disk.status.done', { time: fmtSec(p?.elapsed_ms || 0) })}</span>
      {/if}
      {#if p}
        <span class="muted">{t('disk.status.stats', { files: fmtInt(p.files), dirs: fmtInt(p.dirs), bytes: fmtBytes(p.bytes) })}</span>
        {#if !p.done && rate}<span class="muted">{t('disk.status.rate', { rate: fmtInt(rate), time: fmtSec(p.elapsed_ms) })}</span>{/if}
        {#if du.rootDrive}{@const dr = du.rootDrive}<span class="muted" title={t('disk.status.free_tip')}>{t('disk.status.free', { letter: dr.letter, free: fmtBytes(dr.free), total: fmtBytes(dr.total) })}</span>{/if}
        {#if p.errors}<span class="warn-text" title={t('disk.status.errors_tip')}>{t('disk.status.errors', { n: fmtInt(p.errors) })}</span>{/if}
      {/if}
    </div>

    <div class="pt du-table" style="--row-h:{ROW_H}px">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="pt-scroll" bind:this={scroller} bind:clientHeight={viewH} onscroll={() => (scrollTop = scroller.scrollTop)} onkeydown={onKey} tabindex="0" role="treegrid" aria-label={t('disk.tree.label')} aria-rowcount={rows.length}>
        <div class="du-head" role="row" style="grid-template-columns:{template}">
          {#each COLS as c (c.key)}
            {@const sk = c.sortKey || c.key}
            <button type="button" class="du-th" class:r={c.r} class:sorted={du.sort.key === sk && c.key !== 'pct'} role="columnheader" aria-sort={du.sort.key === sk ? (du.sort.desc ? 'descending' : 'ascending') : 'none'} onclick={() => du.setSort(sk)}>
              <span class="ellipsis">{c.label}</span><span class="arrow">{du.sort.key === sk && c.key !== 'pct' ? (du.sort.desc ? '↓' : '↑') : ''}</span>
            </button>
          {/each}
        </div>
        <div class="pt-body" style="height:{rows.length * ROW_H}px">
          <div class="dt-window" style="transform:translateY({start * ROW_H}px)">
            {#each vis as r (r.key)}
              {#if r.type === 'note'}
                <div class="du-row note" role="row" style="grid-template-columns:{template}">
                  <div class="pc" style="padding-left:{8 + r.depth * 18 + 18}px">{#if r.busy}<span class="spin sm"></span>&nbsp;{/if}<span class:warn-text={r.warn} class="muted">{r.text}</span></div>
                </div>
              {:else}
                {@const size = r.type === 'dir' ? r.n.size : r.type === 'files' ? r.size : r.f.size}
                {@const alloc = r.type === 'dir' ? r.n.alloc : r.type === 'files' ? r.alloc : r.f.alloc}
                {@const pct = pctOf(size, r.parentSize)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                  class="du-row"
                  class:sel={selKey === r.key}
                  class:root={r.depth === 0}
                  role="row"
                  tabindex="-1"
                  aria-selected={selKey === r.key}
                  aria-expanded={r.expandable ? r.open : undefined}
                  aria-level={r.depth + 1}
                  style="grid-template-columns:{template}"
                  onclick={() => select(r)}
                  ondblclick={() => (r.type === 'file' ? open(targetOf(r)) : toggleRow(r))}
                  oncontextmenu={(e) => { e.preventDefault(); openMenu(r, e.clientX, e.clientY, e.shiftKey); }}
                >
                  <div class="pc name" role="gridcell" style="padding-left:{4 + r.depth * 18}px">
                    {#if r.expandable}
                      <button type="button" class="exp" tabindex="-1" aria-label={r.open ? t('disk.tree.collapse') : t('disk.tree.expand')} onclick={(e) => { e.stopPropagation(); toggleRow(r); }}><Glyph name={r.open ? 'chevronDown' : 'chevronRight'} size={9} /></button>
                    {:else}<span class="exp"></span>{/if}
                    <span class="du-chip" title={t('disk.tree.pct_tip', { pct: fmtPct(pct, 1) })}><i style="width:{pct}%"></i><b>{fmtBytes(size)}</b></span>
                    {#if r.type === 'dir'}
                      <FsIcon kind={r.depth === 0 ? 'disk' : 'folder'} link={r.n.link} denied={r.n.denied} />
                      <span class="ellipsis" class:b={r.depth === 0} title={du.paths.get(r.id)}>{r.depth === 0 ? du.root : r.n.name}</span>
                      {#if r.n.link}<span class="tag" title={t('disk.tree.link_tip')}>{t('disk.tree.link')}</span>{/if}
                      {#if r.n.denied}<span class="warn-text" title={t('disk.tree.denied_tip')}><Glyph name="lock" size={11} /></span>{/if}
                      {#if !r.n.done && !r.n.link && !r.n.denied}<span class="spin xs" title={t('disk.tree.scanning_tip')}></span>{/if}
                    {:else if r.type === 'files'}
                      <FsIcon kind="files" size={13} />
                      <span class="muted">{t('disk.tree.files_row', { count: r.count, n: fmtInt(r.count) })}</span>
                    {:else}
                      <FsIcon kind="file" size={13} />
                      <span class="ellipsis" title={r.f.name}>{r.f.name}</span>
                    {/if}
                  </div>
                  <div class="pc r" role="gridcell">{fmtBytes(size)}</div>
                  <div class="pc r muted" role="gridcell">{fmtBytes(alloc)}</div>
                  <div class="pc r" role="gridcell">{r.type === 'dir' ? fmtInt(r.n.files) : r.type === 'files' ? fmtInt(r.count) : ''}</div>
                  <div class="pc r" role="gridcell">{r.type === 'dir' ? fmtInt(r.n.dirs) : ''}</div>
                  <div class="pc" role="gridcell"><span class="du-pct"><i style="width:{pct}%"></i><b>{fmtPct(pct, 1)}</b></span></div>
                  <div class="pc muted" role="gridcell">{r.type === 'dir' ? fmtDate(r.n.mtime) : r.type === 'file' ? fmtDate(r.f.mtime) : ''}</div>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      </div>
    </div>
  {/if}
</section>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menuItems} label={t('disk.menu.label')} onpick={pick} onclose={() => { menu = null; scroller?.focus({ preventScroll: true }); }} />
{/if}
