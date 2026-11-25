<script>
  // Panel trợ lý AI bên phải: luồng trò chuyện, cài đặt và kéo để đổi độ rộng.
  import { tick } from 'svelte';
  import { ai, PROVIDERS } from './ai.svelte.js';
  import { prefs } from './prefs.svelte.js';
  import { renderMarkdown, parseActionBlocks } from './markdown.js';
  import { copyText } from './format.js';
  import { toasts } from './toasts.svelte.js';
  import Glyph from './Glyph.svelte';
  import AiSettings from './AiSettings.svelte';
  import { t, hasT } from './i18n.svelte.js';

  // gợi ý + lời chào theo trang đang mở (ai.help.<trang>, ai.suggest.<trang>.1..n); chip gợi ý được GỬI cho model
  const SUGGEST_N = { disk: 3, ports: 3, apps: 3, cleanup: 3, general: 4 };
  const helpPage = $derived(
    ai.diskSuggest ? 'disk' : ai.portsSuggest ? 'ports' : ai.appsSuggest ? 'apps' : ai.cleanupSuggest ? 'cleanup' : 'general'
  );
  const pageHelp = $derived([
    t(`ai.help.${helpPage}`),
    Array.from({ length: SUGGEST_N[helpPage] }, (_, i) => t(`ai.suggest.${helpPage}.${i + 1}`)),
  ]);
  // nhãn bước tra cứu: dịch theo tên tool, tool lạ -> nhãn của backend
  const toolLabel = (s) => (hasT(`ai.tool.${s.name}`) ? t(`ai.tool.${s.name}`) : s.label);

  let list = $state();
  let input = $state();
  const provider = $derived(ai.cfg?.provider);
  let stick = true; // tự cuộn xuống cuối khi đang ở cuối

  $effect(() => {
    ai.focusTick;
    const composer = input;
    tick().then(() => composer?.focus());
  });

  // tự cuộn khi có nội dung mới
  $effect(() => {
    const last = ai.msgs[ai.msgs.length - 1];
    last?.text; last?.thinking; ai.msgs.length;
    if (stick && list) tick().then(() => { if (list) list.scrollTop = list.scrollHeight; });
  });
  function onScroll() {
    stick = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
  }

  // nút "Xem lại…" trong thẻ đề xuất hành động (HTML do markdown.js tạo, nên bắt sự kiện ở danh sách) -> ActionReview
  function listClick(e) {
    const btn = e.target.closest?.('button[data-act]');
    if (!btn) return;
    const id = +btn.closest('[data-msg]')?.dataset.msg;
    const m = ai.msgs.find((x) => x.id === id);
    const block = m && parseActionBlocks(m.text)[+btn.dataset.idx];
    if (block?.items.length && block.kind === btn.dataset.act) ai.review = block;
  }

  function key(e) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      stick = true;
      ai.send();
    }
  }
  function autosize() {
    if (!input) return;
    input.style.height = 'auto';
    input.style.height = Math.min(160, input.scrollHeight) + 'px';
  }
  $effect(() => { ai.draft; input; tick().then(autosize); });

  async function copy(m) {
    if (await copyText(m.text)) toasts.success(t('ai.toast.copied'));
    else toasts.error(t('ai.toast.copy_failed'));
  }

  // "pid: 42, name: chrome" – tham số gọn của một lần tra cứu
  const argText = (a) => Object.entries(a || {}).map(([k, v]) => `${k}: ${typeof v === 'object' ? JSON.stringify(v) : v}`).join(', ');

  // kéo mép trái để đổi độ rộng (320–600 px)
  function startResize(e) {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = prefs.aiWidth;
    const tgt = e.currentTarget;
    tgt.setPointerCapture(e.pointerId);
    let w = w0;
    const move = (ev) => { w = Math.max(320, Math.min(600, Math.round(w0 - (ev.clientX - x0)))); prefs.aiWidth = w; };
    const up = () => {
      tgt.removeEventListener('pointermove', move);
      tgt.removeEventListener('pointerup', up);
      tgt.removeEventListener('pointercancel', up);
      prefs.set('aiWidth', w);
    };
    tgt.addEventListener('pointermove', move);
    tgt.addEventListener('pointerup', up);
    tgt.addEventListener('pointercancel', up);
  }
  function resizeKey(e) {
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      prefs.set('aiWidth', Math.max(320, Math.min(600, prefs.aiWidth + (e.key === 'ArrowLeft' ? 20 : -20))));
    }
  }
  function panelKey(e) {
    if (e.key === 'Escape' && ai.showSettings) { e.stopPropagation(); ai.showSettings = false; }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="ai" style="width:{prefs.aiWidth}px" aria-label={t('ai.title')} onkeydown={panelKey}>
  <button type="button" class="ai-resizer" aria-label={t('ai.panel.resize_aria')} title={t('ai.panel.resize_tip')} onpointerdown={startResize} onkeydown={resizeKey}></button>
  <header class="ai-head">
    <span class="ai-logo"><Glyph name="chat" size={15} /></span>
    <div class="ai-title grow">
      <h2>{t('ai.title')}</h2>
      {#if ai.subtitle}<p class="muted small" title={ai.subtitle}>{ai.subtitle}</p>{/if}
    </div>
    <button type="button" class="icon-btn" title={t('ai.panel.new_chat')} aria-label={t('ai.panel.new_chat')} onclick={() => ai.newChat()}><Glyph name="add" size={14} /></button>
    <button type="button" class="icon-btn" class:on={ai.showSettings} title={t('ai.panel.settings')} aria-label={t('ai.panel.settings')} aria-pressed={ai.showSettings} onclick={() => (ai.showSettings = !ai.showSettings)}><Glyph name="settings" size={15} /></button>
    <button type="button" class="icon-btn" title={t('ai.panel.close_tip')} aria-label={t('ai.panel.close_aria')} onclick={() => ai.toggle(false)}><Glyph name="close" size={13} /></button>
  </header>

  {#if ai.showSettings}
    <div role="status">
      {#if ai.saveStatus}
        <p class="ai-save-status small" class:ai-error={ai.saveStatus.error}>
          <Glyph name={ai.saveStatus.error ? 'warning' : 'check'} size={12} /> {ai.saveStatus.text}
        </p>
      {/if}
    </div>
    <div class="ai-settings">
      <AiSettings />
      <button type="button" class="btn" onclick={() => (ai.showSettings = false)}>{t('ai.panel.done')}</button>
    </div>
  {:else}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="ai-list" bind:this={list} onscroll={onScroll} onclick={listClick}>
      {#if !ai.msgs.length}
        <div class="ai-empty">
          <span class="ai-big"><Glyph name="chat" size={28} /></span>
          <p class="muted">{pageHelp[0]}</p>
          {#if ai.ready}
            <div class="sugg">
              {#each pageHelp[1] as s (s)}<button type="button" class="chip" disabled={ai.saving} onclick={() => { stick = true; ai.send(s); }}>{s}</button>{/each}
            </div>
          {/if}
        </div>
      {/if}
      {#each ai.msgs as m (m.id)}
        {#if m.role === 'user'}
          <div class="msg user">{m.text}{#if m.disk}<span class="msg-tag"><Glyph name="disk" size={10} /> {t('ai.msg.with_disk')}</span>{/if}{#if m.ports}<span class="msg-tag"><Glyph name="network" size={10} /> {t('ai.msg.with_ports')}</span>{/if}{#if m.cleanup}<span class="msg-tag"><Glyph name="delete" size={10} /> {t('ai.msg.with_cleanup')}</span>{/if}</div>
        {:else if m.role === 'error'}
          <div class="msg err" role="alert"><Glyph name="warning" size={13} /> <span>{m.text}</span></div>
        {:else}
          <div class="msg bot" data-msg={m.id}>
            {#if m.tools?.length}
              <div class="ai-tools" aria-label={t('ai.tools.aria')}>
                {#each m.tools as s (s.id)}
                  <div class="ai-tool" class:fail={s.done && !s.ok} title={argText(s.args)}>
                    {#if !s.done}<span class="spin xs"></span>{:else if s.ok}<Glyph name="check" size={10} />{:else}<Glyph name="warning" size={10} />{/if}
                    <span>{t(s.done ? 'ai.tools.done' : 'ai.tools.running', { label: toolLabel(s) })}</span>
                    {#if argText(s.args)}<span class="faint ellipsis">· {argText(s.args)}</span>{/if}
                  </div>
                {/each}
              </div>
            {/if}
            {#if m.thinking && !m.text}
              <div class="thinking muted small"><span class="spin sm"></span> {t('ai.thinking')}</div>
            {:else if m.streaming && !m.text}
              <div class="thinking muted small"><span class="spin sm"></span> {t('ai.connecting')}</div>
            {/if}
            {#if m.text}
              <!-- renderMarkdown escape HTML trước rồi mới thêm thẻ của chính nó -->
              <div class="md">{@html renderMarkdown(m.text)}</div>
            {/if}
            {#if !m.streaming && m.text}
              <div class="msg-foot">
                <button type="button" class="icon-btn xs" title={t('ai.copy')} aria-label={t('ai.copy_aria')} onclick={() => copy(m)}><Glyph name="copy" size={12} /></button>
                {#if m.usage}<span class="faint small">{t('ai.usage', { input: m.usage.input_tokens, output: m.usage.output_tokens })}</span>{/if}
              </div>
            {/if}
          </div>
        {/if}
      {/each}
    </div>

    <footer class="ai-foot">
      {#if ai.cfg && !ai.ready && !ai.busy}
        <div class="setup-card">
          <b>{t('ai.setup.title', { provider: PROVIDERS[provider] })}</b>
          <p class="muted small">{t('ai.setup.missing', { list: ai.missingSettings.join(', ') })}</p>
          <button type="button" class="btn primary sm" onclick={() => (ai.showSettings = true)}><Glyph name="settings" size={13} /> {t('ai.setup.open')}</button>
        </div>
      {:else if ai.cfg}
        <div class="composer">
          <textarea bind:this={input} bind:value={ai.draft} onkeydown={key} oninput={autosize} rows="1" placeholder={t('ai.composer.placeholder')} aria-label={t('ai.composer.aria')}></textarea>
          {#if ai.busy}
            <button type="button" class="btn danger sm" title={t('ai.stop')} onclick={() => ai.cancel()}><Glyph name="stop" size={12} /> {t('ai.stop')}</button>
          {:else}
            <button type="button" class="btn primary sm" title={t('ai.send_tip')} disabled={ai.saving || !ai.draft.trim()} onclick={() => { stick = true; ai.send(); }}><Glyph name="send" size={13} /></button>
          {/if}
        </div>
      {:else if ai.loadError}
        <div class="setup-card">
          <p class="ai-error" role="alert">{t('ai.load_error', { error: ai.loadError })}</p>
          <button type="button" class="btn primary sm" onclick={() => (ai.showSettings = true)}><Glyph name="settings" size={13} /> {t('ai.setup.open')}</button>
        </div>
      {/if}
    </footer>
  {/if}
</aside>
