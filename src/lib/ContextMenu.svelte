<script>
  // Menu chuột phải tự viết: bàn phím (↑ ↓ Home End Enter, → mở menu con, ← đóng, Esc), bấm ra ngoài để đóng.
  // items: [{ id, label, glyph?, hint?, danger?, disabled?, sep?, sub?: items[] }]
  import { tick } from 'svelte';
  import Glyph from './Glyph.svelte';

  let { x, y, items, onpick, onclose, label = 'Menu' } = $props();

  let el = $state();
  let subEl = $state();
  let pos = $state({ x: 0, y: 0 });
  let subOpen = $state(-1);
  let subPos = $state({ x: 0, y: 0 });

  const usable = (list) => list.map((it, i) => (!it.sep && !it.disabled ? i : -1)).filter((i) => i >= 0);

  $effect(() => {
    // đặt vị trí trong khung nhìn rồi focus mục đầu
    x; y;
    tick().then(() => {
      if (!el) return;
      const r = el.getBoundingClientRect();
      pos = { x: Math.max(4, Math.min(x, innerWidth - r.width - 4)), y: Math.max(4, Math.min(y, innerHeight - r.height - 4)) };
      el.querySelector('[role=menuitem]:not([disabled])')?.focus();
    });
  });

  function focusIn(container, dir) {
    const btns = [...container.querySelectorAll(':scope > [role=menuitem]:not([disabled])')];
    if (!btns.length) return;
    const i = btns.indexOf(document.activeElement);
    const n = dir === 'first' ? 0 : dir === 'last' ? btns.length - 1 : (i + dir + btns.length) % btns.length;
    btns[n].focus();
  }

  async function openSub(i, btn, focus = false) {
    subOpen = i;
    await tick();
    if (!subEl) return;
    const r = btn.getBoundingClientRect();
    const s = subEl.getBoundingClientRect();
    let sx = r.right - 2;
    if (sx + s.width > innerWidth - 4) sx = r.left - s.width + 2;
    subPos = { x: sx, y: Math.max(4, Math.min(r.top - 4, innerHeight - s.height - 4)) };
    if (focus) subEl.querySelector('[role=menuitem]:not([disabled])')?.focus();
  }

  function pick(it) {
    if (it.disabled || it.sub) return;
    onpick(it.id);
    onclose();
  }

  function key(e, inSub) {
    const c = inSub ? subEl : el;
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); focusIn(c, 1); break;
      case 'ArrowUp': e.preventDefault(); focusIn(c, -1); break;
      case 'Home': e.preventDefault(); focusIn(c, 'first'); break;
      case 'End': e.preventDefault(); focusIn(c, 'last'); break;
      case 'ArrowRight': {
        if (inSub) break;
        const b = document.activeElement;
        const i = Number(b?.dataset?.idx);
        if (items[i]?.sub) { e.preventDefault(); openSub(i, b, true); }
        break;
      }
      case 'ArrowLeft':
        if (inSub) {
          e.preventDefault();
          const i = subOpen;
          subOpen = -1;
          el.querySelector(`[data-idx="${i}"]`)?.focus();
        }
        break;
      case 'Escape':
        e.preventDefault();
        e.stopPropagation();
        if (inSub) { const i = subOpen; subOpen = -1; el.querySelector(`[data-idx="${i}"]`)?.focus(); }
        else onclose();
        break;
      case 'Tab':
        e.preventDefault();
        focusIn(c, e.shiftKey ? -1 : 1);
        break;
    }
  }
</script>

<div class="cm-backdrop" role="presentation" onpointerdown={onclose} oncontextmenu={(e) => { e.preventDefault(); onclose(); }}></div>
<div class="cmenu" bind:this={el} role="menu" aria-label={label} tabindex="-1" style="left:{pos.x}px; top:{pos.y}px" onkeydown={(e) => key(e, false)}>
  {#each items as it, i (i)}
    {#if it.sep}
      <div class="cm-sep" role="separator"></div>
    {:else}
      <button
        type="button"
        role="menuitem"
        class="cm-item"
        class:danger={it.danger}
        class:open={subOpen === i}
        data-idx={i}
        disabled={it.disabled}
        aria-haspopup={it.sub ? 'menu' : undefined}
        aria-expanded={it.sub ? subOpen === i : undefined}
        onclick={(e) => (it.sub ? openSub(i, e.currentTarget, true) : pick(it))}
        onpointerenter={(e) => (it.sub ? openSub(i, e.currentTarget) : (subOpen = -1))}
      >
        <span class="cm-g">{#if it.glyph}<Glyph name={it.glyph} size={14} />{/if}</span>
        <span class="cm-l">{it.label}</span>
        {#if it.hint}<span class="cm-hint">{it.hint}</span>{/if}
        {#if it.sub}<span class="cm-hint"><Glyph name="chevronRight" size={10} /></span>{/if}
      </button>
    {/if}
  {/each}
</div>
{#if subOpen >= 0 && items[subOpen]?.sub}
  <div class="cmenu sub" bind:this={subEl} role="menu" tabindex="-1" aria-label={items[subOpen].label} style="left:{subPos.x}px; top:{subPos.y}px" onkeydown={(e) => key(e, true)}>
    {#each items[subOpen].sub as it, i (i)}
      {#if it.sep}
        <div class="cm-sep" role="separator"></div>
      {:else}
        <button type="button" role="menuitem" class="cm-item" class:danger={it.danger} class:checked={it.checked} disabled={it.disabled} onclick={() => pick(it)}>
          <span class="cm-g">{#if it.checked}<Glyph name="check" size={12} />{/if}</span>
          <span class="cm-l">{it.label}</span>
        </button>
      {/if}
    {/each}
  </div>
{/if}
